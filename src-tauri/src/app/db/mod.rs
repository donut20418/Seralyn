use std::path::{Path, PathBuf};
use std::sync::Mutex;
use rusqlite::Connection;
use crate::app::error::{AppError, Result};

pub mod conversations;
pub mod messages;
pub mod provider_sessions;

const MIGRATION_001: &str = r#"
CREATE TABLE IF NOT EXISTS conversations (
    id TEXT PRIMARY KEY,
    title TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    archived INTEGER NOT NULL DEFAULT 0,
    metadata_json TEXT
);

CREATE TABLE IF NOT EXISTS provider_sessions (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    provider_session_id TEXT,
    model TEXT,
    created_at TEXT NOT NULL,
    last_used_at TEXT,
    status TEXT NOT NULL,
    metadata_json TEXT
);

CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    parent_id TEXT REFERENCES messages(id),
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    provider TEXT,
    model TEXT,
    provider_session_id TEXT REFERENCES provider_sessions(id),
    created_at TEXT NOT NULL,
    token_estimate INTEGER,
    metadata_json TEXT
);
"#;

pub struct Database {
    pub conn: Mutex<Connection>,
}

impl Database {
    pub fn new(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| AppError::Database(e.to_string()))?;
        
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;"
        ).map_err(|e| AppError::Database(e.to_string()))?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn new_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| AppError::Database(e.to_string()))?;
        
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn run_migrations(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(MIGRATION_001)
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn default_path() -> PathBuf {
        let mut path = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("MyAI");
        path.push("data");
        std::fs::create_dir_all(&path).ok();
        path.push("myai.db");
        path
    }
}

pub fn execute_in_transaction<F, T>(conn: &mut Connection, f: F) -> Result<T>
where
    F: FnOnce(&Connection) -> Result<T>,
{
    let tx = conn.transaction().map_err(|e| AppError::Database(e.to_string()))?;
    let res = f(&tx)?;
    tx.commit().map_err(|e| AppError::Database(e.to_string()))?;
    Ok(res)
}
