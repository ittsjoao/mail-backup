use serde::{ser::SerializeStruct, Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("usuário ou senha inválidos")]
    AuthFailed,
    #[error("falha de rede: {0}")]
    Network(String),
    #[error("falha de TLS: {0}")]
    Tls(String),
    #[error("erro de disco: {0}")]
    Io(#[from] std::io::Error),
    #[error("senha do arquivo incorreta")]
    WrongPassword,
    #[error("arquivo de backup corrompido: {0}")]
    CorruptArchive(String),
    #[error("caminho inseguro rejeitado: {0}")]
    UnsafePath(String),
    #[error("erro no índice: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("erro nas credenciais: {0}")]
    Keyring(String),
    #[error("{0}")]
    Other(String),
}

impl AppError {
    pub fn kind(&self) -> &'static str {
        match self {
            AppError::AuthFailed => "AuthFailed",
            AppError::Network(_) => "Network",
            AppError::Tls(_) => "Tls",
            AppError::Io(_) => "Io",
            AppError::WrongPassword => "WrongPassword",
            AppError::CorruptArchive(_) => "CorruptArchive",
            AppError::UnsafePath(_) => "UnsafePath",
            AppError::Db(_) => "Db",
            AppError::Keyring(_) => "Keyring",
            AppError::Other(_) => "Other",
        }
    }

    pub fn is_retryable(&self) -> bool {
        matches!(self, AppError::Network(_) | AppError::Tls(_))
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let mut st = s.serialize_struct("AppError", 2)?;
        st.serialize_field("kind", self.kind())?;
        st.serialize_field("message", &self.to_string())?;
        st.end()
    }
}

pub type Result<T> = std::result::Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_kind_and_message() {
        let json = serde_json::to_string(&AppError::AuthFailed).unwrap();
        assert_eq!(json, r#"{"kind":"AuthFailed","message":"usuário ou senha inválidos"}"#);
        assert!(AppError::Network("x".into()).is_retryable());
        assert!(!AppError::WrongPassword.is_retryable());
    }
}
