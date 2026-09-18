use serde::{Deserialize, Serialize};
use crate::app::db::Database;
use crate::app::error::{AppError, Result};
use chrono::Utc;
use uuid::Uuid;
use rusqlite::params;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSnapshotRecord {
    pub id: String,
    pub conversation_id: String,
    pub provider_session_id: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub context_tokens: Option<i64>,
    pub context_window: Option<i64>,
    pub confidence: String,
    pub created_at: String,
}

pub fn create_usage_snapshot(
    db: &Database,
    conversation_id: &str,
    provider_session_id: Option<&str>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    cache_write_tokens: Option<i64>,
    reasoning_tokens: Option<i64>,
    context_tokens: Option<i64>,
    context_window: Option<i64>,
    confidence: &str,
) -> Result<UsageSnapshotRecord> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    let conn = db.conn.lock().unwrap();
    conn.execute(
        "INSERT INTO usage_snapshots (
            id, conversation_id, provider_session_id,
            input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
            reasoning_tokens, context_tokens, context_window,
            confidence, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            id,
            conversation_id,
            provider_session_id,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
            reasoning_tokens,
            context_tokens,
            context_window,
            confidence,
            now
        ],
    ).map_err(|e| AppError::Database(e.to_string()))?;

    Ok(UsageSnapshotRecord {
        id,
        conversation_id: conversation_id.to_string(),
        provider_session_id: provider_session_id.map(|s| s.to_string()),
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        reasoning_tokens,
        context_tokens,
        context_window,
        confidence: confidence.to_string(),
        created_at: now,
    })
}

pub fn get_latest_usage_snapshot(
    db: &Database,
    conversation_id: &str,
) -> Result<Option<UsageSnapshotRecord>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, conversation_id, provider_session_id,
                input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                reasoning_tokens, context_tokens, context_window,
                confidence, created_at
         FROM usage_snapshots
         WHERE conversation_id = ?1
         ORDER BY created_at DESC
         LIMIT 1",
    ).map_err(|e| AppError::Database(e.to_string()))?;

    let mut iter = stmt.query_map(params![conversation_id], |row| {
        Ok(UsageSnapshotRecord {
            id: row.get(0)?,
            conversation_id: row.get(1)?,
            provider_session_id: row.get(2)?,
            input_tokens: row.get(3)?,
            output_tokens: row.get(4)?,
            cache_read_tokens: row.get(5)?,
            cache_write_tokens: row.get(6)?,
            reasoning_tokens: row.get(7)?,
            context_tokens: row.get(8)?,
            context_window: row.get(9)?,
            confidence: row.get(10)?,
            created_at: row.get(11)?,
        })
    }).map_err(|e| AppError::Database(e.to_string()))?;

    if let Some(result) = iter.next() {
        Ok(Some(result.map_err(|e| AppError::Database(e.to_string()))?))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::db::conversations::create_conversation;

    #[test]
    fn test_usage_snapshot_crud() {
        let db = Database::new_in_memory().unwrap();
        db.run_migrations().unwrap();

        let conv = create_conversation(&db, Some("Usage Test")).unwrap();
        let record = create_usage_snapshot(
            &db,
            &conv.id,
            None,
            Some(120),
            Some(45),
            Some(10),
            None,
            Some(30),
            Some(165),
            Some(200000),
            "EXACT",
        ).unwrap();

        assert_eq!(record.conversation_id, conv.id);
        assert_eq!(record.input_tokens, Some(120));
        assert_eq!(record.context_window, Some(200000));

        let latest = get_latest_usage_snapshot(&db, &conv.id).unwrap().unwrap();
        assert_eq!(latest.id, record.id);
        assert_eq!(latest.context_tokens, Some(165));
    }
}
