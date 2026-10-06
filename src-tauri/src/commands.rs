use crate::archive::{self, AccountManifest, ExportItem};
use crate::config::{self, Account, Config, LastRun};
use crate::error::{AppError, Result};
use crate::imap_sync::{self, Progress, SyncCtx, SyncReport};
use crate::index::{FolderInfo, Index, MessagePage, Query};
use crate::message::{self, MessageView};
use crate::store;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

pub struct AppState {
    pub data_dir: PathBuf,
    pub config: Mutex<Config>,
    pub cancel: AtomicBool,
    pub running: AtomicBool,
}

impl AppState {
    fn account(&self, id: &str) -> Result<Account> {
        self.config.lock().unwrap().get(id).cloned()
    }

    fn update<R>(&self, f: impl FnOnce(&mut Config) -> Result<R>) -> Result<R> {
        let mut c = self.config.lock().unwrap();
        let r = f(&mut c)?;
        c.save(&self.data_dir.join("config.json"))?;
        Ok(r)
    }
}

#[derive(Debug, Deserialize)]
pub struct AccountInput {
    pub id: Option<String>,
    pub name: String,
    pub email: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: Option<String>,
    pub dir: Option<String>,
}

#[derive(Serialize)]
pub struct BackupResult {
    pub account_id: String,
    pub report: Option<SyncReport>,
    pub error: Option<AppError>,
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn other(e: impl std::fmt::Display) -> AppError {
    AppError::Other(e.to_string())
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(other)?
}

fn open_index(acc: &Account) -> Result<Index> {
    std::fs::create_dir_all(&acc.dir)?;
    let (mut index, needs_reindex) = Index::open(&acc.dir.join("index.sqlite"))?;
    if needs_reindex {
        index.reindex(&acc.dir.join("mail"), &mut |_| {})?;
    }
    Ok(index)
}

fn message_path(acc: &Account, message_id: i64) -> Result<PathBuf> {
    let rel = open_index(acc)?.message_file(message_id)?;
    store::resolve(&acc.dir.join("mail"), &rel)
}

fn norm(p: &Path) -> PathBuf {
    let p = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    if cfg!(windows) { PathBuf::from(p.to_string_lossy().to_lowercase()) } else { p }
}

/// true se as pastas são iguais ou uma contém a outra
fn dirs_overlap(a: &Path, b: &Path) -> bool {
    let (a, b) = (norm(a), norm(b));
    a.starts_with(&b) || b.starts_with(&a)
}

fn password_for(input: &AccountInput) -> Result<String> {
    match (&input.password, &input.id) {
        (Some(p), _) if !p.is_empty() => Ok(p.clone()),
        (_, Some(id)) => config::get_password(id),
        _ => Err(other("informe a senha")),
    }
}

#[tauri::command]
pub async fn list_accounts(state: State<'_, AppState>) -> Result<Vec<Account>> {
    Ok(state.config.lock().unwrap().accounts.clone())
}

#[tauri::command]
pub async fn save_account(state: State<'_, AppState>, input: AccountInput) -> Result<Account> {
    let acc = state.update(|c| {
        if let Some(id) = &input.id {
            let a = c.get_mut(id)?;
            a.name = input.name.clone();
            a.email = input.email.clone();
            a.host = input.host.clone();
            a.port = input.port;
            a.username = input.username.clone();
            return Ok(a.clone());
        }
        let id = uuid::Uuid::new_v4().to_string();
        let dir = match input.dir.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
            Some(d) => {
                let d = PathBuf::from(d);
                if !d.is_absolute() {
                    return Err(other("pasta do backup inválida"));
                }
                if c.accounts.iter().any(|a| dirs_overlap(&a.dir, &d)) {
                    return Err(other("a pasta escolhida se sobrepõe à pasta de outra conta"));
                }
                d
            }
            None => state.data_dir.join("accounts").join(&id),
        };
        let a = Account {
            id, name: input.name.clone(), email: input.email.clone(), host: input.host.clone(), port: input.port,
            username: input.username.clone(), dir, has_credentials: false, last_run: None,
        };
        c.accounts.push(a.clone());
        Ok(a)
    })?;
    match input.password.as_deref().filter(|p| !p.is_empty()) {
        Some(pw) => {
            config::set_password(&acc.id, pw)?;
            state.update(|c| {
                let a = c.get_mut(&acc.id)?;
                a.has_credentials = true;
                Ok(a.clone())
            })
        }
        None => Ok(acc),
    }
}

