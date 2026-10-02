//! Structured application errors. Messages are user-facing and must never
//! contain secrets (passphrases are never formatted into errors).

use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("I/O error at {path}: {source}")]
    Io { path: String, #[source] source: std::io::Error },
    #[error("Path rejected by safety rules: {0}")]
    UnsafePath(String),
    #[error("Manifest is invalid: {0}")]
    InvalidManifest(String),
    #[error("Unsupported manifest schema version {found} (this app supports {supported}.x)")]
    UnsupportedSchema { found: String, supported: u32 },
    #[error("Integrity check failed: {0}")]
    Integrity(String),
    #[error("Encryption error: {0}")]
    Crypto(String),
    #[error("Insufficient disk space: need about {needed} bytes, {available} available")]
    InsufficientSpace { needed: u64, available: u64 },
    #[error("Operation canceled")]
    Canceled,
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Confirmation required: {0}")]
    ConfirmationRequired(String),
    #[error("Invalid request: {0}")]
    InvalidRequest(String),
    #[error("Platform adapter unavailable: {0}")]
    AdapterUnavailable(String),
    #[error("Database error: {0}")]
    Database(String),
    #[error("Serialization error: {0}")]
    Serialization(String),
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    pub fn io(path: impl AsRef<std::path::Path>, source: std::io::Error) -> Self {
        AppError::Io { path: path.as_ref().display().to_string(), source }
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Serialization(e.to_string())
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        AppError::Database(e.to_string())
    }
}

/// Errors cross the Tauri IPC boundary as plain strings.
impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// Helper to attach a path to std::io results.
pub trait IoContext<T> {
    fn at(self, path: impl AsRef<std::path::Path>) -> AppResult<T>;
}

impl<T> IoContext<T> for std::io::Result<T> {
    fn at(self, path: impl AsRef<std::path::Path>) -> AppResult<T> {
        self.map_err(|e| AppError::io(path, e))
    }
}
