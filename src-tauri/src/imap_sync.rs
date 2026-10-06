use crate::error::{AppError, Result};
use crate::index::{Index, MessageRecord};
use crate::store;
use async_imap::types::{Fetch, NameAttribute};
use async_imap::{Client, Session};
use futures::TryStreamExt;
use serde::Serialize;
use std::future::Future;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_rustls::rustls;

pub type TlsSession = Session<tokio_rustls::client::TlsStream<TcpStream>>;

const BATCH: u32 = 100;
const BACKOFF_SECS: [u64; 3] = [1, 4, 16];
const FETCH_QUERY: &str = "(UID FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[])";
const READ_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub account_id: String,
    pub folder: String,
    pub done: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncReport {
    pub new_messages: u64,
    pub failed_folders: Vec<String>,
    pub cancelled: bool,
}

pub struct SyncCtx<'a> {
    pub account_id: &'a str,
    pub mail_root: &'a Path,
    pub cancel: &'a AtomicBool,
    pub progress: &'a (dyn Fn(Progress) + Send + Sync),
}

struct RemoteFolder {
    name: String,
    delimiter: Option<String>,
}

fn net(e: impl std::fmt::Display) -> AppError {
    AppError::Network(e.to_string())
}

/// Limita a espera por qualquer resposta do servidor; sem isso um servidor mudo trava o backup e o cancelamento.
pub async fn timed<F: Future>(f: F) -> Result<F::Output> {
    tokio::time::timeout(READ_TIMEOUT, f).await.map_err(|_| net("tempo esgotado aguardando o servidor"))
}

pub async fn login<T>(stream: T, user: &str, pass: &str) -> Result<Session<T>>
where
    T: AsyncRead + AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    let mut client = Client::new(stream);
    timed(client.read_response())
        .await?
        .map_err(net)?
        .ok_or_else(|| net("o servidor fechou a conexão antes da saudação"))?;
    timed(client.login(user, pass)).await?.map_err(|(e, _)| match e {
        async_imap::error::Error::No(_) | async_imap::error::Error::Bad(_) => AppError::AuthFailed,
        other => net(other),
    })
}

pub async fn connect_tls(host: &str, port: u16, user: &str, pass: &str) -> Result<TlsSession> {
    let tls = |e: rustls::Error| AppError::Tls(e.to_string());
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(tls)?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let tcp = tokio::time::timeout(Duration::from_secs(20), TcpStream::connect((host, port)))
        .await
        .map_err(|_| net("tempo esgotado ao conectar"))?
        .map_err(net)?;
    let name = rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|e| AppError::Tls(e.to_string()))?;
    let stream = timed(tokio_rustls::TlsConnector::from(Arc::new(config)).connect(name, tcp))
        .await?
        .map_err(|e| AppError::Tls(e.to_string()))?;
    login(stream, user, pass).await
}

async fn list_folders<T>(s: &mut Session<T>) -> Result<Vec<RemoteFolder>>
where
    T: AsyncRead + AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    let names: Vec<async_imap::types::Name> =
        timed(async { s.list(Some(""), Some("*")).await?.try_collect().await }).await?.map_err(net)?;
    // conexão morta pode devolver LIST vazio em vez de erro; um servidor IMAP sempre tem INBOX
    if names.is_empty() {
        return Err(net("lista de pastas vazia (conexão encerrada?)"));
    }
    Ok(names
        .iter()
        .filter(|n| !n.attributes().iter().any(|a| matches!(a, NameAttribute::NoSelect)))
        .map(|n| RemoteFolder { name: n.name().to_string(), delimiter: n.delimiter().map(str::to_string) })
        .collect())
}

fn flags_string(f: &Fetch) -> String {
    f.flags().map(|fl| format!("{fl:?}")).collect::<Vec<_>>().join(" ")
}

