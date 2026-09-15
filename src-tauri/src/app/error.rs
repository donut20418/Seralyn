use serde::{Deserialize, Serialize};

#[derive(thiserror::Error, Debug, Clone, Serialize, Deserialize)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(String),
    #[error("Provider error: {0}")]
    Provider(String),
    #[error("Process error: {0}")]
    Process(String),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("IO error: {0}")]
    Io(String),
    #[error("Config error: {0}")]
    Config(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("Timeout error: {0}")]
    Timeout(String),
    #[error("Provider not installed: {0}")]
    ProviderNotInstalled(String),
    #[error("Provider not authenticated: {0}")]
    ProviderNotAuthenticated(String),
    #[error("Session not found: {0}")]
    SessionNotFound(String),
    #[error("Conversation not found: {0}")]
    ConversationNotFound(String),
}

impl From<rusqlite::Error> for AppError {
    fn from(err: rusqlite::Error) -> Self {
        AppError::Database(err.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        AppError::Serialization(err.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::Io(err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
