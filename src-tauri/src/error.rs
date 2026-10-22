use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Falsches Passwort oder Schlüssel")]
    WrongPassword,
    #[error("Der Tresor ist gesperrt")]
    Locked,
    #[error("Der Tresor ist nur lesend geöffnet (wird an einem anderen PC bearbeitet)")]
    ReadOnly,
    #[error("{0}")]
    InvalidInput(String),
    #[error("Nicht gefunden: {0}")]
    NotFound(String),
    #[error("Daten beschädigt: {0}")]
    Corrupt(String),
    #[error("Kryptografie-Fehler: {0}")]
    Crypto(String),
    #[error("Dateifehler: {0}")]
    Io(String),
    #[error("{0}")]
    Other(String),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::InvalidData {
            Error::Corrupt(e.to_string())
        } else {
            Error::Io(e.to_string())
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Corrupt(format!("JSON: {e}"))
    }
}

/// Fehler gehen als lesbarer Text ans Frontend.
impl Serialize for Error {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