async fn sync_folder<T>(s: &mut Session<T>, remote: &RemoteFolder, ctx: &SyncCtx<'_>, index: &mut Index) -> Result<u64>
where
    T: AsyncRead + AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    let candidate = store::folder_rel_path(&remote.name, remote.delimiter.as_deref());
    let mut folder = index.folder_for_remote(&remote.name, &candidate)?;
    if !store::is_safe_rel(&folder.path) {
        return Err(AppError::UnsafePath(folder.path));
    }
    let mailbox = timed(s.examine(&remote.name)).await?.map_err(net)?;
    let uidvalidity = mailbox.uid_validity.unwrap_or(0);
    if folder.uidvalidity != Some(uidvalidity) {
        index.reset_folder(folder.id, uidvalidity)?;
        folder.last_uid = 0;
    }
    // derivado do estado persistido: sobrevive a retry, cancelamento e queda no meio da pasta
    let dedupe = index.has_messages_outside(folder.id, &format!("{}/{}-", folder.path, uidvalidity))?;
    if mailbox.exists == 0 || mailbox.uid_next.is_some_and(|next| next <= folder.last_uid + 1) {
        return Ok(0);
    }
    let total = mailbox.uid_next.map_or(mailbox.exists, |n| n - folder.last_uid - 1).min(mailbox.exists);
    let report = |done: u32| {
        (ctx.progress)(Progress { account_id: ctx.account_id.to_string(), folder: folder.path.clone(), done, total })
    };

    let mut stream = timed(s.uid_fetch(format!("{}:*", folder.last_uid + 1), FETCH_QUERY)).await?.map_err(net)?;
    let mut batch = Vec::with_capacity(BATCH as usize);
    let (mut last_uid, mut done, mut saved) = (folder.last_uid, 0u32, 0u64);
    loop {
        let next = timed(stream.try_next()).await?.map_err(net)?;
        let Some(fetch) = next else { break };
        let Some(uid) = fetch.uid else { continue };
        if uid <= folder.last_uid {
            continue; // "N:*" devolve a última mensagem mesmo sem novidades
        }
        last_uid = last_uid.max(uid);
        done += 1;
        // ponytail: resposta sem corpo (mensagem removida durante o fetch) é ignorada
        if let Some(raw) = fetch.body() {
            let file = format!("{}/{}", folder.path, store::eml_name(uidvalidity, uid));
            let internal = fetch.internal_date().map(|d| d.timestamp());
            let rec = MessageRecord::from_raw(raw, Some(uid), file, flags_string(&fetch), internal);
            if !(dedupe && index.has_duplicate(folder.id, &rec)?) {
                store::write_atomic(&store::join_rel(ctx.mail_root, &rec.file), raw)?;
                batch.push(rec);
                saved += 1;
            }
        }
        if done % BATCH == 0 {
            index.commit_batch(folder.id, &batch, Some(last_uid))?;
            batch.clear();
            report(done);
            if ctx.cancel.load(Ordering::SeqCst) {
                return Ok(saved);
            }
        }
    }
    index.commit_batch(folder.id, &batch, Some(last_uid))?;
    report(done);
    Ok(saved)
}

pub async fn sync_account<T, C, F>(mut connect: C, ctx: &SyncCtx<'_>, index: &mut Index) -> Result<SyncReport>
where
    T: AsyncRead + AsyncWrite + Unpin + std::fmt::Debug + Send,
    C: FnMut() -> F,
    F: Future<Output = Result<Session<T>>>,
{
    store::remove_tmp_files(ctx.mail_root)?;
    let mut report = SyncReport::default();
    let mut session = connect().await?;
    let mut attempt = 0;
    let folders = loop {
        match list_folders(&mut session).await {
            Ok(f) => break f,
            Err(e) if e.is_retryable() && attempt < BACKOFF_SECS.len() => session = reconnect(&mut connect, &mut attempt).await?,
            Err(e) => return Err(e),
        }
    };
    let mut stale = false; // sessão suspeita após esgotar as tentativas de uma pasta
    for remote in &folders {
        if ctx.cancel.load(Ordering::SeqCst) {
            report.cancelled = true;
            return Ok(report); // sessão descartada sem LOGOUT: pode haver respostas pendentes
        }
        if std::mem::take(&mut stale) {
            session = connect().await?;
        }
        let mut attempt = 0;
        loop {
            match sync_folder(&mut session, remote, ctx, index).await {
                Ok(n) => {
                    report.new_messages += n;
                    break;
                }
                Err(e) if e.is_retryable() && attempt < BACKOFF_SECS.len() => {
                    session = reconnect(&mut connect, &mut attempt).await?;
                }
                Err(e) if e.is_retryable() => {
                    report.failed_folders.push(store::folder_rel_path(&remote.name, remote.delimiter.as_deref()));
                    stale = true;
                    break;
                }
                Err(e) => return Err(e),
            }
        }
    }
    if ctx.cancel.load(Ordering::SeqCst) {
        report.cancelled = true;
        return Ok(report);
    }
    let _ = timed(session.logout()).await;
    Ok(report)
}

/// Espera o backoff e reconecta; se todas as tentativas restantes falharem, devolve o erro (a conta inteira falha).
async fn reconnect<T, C, F>(connect: &mut C, attempt: &mut usize) -> Result<Session<T>>
where
    T: AsyncRead + AsyncWrite + Unpin + std::fmt::Debug + Send,
    C: FnMut() -> F,
    F: Future<Output = Result<Session<T>>>,
{
    loop {
        tokio::time::sleep(Duration::from_secs(BACKOFF_SECS[*attempt])).await;
        *attempt += 1;
        match connect().await {
            Ok(s) => return Ok(s),
            Err(e) if e.is_retryable() && *attempt < BACKOFF_SECS.len() => continue,
            Err(e) => return Err(e),
        }
    }
}
