use std::path::{Path, PathBuf};
use std::sync::Mutex;
use rusqlite::Connection;
use crate::app::error::{AppError, Result};

pub mod conversations;
pub mod messages;
pub mod provider_sessions;

const MIGRATION_001: &str = include_str!("../../../migrations/001_initial.sql");
const MIGRATION_002: &str = include_str!("../../../migrations/002_sync_cursor.sql");

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

        // 1. Ensure schema_migrations table exists
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))
            );"
        ).map_err(|e| AppError::Database(e.to_string()))?;

        // 2. Check if legacy database without schema_migrations already has baseline tables
        let has_conversations: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='conversations')",
            [],
            |row| row.get(0),
        ).unwrap_or(false);

        let has_v1: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = 1)",
            [],
            |row| row.get(0),
        ).unwrap_or(false);

        if has_conversations && !has_v1 {
            // Existing DB already has baseline tables; record version 1 as applied
            let _ = conn.execute(
                "INSERT OR IGNORE INTO schema_migrations (version) VALUES (1)",
                [],
            );
        }

        // 3. Check if DB from Phase 1.2 already has sync cursor columns
        let has_seq_column: bool = {
            let mut stmt = conn.prepare("PRAGMA table_info(messages)").ok();
            if let Some(ref mut stmt) = stmt {
                let rows = stmt.query_map([], |row| {
                    let col_name: String = row.get(1)?;
                    Ok(col_name)
                }).ok();
                rows.map(|mut r| r.any(|c| c.as_deref() == Ok("seq"))).unwrap_or(false)
            } else {
                false
            }
        };

        if has_seq_column {
            let _ = conn.execute(
                "INSERT OR IGNORE INTO schema_migrations (version) VALUES (2)",
                [],
            );
        }

        let migrations: Vec<(i32, &str)> = vec![
            (1, MIGRATION_001),
            (2, MIGRATION_002),
        ];

        for (ver, sql) in migrations {
            let is_applied: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = ?1)",
                [ver],
                |row| row.get(0),
            ).unwrap_or(false);

            if !is_applied {
                tracing::info!("Applying database migration {:03}...", ver);
                conn.execute_batch(sql).map_err(|e| {
                    AppError::Database(format!("Migration {:03} failed: {}", ver, e))
                })?;

                conn.execute(
                    "INSERT INTO schema_migrations (version) VALUES (?1)",
                    [ver],
                ).map_err(|e| AppError::Database(e.to_string()))?;
            }
        }

        // 4. Backfill sequence numbers for legacy messages where seq == 0
        let zero_seq_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE seq = 0",
            [],
            |row| row.get(0),
        ).unwrap_or(0);

        if zero_seq_count > 0 {
            tracing::info!("Backfilling seq for {} legacy messages...", zero_seq_count);
            let _ = conn.execute_batch(
                "UPDATE messages
                 SET seq = (
                     SELECT COUNT(*)
                     FROM messages m2
                     WHERE m2.conversation_id = messages.conversation_id
                       AND (m2.created_at < messages.created_at OR (m2.created_at = messages.created_at AND m2.rowid <= messages.rowid))
                 )
                 WHERE seq = 0;"
            );
        }

        Ok(())
    }

    pub fn default_path() -> PathBuf {
        let mut path = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("Seralyn");
        path.push("data");
        std::fs::create_dir_all(&path).ok();
        path.push("seralyn.db");
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
