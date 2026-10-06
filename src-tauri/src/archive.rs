use crate::error::{AppError, Result};
use crate::index::FolderState;
use crate::store;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use zip::result::ZipError;
use zip::write::{FileOptions, SimpleFileOptions};
use zip::{AesMode, CompressionMethod, ZipArchive, ZipWriter};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccountManifest {
    pub name: String,
    pub email: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub folders: Vec<FolderState>,
}

pub struct ExportItem<'a> {
    pub manifest: AccountManifest,
    pub dir: &'a Path,
}

#[derive(Debug)]
pub struct ImportedAccount {
    pub dir: PathBuf,
    pub top: String,
    pub manifest: Option<AccountManifest>,
}

fn zip_err(e: ZipError) -> AppError {
    match e {
        ZipError::Io(io) => AppError::Io(io),
        ZipError::InvalidPassword => AppError::WrongPassword,
        ZipError::UnsupportedArchive(m) if m == "Password required to decrypt file" => AppError::WrongPassword,
        other => AppError::CorruptArchive(other.to_string()),
    }
}

fn options(password: Option<&str>) -> FileOptions<'_, ()> {
    let o = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    match password {
        Some(p) => o.with_aes_encryption(AesMode::Aes256, p),
        None => o,
    }
}

pub fn export(dest: &Path, items: &[ExportItem], password: Option<&str>) -> Result<()> {
    let mut tmp = dest.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let result = (|| -> Result<()> {
        let mut zip = ZipWriter::new(File::create(&tmp)?);
        let opts = options(password);
        let mut used = HashSet::new();
        for item in items {
            let top = store::unique_path(&store::sanitize_component(&item.manifest.email), &used);
            used.insert(top.to_lowercase());
            zip.start_file(format!("{top}/account.json"), opts.clone()).map_err(zip_err)?;
            serde_json::to_writer_pretty(&mut zip, &item.manifest).map_err(|e| AppError::Other(e.to_string()))?;
            for (abs, rel) in store::walk_files(&item.dir.join("mail"))? {
                if !rel.ends_with(".eml") {
                    continue;
                }
                zip.start_file(format!("{top}/mail/{rel}"), opts.clone()).map_err(zip_err)?;
                io::copy(&mut File::open(&abs)?, &mut zip)?;
            }
        }
        drop(zip.finish().map_err(zip_err)?);
        Ok(())
    })();
    match result {
        Ok(()) => Ok(fs::rename(&tmp, dest)?),
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

pub fn import(src: &Path, accounts_root: &Path, password: Option<&str>, mut new_id: impl FnMut() -> String) -> Result<Vec<ImportedAccount>> {
    let mut created: Vec<ImportedAccount> = Vec::new();
    let result = (|| -> Result<()> {
        let mut archive = ZipArchive::new(File::open(src)?).map_err(zip_err)?;
        let mut by_top: HashMap<String, usize> = HashMap::new();
        for i in 0..archive.len() {
            let mut entry = match password {
                Some(p) => archive.by_index_decrypt(i, p.as_bytes()),
                None => archive.by_index(i),
            }
            .map_err(zip_err)?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().to_string();
            if !store::is_safe_rel(&name) {
                continue; // UnsafePath: entrada ignorada (zip-slip)
            }
            let Some((top, rest)) = name.split_once('/') else { continue };
            let idx = *by_top.entry(top.to_string()).or_insert_with(|| {
                created.push(ImportedAccount { dir: accounts_root.join(new_id()), top: top.to_string(), manifest: None });
                created.len() - 1
            });
            let acc = &mut created[idx];
            if rest == "account.json" {
                acc.manifest = Some(serde_json::from_reader(&mut entry).map_err(|e| AppError::CorruptArchive(e.to_string()))?);
            } else if let Some(rel) = rest.strip_prefix("mail/").filter(|r| r.ends_with(".eml")) {
                let out = store::join_rel(&acc.dir.join("mail"), rel);
                if let Some(parent) = out.parent() {
                    fs::create_dir_all(parent)?;
                }
                io::copy(&mut entry, &mut File::create(&out)?).map_err(|e| match e.kind() {
                    io::ErrorKind::InvalidData | io::ErrorKind::InvalidInput => AppError::CorruptArchive(e.to_string()),
                    _ => AppError::Io(e),
                })?;
            }
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok(created),
        Err(e) => {
            for a in &created {
                let _ = fs::remove_dir_all(&a.dir);
            }
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn manifest(email: &str) -> AccountManifest {
        AccountManifest {
            name: "Joao".into(), email: email.into(), host: "imap.uhserver.com".into(), port: 993, username: email.into(),
            folders: vec![FolderState { id: 1, imap_name: Some("INBOX".into()), path: "INBOX".into(), uidvalidity: Some(7), last_uid: 2 }],
        }
    }

    fn account_dir(root: &Path) -> PathBuf {
        let dir = root.join("conta");
        store::write_atomic(&dir.join("mail/INBOX/7-00001.eml"), b"um").unwrap();
        store::write_atomic(&dir.join("mail/Itens Enviados/A\u{e7}\u{e3}o/7-00002.eml"), b"dois").unwrap();
        store::write_atomic(&dir.join("index.sqlite"), b"nao exportar").unwrap();
        dir
    }

    fn ids() -> impl FnMut() -> String {
        let mut n = 0;
        move || { n += 1; format!("id{n}") }
    }

    #[test]
    fn roundtrip_with_password_preserves_bytes_and_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = account_dir(tmp.path());
        let zip_path = tmp.path().join("b.zip");
        export(&zip_path, &[ExportItem { manifest: manifest("a@b.com"), dir: &dir }], Some("s3nha")).unwrap();
        assert!(!tmp.path().join("b.zip.tmp").exists());
        let out = tmp.path().join("accounts");
        let imported = import(&zip_path, &out, Some("s3nha"), ids()).unwrap();
        assert_eq!(imported.len(), 1);
        let acc = &imported[0];
        assert_eq!(acc.top, "a@b.com");
        assert_eq!(acc.manifest.as_ref().unwrap(), &manifest("a@b.com"));
        assert_eq!(std::fs::read(acc.dir.join("mail/INBOX/7-00001.eml")).unwrap(), b"um");
        assert_eq!(std::fs::read(acc.dir.join("mail/Itens Enviados/A\u{e7}\u{e3}o/7-00002.eml")).unwrap(), b"dois");
        assert!(!acc.dir.join("index.sqlite").exists());
    }

    #[test]
    fn wrong_or_missing_password_fails_and_cleans_up() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = account_dir(tmp.path());
        let zip_path = tmp.path().join("b.zip");
        export(&zip_path, &[ExportItem { manifest: manifest("a@b.com"), dir: &dir }], Some("s3nha")).unwrap();
        let out = tmp.path().join("accounts");
        assert!(matches!(import(&zip_path, &out, Some("errada"), ids()), Err(AppError::WrongPassword)));
        assert!(matches!(import(&zip_path, &out, None, ids()), Err(AppError::WrongPassword)));
        assert!(!out.exists() || std::fs::read_dir(&out).unwrap().next().is_none());
    }

    #[test]
    fn export_without_password_imports_without_password() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = account_dir(tmp.path());
        let zip_path = tmp.path().join("b.zip");
        export(&zip_path, &[ExportItem { manifest: manifest("a@b.com"), dir: &dir }], None).unwrap();
        let imported = import(&zip_path, &tmp.path().join("accounts"), None, ids()).unwrap();
        assert_eq!(imported.len(), 1);
    }

    #[test]
    fn export_two_accounts_same_email_keeps_both() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = account_dir(tmp.path());
        let zip_path = tmp.path().join("b.zip");
        let items = [ExportItem { manifest: manifest("a@b.com"), dir: &dir }, ExportItem { manifest: manifest("A@b.com"), dir: &dir }];
        export(&zip_path, &items, None).unwrap();
        let imported = import(&zip_path, &tmp.path().join("accounts"), None, ids()).unwrap();
        let tops: Vec<&str> = imported.iter().map(|a| a.top.as_str()).collect();
        assert_eq!(tops, vec!["a@b.com", "A@b.com~2"]);
        assert_ne!(imported[0].dir, imported[1].dir);
    }

    #[test]
    fn zip_slip_entries_are_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        let zip_path = tmp.path().join("evil.zip");
        {
            let mut w = ZipWriter::new(File::create(&zip_path).unwrap());
            for name in ["../evil.eml", "acc/mail/../../evil2.eml", "acc/mail/INBOX/1-00001.eml", "acc/mail/INBOX/run.exe"] {
                w.start_file(name, SimpleFileOptions::default()).unwrap();
                w.write_all(b"x").unwrap();
            }
            w.finish().unwrap();
        }
        let out = tmp.path().join("accounts");
        let imported = import(&zip_path, &out, None, ids()).unwrap();
        assert_eq!(imported.len(), 1);
        assert!(imported[0].dir.join("mail/INBOX/1-00001.eml").exists());
        assert!(!tmp.path().join("evil.eml").exists());
        assert!(!out.join("evil2.eml").exists());
        assert!(!imported[0].dir.join("evil2.eml").exists());
        assert!(!imported[0].dir.join("mail/INBOX/run.exe").exists()); // só .eml são extraídos
    }

    #[test]
    fn garbage_file_is_corrupt_archive() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("x.zip");
        std::fs::write(&p, b"nao sou zip").unwrap();
        assert!(matches!(import(&p, &tmp.path().join("a"), None, ids()), Err(AppError::CorruptArchive(_))));
    }
}
