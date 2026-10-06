use crate::error::{AppError, Result};
use crate::store;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const KEYRING_SERVICE: &str = "mail-backup";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LastRun {
    pub at: i64,
    pub new_messages: u64,
    pub failed_folders: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub email: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub dir: PathBuf,
    pub has_credentials: bool,
    pub last_run: Option<LastRun>,
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Config {
    pub accounts: Vec<Account>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Config> {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| AppError::Other(format!("config.json inválido: {e}"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_vec_pretty(self).map_err(|e| AppError::Other(e.to_string()))?;
        store::write_atomic(path, &json)
    }

    pub fn get(&self, id: &str) -> Result<&Account> {
        self.accounts.iter().find(|a| a.id == id).ok_or_else(|| AppError::Other(format!("conta não encontrada: {id}")))
    }

    pub fn get_mut(&mut self, id: &str) -> Result<&mut Account> {
        self.accounts.iter_mut().find(|a| a.id == id).ok_or_else(|| AppError::Other(format!("conta não encontrada: {id}")))
    }
}

fn entry(id: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, id).map_err(|e| AppError::Keyring(e.to_string()))
}

pub fn set_password(id: &str, password: &str) -> Result<()> {
    entry(id)?.set_password(password).map_err(|e| AppError::Keyring(e.to_string()))
}

pub fn get_password(id: &str) -> Result<String> {
    entry(id)?.get_password().map_err(|e| AppError::Keyring(e.to_string()))
}

pub fn delete_password(id: &str) -> Result<()> {
    match entry(id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::Keyring(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: &str) -> Account {
        Account {
            id: id.into(), name: "Joao".into(), email: "joao@x.com".into(), host: "imap.uhserver.com".into(),
            port: 993, username: "joao@x.com".into(), dir: PathBuf::from("C:/tmp/x"), has_credentials: true, last_run: None,
        }
    }

    #[test]
    fn load_missing_returns_empty_and_roundtrips() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("config.json");
        assert_eq!(Config::load(&p).unwrap(), Config::default());
        let mut c = Config::default();
        c.accounts.push(account("a1"));
        c.save(&p).unwrap();
        let loaded = Config::load(&p).unwrap();
        assert_eq!(loaded, c);
        assert!(loaded.get("a1").is_ok());
        assert!(loaded.get("nope").is_err());
    }

    #[test]
    fn invalid_json_is_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("config.json");
        std::fs::write(&p, b"{nao json").unwrap();
        assert!(matches!(Config::load(&p), Err(AppError::Other(_))));
    }

    #[test]
    fn keyring_roundtrip() {
        let id = format!("teste-{}", uuid::Uuid::new_v4());
        set_password(&id, "s3nha").unwrap();
        assert_eq!(get_password(&id).unwrap(), "s3nha");
        delete_password(&id).unwrap();
        assert!(get_password(&id).is_err());
        delete_password(&id).unwrap(); // apagar o que não existe não é erro
    }
}
