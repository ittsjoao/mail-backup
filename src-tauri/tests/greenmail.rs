//! Requer GreenMail em 127.0.0.1:3143 (ver Task 4, Step 1). Rodar com:
//! cargo test --test greenmail -- --ignored --test-threads=1
use futures::TryStreamExt;
use mail_backup_lib::error::{AppError, Result};
use mail_backup_lib::imap_sync::{self, Progress, SyncCtx, SyncReport};
use mail_backup_lib::index::{Index, Query};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::net::TcpStream;

async fn plain(user: &str) -> Result<async_imap::Session<TcpStream>> {
    let tcp = TcpStream::connect(("127.0.0.1", 3143)).await.map_err(|e| AppError::Network(e.to_string()))?;
    imap_sync::login(tcp, user, "x").await
}

fn user(tag: &str) -> String {
    format!("{tag}{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())
}

fn msg(i: usize) -> String {
    format!("From: a{i}@x.com\r\nTo: t@x.com\r\nSubject: Teste {i}\r\nMessage-ID: <m{i}@x>\r\nDate: Mon, 6 Oct 2026 10:00:00 +0000\r\n\r\nCorpo {i}\r\n")
}

// INTERNALDATE fixo: sem ele o GreenMail usa a hora do APPEND e a deduplicação por (Message-ID, data, tamanho) não casa após recriar a pasta.
async fn seed(user: &str, folder: &str, range: std::ops::Range<usize>) {
    let mut s = plain(user).await.unwrap();
    if folder != "INBOX" {
        let _ = s.create(folder).await;
    }
    for i in range {
        s.append(folder, None, Some("\"06-Oct-2026 10:00:00 +0000\""), msg(i)).await.unwrap();
    }
    s.logout().await.unwrap();
}

async fn run(user: &str, dir: &Path, cancel_after_first_batch: bool) -> (SyncReport, Index) {
    let (mut index, _) = Index::open(&dir.join("index.sqlite")).unwrap();
    let cancel = AtomicBool::new(false);
    let progress = |_: Progress| if cancel_after_first_batch { cancel.store(true, Ordering::SeqCst) };
    let mail = dir.join("mail");
    let ctx = SyncCtx { account_id: "t", mail_root: &mail, cancel: &cancel, progress: &progress };
    let report = imap_sync::sync_account(|| plain(user), &ctx, &mut index).await.unwrap();
    (report, index)
}

// GreenMail 1.6.15 quebra (NPE) no IMAP DELETE de uma pasta já selecionada por qualquer sessão anterior
// (mesmo após LOGOUT), então a pasta é recriada via API REST de reset (apaga tudo; com auth desabilitada
// o usuário é recriado no próximo login).
async fn reset_greenmail() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut c = TcpStream::connect(("127.0.0.1", 8080)).await.unwrap();
    c.write_all(b"POST /api/service/reset HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
    let mut buf = [0u8; 512]; // o servidor ignora "Connection: close"; uma leitura basta
    let n = c.read(&mut buf).await.unwrap();
    let resp = String::from_utf8_lossy(&buf[..n]);
    assert!(resp.starts_with("HTTP/1.1 200"), "reset falhou: {resp}");
}

async fn uid_validity(user: &str, folder: &str) -> Option<u32> {
    let mut s = plain(user).await.unwrap();
    let v = s.examine(folder).await.unwrap().uid_validity;
    s.logout().await.unwrap();
    v
}

fn eml_count(dir: &Path) -> usize {
    mail_backup_lib::store::walk_files(&dir.join("mail")).unwrap().iter().filter(|f| f.1.ends_with(".eml")).count()
}

fn total(index: &Index) -> i64 {
    index.list_messages(&Query { limit: 1, ..Default::default() }).unwrap().total
}

#[tokio::test]
#[ignore]
async fn full_then_incremental() {
    let u = user("inc");
    let tmp = tempfile::tempdir().unwrap();
    seed(&u, "INBOX", 0..5).await;
    seed(&u, "Clientes", 0..3).await;
    let (r, _) = run(&u, tmp.path(), false).await;
    assert_eq!(r.new_messages, 8);
    assert!(r.failed_folders.is_empty());
    assert!(tmp.path().join("mail").join("Clientes").exists());
    seed(&u, "INBOX", 5..7).await;
    assert_eq!(run(&u, tmp.path(), false).await.0.new_messages, 2);
    let (r, index) = run(&u, tmp.path(), false).await; // "N:*" devolve a última mensagem: não pode baixar de novo
    assert_eq!(r.new_messages, 0);
    assert_eq!(total(&index), 10);
    assert_eq!(eml_count(tmp.path()), 10);
}

#[tokio::test]
#[ignore]
async fn interrupted_backup_resumes_without_duplicates() {
    let u = user("cancel");
    let tmp = tempfile::tempdir().unwrap();
    seed(&u, "INBOX", 0..250).await;
    let (r, _) = run(&u, tmp.path(), true).await;
    assert!(r.cancelled);
    assert!(r.new_messages < 250);
    let (_, index) = run(&u, tmp.path(), false).await;
    assert_eq!(total(&index), 250);
    assert_eq!(eml_count(tmp.path()), 250);
}

#[tokio::test]
#[ignore]
async fn uidvalidity_change_does_not_duplicate() {
    let u = user("uv");
    let tmp = tempfile::tempdir().unwrap();
    seed(&u, "Pasta", 0..3).await;
    run(&u, tmp.path(), false).await;
    let before = uid_validity(&u, "Pasta").await;
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    reset_greenmail().await;
    seed(&u, "Pasta", 0..3).await;
    let after = uid_validity(&u, "Pasta").await;
    assert_ne!(before, after, "GreenMail não mudou o UIDVALIDITY; o teste não é válido");
    let (r, index) = run(&u, tmp.path(), false).await;
    assert_eq!(r.new_messages, 0);
    assert_eq!(total(&index), 3);
}

#[tokio::test]
#[ignore]
async fn backup_never_marks_messages_as_seen() {
    let u = user("seen");
    let tmp = tempfile::tempdir().unwrap();
    seed(&u, "INBOX", 0..3).await;
    run(&u, tmp.path(), false).await;
    let mut s = plain(&u).await.unwrap();
    s.examine("INBOX").await.unwrap();
    let fetches: Vec<_> = s.fetch("1:*", "FLAGS").await.unwrap().try_collect().await.unwrap();
    assert_eq!(fetches.len(), 3);
    for f in &fetches {
        assert!(!f.flags().any(|fl| matches!(fl, async_imap::types::Flag::Seen)));
    }
}