#[tauri::command]
pub async fn delete_account(state: State<'_, AppState>, id: String, delete_files: bool) -> Result<()> {
    let acc = state.account(&id)?;
    state.update(|c| {
        c.accounts.retain(|a| a.id != id);
        Ok(())
    })?;
    config::delete_password(&id)?;
    let shared = state.config.lock().unwrap().accounts.iter().any(|a| dirs_overlap(&a.dir, &acc.dir));
    if delete_files && !shared {
        // só apaga o que o app criou; a pasta pode ter sido escolhida pelo usuário
        let mail = acc.dir.join("mail");
        if mail.exists() {
            std::fs::remove_dir_all(mail)?;
        }
        for f in ["index.sqlite", "index.sqlite-wal", "index.sqlite-shm"] {
            let _ = std::fs::remove_file(acc.dir.join(f));
        }
        let _ = std::fs::remove_dir(&acc.dir); // só remove se estiver vazia
    }
    Ok(())
}

#[tauri::command]
pub async fn test_connection(input: AccountInput) -> Result<()> {
    let pw = password_for(&input)?;
    let mut s = imap_sync::connect_tls(&input.host, input.port, &input.username, &pw).await?;
    let _ = imap_sync::timed(s.logout()).await;
    Ok(())
}

#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> Result<Option<String>> {
    Ok(app.dialog().file().blocking_pick_folder().and_then(|p| p.into_path().ok()).map(|p| p.to_string_lossy().into_owned()))
}

async fn backup_one(app: &AppHandle, state: &AppState, id: &str) -> Result<SyncReport> {
    let acc = state.account(id)?;
    if !acc.has_credentials {
        return Err(other("conta sem credenciais (somente leitura)"));
    }
    let pw = config::get_password(id)?;
    let mut index = open_index(&acc)?;
    let mail_root = acc.dir.join("mail");
    let progress = |p: Progress| {
        let _ = app.emit("backup://progress", p);
    };
    let ctx = SyncCtx { account_id: id, mail_root: &mail_root, cancel: &state.cancel, progress: &progress };
    let (host, user, pass, port) = (acc.host.as_str(), acc.username.as_str(), pw.as_str(), acc.port);
    imap_sync::sync_account(move || imap_sync::connect_tls(host, port, user, pass), &ctx, &mut index).await
}

#[tauri::command]
pub async fn run_backup(app: AppHandle, state: State<'_, AppState>, ids: Vec<String>) -> Result<Vec<BackupResult>> {
    if state.running.swap(true, Ordering::SeqCst) {
        return Err(other("já existe um backup em andamento"));
    }
    state.cancel.store(false, Ordering::SeqCst);
    let mut results = Vec::new();
    for id in ids {
        if state.cancel.load(Ordering::SeqCst) {
            break;
        }
        let outcome = backup_one(&app, &state, &id).await;
        let last = match &outcome {
            Ok(r) => LastRun { at: now(), new_messages: r.new_messages, failed_folders: r.failed_folders.clone(), error: None },
            Err(e) => LastRun { at: now(), new_messages: 0, failed_folders: vec![], error: Some(e.to_string()) },
        };
        let _ = state.update(|c| {
            c.get_mut(&id)?.last_run = Some(last);
            Ok(())
        });
        results.push(match outcome {
            Ok(r) => BackupResult { account_id: id, report: Some(r), error: None },
            Err(e) => BackupResult { account_id: id, report: None, error: Some(e) },
        });
    }
    state.running.store(false, Ordering::SeqCst);
    Ok(results)
}

