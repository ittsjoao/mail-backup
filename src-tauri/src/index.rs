use crate::error::Result;
use crate::store;
use mail_parser::MessageParser;
use rusqlite::{params, params_from_iter, types::Value, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS folders (
  id INTEGER PRIMARY KEY,
  imap_name TEXT UNIQUE,
  path TEXT UNIQUE NOT NULL,
  uidvalidity INTEGER,
  last_uid INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS messages (
  id INTEGER PRIMARY KEY,
  folder_id INTEGER NOT NULL REFERENCES folders(id),
  uid INTEGER,
  file TEXT NOT NULL,
  date INTEGER,
  from_addr TEXT, from_name TEXT, subject TEXT,
  message_id TEXT,
  flags TEXT NOT NULL DEFAULT '', size INTEGER NOT NULL DEFAULT 0, has_attach INTEGER NOT NULL DEFAULT 0,
  UNIQUE(folder_id, file)
);
CREATE INDEX IF NOT EXISTS messages_folder_date ON messages(folder_id, date DESC);
CREATE INDEX IF NOT EXISTS messages_mid ON messages(folder_id, message_id);
";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FolderState {
    pub id: i64,
    pub imap_name: Option<String>,
    pub path: String,
    pub uidvalidity: Option<u32>,
    pub last_uid: u32,
}

#[derive(Debug, Serialize)]
pub struct FolderInfo {
    pub id: i64,
    pub path: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct MessageRow {
    pub id: i64,
    pub folder_id: i64,
    pub date: Option<i64>,
    pub from_addr: Option<String>,
    pub from_name: Option<String>,
    pub subject: Option<String>,
    pub flags: String,
    pub size: i64,
    pub has_attach: bool,
}

#[derive(Debug, Default, Deserialize)]
pub struct Query {
    pub folder_id: Option<i64>,
    pub text: Option<String>,
    pub date_from: Option<i64>,
    pub date_to: Option<i64>,
    pub offset: i64,
    pub limit: i64,
}

#[derive(Debug, Serialize)]
pub struct MessagePage {
    pub rows: Vec<MessageRow>,
    pub total: i64,
}

#[derive(Debug, Clone)]
pub struct MessageRecord {
    pub uid: Option<u32>,
    pub file: String,
    pub date: Option<i64>,
    pub from_addr: Option<String>,
    pub from_name: Option<String>,
    pub subject: Option<String>,
    pub message_id: Option<String>,
    pub flags: String,
    pub size: i64,
    pub has_attach: bool,
}

impl MessageRecord {
    pub fn from_raw(raw: &[u8], uid: Option<u32>, file: String, flags: String, internal_date: Option<i64>) -> Self {
        let mut rec = MessageRecord {
            uid, file, date: internal_date, from_addr: None, from_name: None, subject: None,
            message_id: None, flags, size: raw.len() as i64, has_attach: false,
        };
        if let Some(m) = MessageParser::default().parse(raw) {
            let from = m.from().and_then(|a| a.first());
            rec.date = internal_date.or_else(|| m.date().map(|d| d.to_timestamp()));
            rec.from_addr = from.and_then(|a| a.address()).map(str::to_string);
            rec.from_name = from.and_then(|a| a.name()).map(str::to_string);
            rec.subject = m.subject().map(str::to_string);
            rec.message_id = m.message_id().map(str::to_string);
            rec.has_attach = m.attachment_count() > 0;
        }
        rec
    }
}

pub struct Index {
    conn: Connection,
}

fn folder_from_row(r: &rusqlite::Row) -> rusqlite::Result<FolderState> {
    Ok(FolderState { id: r.get(0)?, imap_name: r.get(1)?, path: r.get(2)?, uidvalidity: r.get(3)?, last_uid: r.get(4)? })
}

const FOLDER_COLS: &str = "id, imap_name, path, uidvalidity, last_uid";

impl Index {
    pub fn open(path: &Path) -> Result<(Index, bool)> {
        let existed = path.exists();
        let mut conn = Connection::open(path)?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        // user_version e o marcador de "indice completo": so reindex() o grava
        let needs_reindex = version != SCHEMA_VERSION;
        if existed && version != 0 && version != SCHEMA_VERSION {
            drop(conn);
            for suffix in ["", "-wal", "-shm"] {
                let mut p = path.as_os_str().to_owned();
                p.push(suffix);
                let _ = std::fs::remove_file(PathBuf::from(p));
            }
            conn = Connection::open(path)?;
        }
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |_| Ok(()))?;
        conn.execute_batch(SCHEMA)?;
        Ok((Index { conn }, needs_reindex))
    }

    fn taken_paths(&self) -> Result<HashSet<String>> {
        let mut st = self.conn.prepare("SELECT path FROM folders")?;
        let paths = st.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(paths.into_iter().map(|p| p.to_lowercase()).collect())
    }

    fn folder_where(&self, clause: &str, arg: &str) -> Result<Option<FolderState>> {
        Ok(self.conn
            .query_row(&format!("SELECT {FOLDER_COLS} FROM folders WHERE {clause}"), [arg], folder_from_row)
            .optional()?)
    }

    pub fn folder_for_remote(&self, imap_name: &str, candidate_path: &str) -> Result<FolderState> {
        if let Some(f) = self.folder_where("imap_name = ?1", imap_name)? {
            return Ok(f);
        }
        if let Some(mut f) = self.folder_where("path = ?1 AND imap_name IS NULL", candidate_path)? {
            self.conn.execute("UPDATE folders SET imap_name = ?1 WHERE id = ?2", params![imap_name, f.id])?;
            f.imap_name = Some(imap_name.to_string());
            return Ok(f);
        }
        let path = store::unique_path(candidate_path, &self.taken_paths()?);
        self.conn.execute("INSERT INTO folders (imap_name, path) VALUES (?1, ?2)", params![imap_name, path])?;
        Ok(FolderState { id: self.conn.last_insert_rowid(), imap_name: Some(imap_name.to_string()), path, uidvalidity: None, last_uid: 0 })
    }

    fn folder_id_by_path(conn: &Connection, path: &str) -> Result<i64> {
        let found = conn.query_row("SELECT id FROM folders WHERE path = ?1", [path], |r| r.get(0)).optional()?;
        if let Some(id) = found {
            return Ok(id);
        }
        conn.execute("INSERT INTO folders (path) VALUES (?1)", [path])?;
        Ok(conn.last_insert_rowid())
    }

    fn insert_records(conn: &Connection, folder_id: i64, recs: &[MessageRecord]) -> Result<()> {
        let mut st = conn.prepare_cached(
            "INSERT OR REPLACE INTO messages (folder_id, uid, file, date, from_addr, from_name, subject, message_id, flags, size, has_attach)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )?;
        for r in recs {
            st.execute(params![folder_id, r.uid, r.file, r.date, r.from_addr, r.from_name, r.subject, r.message_id, r.flags, r.size, r.has_attach])?;
        }
        Ok(())
    }

    pub fn reset_folder(&self, folder_id: i64, uidvalidity: u32) -> Result<()> {
        self.conn.execute("UPDATE folders SET uidvalidity = ?1, last_uid = 0 WHERE id = ?2", params![uidvalidity, folder_id])?;
        Ok(())
    }

    pub fn has_duplicate(&self, folder_id: i64, rec: &MessageRecord) -> Result<bool> {
        let Some(mid) = &rec.message_id else { return Ok(false) };
        Ok(self.conn
            .query_row(
                "SELECT 1 FROM messages WHERE folder_id = ?1 AND message_id = ?2 AND size = ?3 LIMIT 1",
                params![folder_id, mid, rec.size],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    pub fn commit_batch(&mut self, folder_id: i64, recs: &[MessageRecord], last_uid: Option<u32>) -> Result<()> {
        let tx = self.conn.transaction()?;
        Self::insert_records(&tx, folder_id, recs)?;
        if let Some(uid) = last_uid {
            tx.execute("UPDATE folders SET last_uid = ?1 WHERE id = ?2", params![uid, folder_id])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn folders_state(&self) -> Result<Vec<FolderState>> {
        let mut st = self.conn.prepare(&format!("SELECT {FOLDER_COLS} FROM folders ORDER BY path"))?;
        let rows = st.query_map([], folder_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn restore_folders(&self, folders: &[FolderState]) -> Result<()> {
        for f in folders {
            self.conn.execute(
                "INSERT OR IGNORE INTO folders (imap_name, path, uidvalidity, last_uid) VALUES (?1, ?2, ?3, ?4)",
                params![f.imap_name, f.path, f.uidvalidity, f.last_uid],
            )?;
        }
        Ok(())
    }

    pub fn list_folders(&self) -> Result<Vec<FolderInfo>> {
        let mut st = self.conn.prepare(
            "SELECT f.id, f.path, COUNT(m.id) FROM folders f LEFT JOIN messages m ON m.folder_id = f.id
             GROUP BY f.id ORDER BY f.path COLLATE NOCASE",
        )?;
        let rows = st
            .query_map([], |r| Ok(FolderInfo { id: r.get(0)?, path: r.get(1)?, count: r.get(2)? }))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn list_messages(&self, q: &Query) -> Result<MessagePage> {
        let mut sql = String::from(" FROM messages WHERE 1 = 1");
        let mut args: Vec<Value> = Vec::new();
        if let Some(f) = q.folder_id {
            sql.push_str(" AND folder_id = ?");
            args.push(f.into());
        }
        if let Some(t) = q.text.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            let escaped = t.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
            let pattern = format!("%{escaped}%");
            sql.push_str(" AND (subject LIKE ? ESCAPE '\\' OR from_name LIKE ? ESCAPE '\\' OR from_addr LIKE ? ESCAPE '\\')");
            for _ in 0..3 {
                args.push(pattern.clone().into());
            }
        }
        if let Some(d) = q.date_from {
            sql.push_str(" AND date >= ?");
            args.push(d.into());
        }
        if let Some(d) = q.date_to {
            sql.push_str(" AND date < ?");
            args.push(d.into());
        }
        let total: i64 = self.conn.query_row(&format!("SELECT COUNT(*){sql}"), params_from_iter(args.iter()), |r| r.get(0))?;
        args.push(q.limit.clamp(1, 1000).into());
        args.push(q.offset.max(0).into());
        let mut st = self.conn.prepare(&format!(
            "SELECT id, folder_id, date, from_addr, from_name, subject, flags, size, has_attach{sql} ORDER BY date DESC, id DESC LIMIT ? OFFSET ?"
        ))?;
        let rows = st
            .query_map(params_from_iter(args.iter()), |r| {
                Ok(MessageRow {
                    id: r.get(0)?, folder_id: r.get(1)?, date: r.get(2)?, from_addr: r.get(3)?, from_name: r.get(4)?,
                    subject: r.get(5)?, flags: r.get(6)?, size: r.get(7)?, has_attach: r.get(8)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(MessagePage { rows, total })
    }

    pub fn message_file(&self, message_id: i64) -> Result<String> {
        Ok(self.conn.query_row("SELECT file FROM messages WHERE id = ?1", [message_id], |r| r.get(0))?)
    }

    // ponytail: flags não são recuperáveis do .eml; reindexação as deixa vazias
    pub fn reindex(&mut self, mail_root: &Path, progress: &mut dyn FnMut(u64)) -> Result<u64> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM messages", [])?;
        let mut batch: Vec<MessageRecord> = Vec::new();
        let mut current: Option<(String, i64)> = None;
        let mut n = 0u64;
        for (abs, rel) in store::walk_files(mail_root)? {
            if !rel.ends_with(".eml") {
                continue;
            }
            let Some((folder, name)) = rel.rsplit_once('/') else { continue };
            let Ok(raw) = std::fs::read(&abs) else { continue };
            let folder_id = match &current {
                Some((p, id)) if p == folder => *id,
                _ => {
                    if let Some((_, id)) = current.take() {
                        Self::insert_records(&tx, id, &batch)?;
                        batch.clear();
                    }
                    let id = Self::folder_id_by_path(&tx, folder)?;
                    current = Some((folder.to_string(), id));
                    id
                }
            };
            batch.push(MessageRecord::from_raw(&raw, store::uid_from_name(name), rel.clone(), String::new(), None));
            n += 1;
            if batch.len() >= 500 {
                Self::insert_records(&tx, folder_id, &batch)?;
                batch.clear();
                progress(n);
            }
        }
        if let Some((_, id)) = current {
            Self::insert_records(&tx, id, &batch)?;
        }
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        tx.commit()?;
        progress(n);
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(subject: &str, date: &str, mid: &str) -> Vec<u8> {
        format!("From: Joao <joao@x.com>\r\nSubject: {subject}\r\nDate: {date}\r\nMessage-ID: <{mid}>\r\n\r\ncorpo\r\n").into_bytes()
    }

    fn rec(file: &str, subject: &str, date: &str, mid: &str) -> MessageRecord {
        MessageRecord::from_raw(&raw(subject, date, mid), store::uid_from_name(file.rsplit('/').next().unwrap()), file.to_string(), String::new(), None)
    }

    fn q(folder_id: Option<i64>, text: Option<&str>) -> Query {
        Query { folder_id, text: text.map(str::to_string), offset: 0, limit: 100, ..Default::default() }
    }

    #[test]
    fn open_reports_reindex_until_a_reindex_completes() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("index.sqlite");
        let empty = tmp.path().join("mail");
        assert!(Index::open(&p).unwrap().1);
        assert!(Index::open(&p).unwrap().1);
        {
            let (mut idx, _) = Index::open(&p).unwrap();
            idx.reindex(&empty, &mut |_| {}).unwrap();
        }
        assert!(!Index::open(&p).unwrap().1);
        {
            let (idx, _) = Index::open(&p).unwrap();
            idx.folder_for_remote("INBOX", "INBOX").unwrap();
            idx.conn.pragma_update(None, "user_version", 99).unwrap();
        }
        let (idx, needs) = Index::open(&p).unwrap();
        assert!(needs);
        assert!(idx.folders_state().unwrap().is_empty());
        idx.folder_for_remote("INBOX", "INBOX").unwrap();
        idx.conn.pragma_update(None, "user_version", 0).unwrap();
        drop(idx);
        let (idx, needs) = Index::open(&p).unwrap();
        assert!(needs);
        assert_eq!(idx.folders_state().unwrap().len(), 1);
    }

    #[test]
    fn record_parses_headers() {
        let r = rec("INBOX/7-00003.eml", "Olá", "Mon, 6 Oct 2026 10:00:00 +0000", "a@b");
        assert_eq!(r.uid, Some(3));
        assert_eq!(r.subject.as_deref(), Some("Olá"));
        assert_eq!(r.from_addr.as_deref(), Some("joao@x.com"));
        assert_eq!(r.from_name.as_deref(), Some("Joao"));
        assert_eq!(r.message_id.as_deref(), Some("a@b"));
        assert_eq!(r.date, Some(1791280800));
    }

    #[test]
    fn record_from_garbage_does_not_panic() {
        let r = MessageRecord::from_raw(b"\x00\xffsem cabecalhos", None, "X/1.eml".into(), String::new(), None);
        assert_eq!(r.size, 16);
        assert!(r.subject.is_none() && r.from_addr.is_none() && r.date.is_none());
    }

    #[test]
    fn folder_for_remote_creates_adopts_and_avoids_case_collision() {
        let tmp = tempfile::tempdir().unwrap();
        let (idx, _) = Index::open(&tmp.path().join("i.sqlite")).unwrap();
        let a = idx.folder_for_remote("Clientes", "Clientes").unwrap();
        assert_eq!(a.path, "Clientes");
        assert_eq!(idx.folder_for_remote("Clientes", "Clientes").unwrap().id, a.id);
        let b = idx.folder_for_remote("clientes", "clientes").unwrap();
        assert_eq!(b.path, "clientes~2");
        // pasta vinda de reindexação (imap_name NULL) é adotada
        idx.restore_folders(&[FolderState { id: 0, imap_name: None, path: "INBOX".into(), uidvalidity: None, last_uid: 0 }]).unwrap();
        let inbox = idx.folder_for_remote("INBOX", "INBOX").unwrap();
        assert_eq!(inbox.path, "INBOX");
        assert_eq!(inbox.imap_name.as_deref(), Some("INBOX"));
    }

    #[test]
    fn commit_batch_updates_last_uid_and_lists_by_date() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut idx, _) = Index::open(&tmp.path().join("i.sqlite")).unwrap();
        let f = idx.folder_for_remote("INBOX", "INBOX").unwrap();
        idx.reset_folder(f.id, 7).unwrap();
        idx.commit_batch(f.id, &[
            rec("INBOX/7-00001.eml", "velha", "Mon, 1 Jan 2024 10:00:00 +0000", "1@x"),
            rec("INBOX/7-00002.eml", "nova", "Mon, 6 Oct 2026 10:00:00 +0000", "2@x"),
        ], Some(2)).unwrap();
        let st = idx.folders_state().unwrap();
        assert_eq!((st[0].uidvalidity, st[0].last_uid), (Some(7), 2));
        let page = idx.list_messages(&q(Some(f.id), None)).unwrap();
        assert_eq!(page.total, 2);
        assert_eq!(page.rows[0].subject.as_deref(), Some("nova"));
        assert_eq!(idx.message_file(page.rows[0].id).unwrap(), "INBOX/7-00002.eml");
        assert_eq!(idx.list_folders().unwrap()[0].count, 2);
    }

    #[test]
    fn list_orders_null_dates_last() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut idx, _) = Index::open(&tmp.path().join("i.sqlite")).unwrap();
        let f = idx.folder_for_remote("INBOX", "INBOX").unwrap();
        let garbage = MessageRecord::from_raw(b"lixo", Some(9), "INBOX/1-00009.eml".into(), String::new(), None);
        idx.commit_batch(f.id, &[garbage, rec("INBOX/1-00001.eml", "ok", "Mon, 6 Oct 2026 10:00:00 +0000", "1@x")], None).unwrap();
        let rows = idx.list_messages(&q(None, None)).unwrap().rows;
        assert_eq!(rows[0].subject.as_deref(), Some("ok"));
        assert!(rows[1].date.is_none());
    }

    #[test]
    fn search_matches_subject_and_sender_and_treats_wildcards_literally() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut idx, _) = Index::open(&tmp.path().join("i.sqlite")).unwrap();
        let f = idx.folder_for_remote("INBOX", "INBOX").unwrap();
        idx.commit_batch(f.id, &[
            rec("INBOX/1-00001.eml", "50% off", "Mon, 6 Oct 2026 10:00:00 +0000", "1@x"),
            rec("INBOX/1-00002.eml", "500 coisas", "Mon, 6 Oct 2026 11:00:00 +0000", "2@x"),
            rec("INBOX/1-00003.eml", "a_b", "Mon, 6 Oct 2026 12:00:00 +0000", "3@x"),
            rec("INBOX/1-00004.eml", "axb", "Mon, 6 Oct 2026 13:00:00 +0000", "4@x"),
        ], None).unwrap();
        assert_eq!(idx.list_messages(&q(None, Some("50%"))).unwrap().total, 1);
        assert_eq!(idx.list_messages(&q(None, Some("a_b"))).unwrap().total, 1);
        assert_eq!(idx.list_messages(&q(None, Some("JOAO@x"))).unwrap().total, 4);
        assert_eq!(idx.list_messages(&q(None, Some("  "))).unwrap().total, 4);
    }

    #[test]
    fn has_duplicate_requires_message_id() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut idx, _) = Index::open(&tmp.path().join("i.sqlite")).unwrap();
        let f = idx.folder_for_remote("INBOX", "INBOX").unwrap();
        let r1 = rec("INBOX/1-00001.eml", "s", "Mon, 6 Oct 2026 10:00:00 +0000", "1@x");
        idx.commit_batch(f.id, &[r1.clone()], None).unwrap();
        let again = rec("INBOX/2-00001.eml", "s", "Mon, 6 Oct 2026 10:00:00 +0000", "1@x");
        assert!(idx.has_duplicate(f.id, &again).unwrap());
        let other_date = MessageRecord { date: Some(1), ..again.clone() };
        assert!(idx.has_duplicate(f.id, &other_date).unwrap());
        let no_id = MessageRecord { message_id: None, ..again };
        assert!(!idx.has_duplicate(f.id, &no_id).unwrap());
    }

    #[test]
    fn reindex_rebuilds_from_files_keeping_folder_state() {
        let tmp = tempfile::tempdir().unwrap();
        let mail = tmp.path().join("mail");
        store::write_atomic(&mail.join("INBOX").join("5-00001.eml"), &raw("um", "Mon, 6 Oct 2026 10:00:00 +0000", "1@x")).unwrap();
        store::write_atomic(&mail.join("INBOX").join("Sub").join("5-00002.eml"), &raw("dois", "Mon, 6 Oct 2026 10:00:00 +0000", "2@x")).unwrap();
        store::write_atomic(&mail.join("INBOX").join("ignorar.txt"), b"x").unwrap();
        let (mut idx, needs) = Index::open(&tmp.path().join("index.sqlite")).unwrap();
        assert!(needs);
        idx.restore_folders(&[FolderState { id: 0, imap_name: Some("INBOX".into()), path: "INBOX".into(), uidvalidity: Some(5), last_uid: 1 }]).unwrap();
        let mut seen = 0;
        assert_eq!(idx.reindex(&mail, &mut |n| seen = n).unwrap(), 2);
        assert_eq!(seen, 2);
        let folders = idx.list_folders().unwrap();
        assert_eq!(folders.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), vec!["INBOX", "INBOX/Sub"]);
        let st = idx.folders_state().unwrap();
        assert_eq!(st.iter().find(|f| f.path == "INBOX").unwrap().last_uid, 1);
        assert_eq!(idx.reindex(&mail, &mut |_| {}).unwrap(), 2);
        assert_eq!(idx.list_messages(&q(None, None)).unwrap().total, 2);
    }

    #[test]
    fn reader_not_blocked_by_open_write_transaction() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("i.sqlite");
        let (mut writer, _) = Index::open(&p).unwrap();
        let f = writer.folder_for_remote("INBOX", "INBOX").unwrap();
        let (reader, _) = Index::open(&p).unwrap();
        let tx = writer.conn.transaction().unwrap();
        tx.execute("INSERT INTO messages (folder_id, file, flags, size, has_attach) VALUES (?1, 'INBOX/x.eml', '', 0, 0)", [f.id]).unwrap();
        assert_eq!(reader.list_messages(&q(None, None)).unwrap().total, 0);
        tx.commit().unwrap();
        assert_eq!(reader.list_messages(&q(None, None)).unwrap().total, 1);
    }
}