#[tauri::command]
pub async fn cancel_backup(state: State<'_, AppState>) -> Result<()> {
    state.cancel.store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub async fn list_folders(state: State<'_, AppState>, account_id: String) -> Result<Vec<FolderInfo>> {
    let acc = state.account(&account_id)?;
    blocking(move || open_index(&acc)?.list_folders()).await
}

#[tauri::command]
pub async fn list_messages(state: State<'_, AppState>, account_id: String, query: Query) -> Result<MessagePage> {
    let acc = state.account(&account_id)?;
    blocking(move || open_index(&acc)?.list_messages(&query)).await
}

#[tauri::command]
pub async fn get_message(state: State<'_, AppState>, account_id: String, message_id: i64, allow_remote_images: bool) -> Result<MessageView> {
    let acc = state.account(&account_id)?;
    blocking(move || Ok(message::render(&std::fs::read(message_path(&acc, message_id)?)?, allow_remote_images))).await
}

#[tauri::command]
pub async fn open_eml(app: AppHandle, state: State<'_, AppState>, account_id: String, message_id: i64) -> Result<()> {
    let path = message_path(&state.account(&account_id)?, message_id)?;
    app.opener().open_path(path.to_string_lossy(), None::<&str>).map_err(other)
}

fn load_attachment(state: &AppState, account_id: &str, message_id: i64, index: usize) -> Result<(String, Vec<u8>)> {
    let raw = std::fs::read(message_path(&state.account(account_id)?, message_id)?)?;
    message::attachment(&raw, index).ok_or_else(|| other("anexo não encontrado"))
}

#[tauri::command]
pub async fn save_attachment(app: AppHandle, state: State<'_, AppState>, account_id: String, message_id: i64, index: usize) -> Result<bool> {
    let (name, bytes) = load_attachment(&state, &account_id, message_id, index)?;
    let Some(dest) = app.dialog().file().set_file_name(store::sanitize_component(&name)).blocking_save_file() else {
        return Ok(false);
    };
    std::fs::write(dest.into_path().map_err(other)?, bytes)?;
    Ok(true)
}

#[tauri::command]
pub async fn open_attachment(app: AppHandle, state: State<'_, AppState>, account_id: String, message_id: i64, index: usize) -> Result<()> {
    let (name, bytes) = load_attachment(&state, &account_id, message_id, index)?;
    if message::is_blocked(&name) {
        return Err(other("por segurança, este tipo de arquivo só pode ser salvo, não aberto"));
    }
    let path = std::env::temp_dir().join("mail-backup").join(uuid::Uuid::new_v4().to_string()).join(store::sanitize_component(&name));
    store::write_atomic(&path, &bytes)?;
    app.opener().open_path(path.to_string_lossy(), None::<&str>).map_err(other)
}

#[tauri::command]
pub async fn reindex_account(state: State<'_, AppState>, account_id: String) -> Result<u64> {
    let acc = state.account(&account_id)?;
    blocking(move || open_index(&acc)?.reindex(&acc.dir.join("mail"), &mut |_| {})).await
}

#[tauri::command]
pub async fn export_accounts(app: AppHandle, state: State<'_, AppState>, ids: Vec<String>, password: Option<String>) -> Result<bool> {
    let accounts = ids.iter().map(|id| state.account(id)).collect::<Result<Vec<_>>>()?;
    let Some(dest) = app.dialog().file().add_filter("Backup de e-mails", &["zip"]).set_file_name("backup-emails.zip").blocking_save_file() else {
        return Ok(false);
    };
    let dest = dest.into_path().map_err(other)?;
    blocking(move || {
        let manifests = accounts
            .iter()
            .map(|a| {
                Ok(AccountManifest {
                    name: a.name.clone(), email: a.email.clone(), host: a.host.clone(), port: a.port,
                    username: a.username.clone(), folders: open_index(a)?.folders_state()?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let items: Vec<ExportItem> = accounts.iter().zip(manifests).map(|(a, manifest)| ExportItem { manifest, dir: &a.dir }).collect();
        archive::export(&dest, &items, password.as_deref().filter(|p| !p.is_empty()))
    })
    .await?;
    Ok(true)
}

#[tauri::command]
pub async fn pick_archive(app: AppHandle) -> Result<Option<String>> {
    Ok(app
        .dialog()
        .file()
        .add_filter("Backup de e-mails", &["zip"])
        .blocking_pick_file()
        .and_then(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned()))
}

#[tauri::command]
pub async fn import_archive(state: State<'_, AppState>, path: String, password: Option<String>) -> Result<Vec<Account>> {
    let root = state.data_dir.join("accounts");
    let imported = blocking(move || {
        archive::import(Path::new(&path), &root, password.as_deref().filter(|p| !p.is_empty()), || uuid::Uuid::new_v4().to_string())
    })
    .await?;
    let mut added = Vec::new();
    for imp in imported {
        if !imp.dir.exists() {
            continue;
        }
        let id = imp.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let m = imp.manifest;
        let email = m.as_ref().map_or_else(|| imp.top.clone(), |m| m.email.clone());
        let exists = state.config.lock().unwrap().accounts.iter().any(|a| a.email.eq_ignore_ascii_case(&email));
        let name = if exists { format!("{email} (importado)") } else { m.as_ref().map_or_else(|| email.clone(), |m| m.name.clone()) };
        let acc = Account {
            id, name, email,
            host: m.as_ref().map_or_else(String::new, |m| m.host.clone()),
            port: m.as_ref().map_or(993, |m| m.port),
            username: m.as_ref().map_or_else(String::new, |m| m.username.clone()),
            dir: imp.dir.clone(), has_credentials: false, last_run: None,
        };
        let folders = m.map(|m| m.folders).unwrap_or_default();
        let dir = acc.dir.clone();
        blocking(move || {
            let (mut index, _) = Index::open(&dir.join("index.sqlite"))?;
            index.restore_folders(&folders)?;
            index.reindex(&dir.join("mail"), &mut |_| {})?;
            Ok(())
        })
        .await?;
        state.update(|c| {
            c.accounts.push(acc.clone());
            Ok(())
        })?;
        added.push(acc);
    }
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_cases() {
        let p = Path::new;
        assert!(dirs_overlap(p("C:/a/b"), p("C:/a/b")));
        assert!(dirs_overlap(p("C:/a"), p("C:/a/b")));
        assert!(dirs_overlap(p("C:/a/b"), p("C:/a")));
        assert!(!dirs_overlap(p("C:/a/b"), p("C:/a/bc")));
        assert_eq!(dirs_overlap(p("C:/A/B"), p("C:/a/b")), cfg!(windows));
    }
}
