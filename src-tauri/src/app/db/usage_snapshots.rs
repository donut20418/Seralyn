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
    provider: Option<&str>,
) -> Result<Option<UsageSnapshotRecord>> {
    let conn = db.conn.lock().unwrap();

    if let Some(prov) = provider {
        let mut stmt = conn.prepare(
            "SELECT u.id, u.conversation_id, u.provider_session_id,
                    u.input_tokens, u.output_tokens, u.cache_read_tokens, u.cache_write_tokens,
                    u.reasoning_tokens, u.context_tokens, u.context_window,
                    u.confidence, u.created_at
             FROM usage_snapshots u
             JOIN provider_sessions ps ON u.provider_session_id = ps.id
             WHERE u.conversation_id = ?1 AND ps.provider = ?2
             ORDER BY u.created_at DESC
             LIMIT 1",
        ).map_err(|e| AppError::Database(e.to_string()))?;

        let mut iter = stmt.query_map(params![conversation_id, prov], |row| {
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
            return Ok(Some(result.map_err(|e| AppError::Database(e.to_string()))?));
        }
        return Ok(None);
    }

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
    use crate::app::db::provider_sessions::create_provider_session;

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

        let latest = get_latest_usage_snapshot(&db, &conv.id, None).unwrap().unwrap();
        assert_eq!(latest.id, record.id);
        assert_eq!(latest.context_tokens, Some(165));
    }

    #[test]
    fn test_usage_snapshot_provider_scoping() {
        let db = Database::new_in_memory().unwrap();
        db.run_migrations().unwrap();

        let conv = create_conversation(&db, Some("Provider Scope Test")).unwrap();

        // 1. Create Claude session & snapshot
        let claude_ps = create_provider_session(&db, &conv.id, "claude", Some("claude-session-1"), Some("claude-opus-5")).unwrap();
        let claude_snapshot = create_usage_snapshot(
            &db,
            &conv.id,
            Some(&claude_ps.id),
            Some(500),
            Some(100),
            None,
            None,
            None,
            Some(600),
            Some(200000),
            "EXACT",
        ).unwrap();

        // 2. Create Gemini session & snapshot
        let gemini_ps = create_provider_session(&db, &conv.id, "gemini", Some("gemini-session-1"), Some("gemini-3.8-flash")).unwrap();
        let gemini_snapshot = create_usage_snapshot(
            &db,
            &conv.id,
            Some(&gemini_ps.id),
            Some(1200),
            Some(300),
            None,
            None,
            None,
            Some(1500),
            Some(1000000),
            "EXACT",
        ).unwrap();

        // 3. Query Claude: must return Claude's snapshot (600 tokens)
        let claude_res = get_latest_usage_snapshot(&db, &conv.id, Some("claude")).unwrap().unwrap();
        assert_eq!(claude_res.id, claude_snapshot.id);
        assert_eq!(claude_res.context_tokens, Some(600));

        // 4. Query Gemini: must return Gemini's snapshot (1500 tokens)
        let gemini_res = get_latest_usage_snapshot(&db, &conv.id, Some("gemini")).unwrap().unwrap();
        assert_eq!(gemini_res.id, gemini_snapshot.id);
        assert_eq!(gemini_res.context_tokens, Some(1500));

        // 5. Query Codex: hasn't run yet in this conversation -> must return None, NOT Claude or Gemini's usage!
        let codex_res = get_latest_usage_snapshot(&db, &conv.id, Some("codex")).unwrap();
        assert!(codex_res.is_none());

        // 6. Query None: returns latest across conversation (Gemini)
        let latest_any = get_latest_usage_snapshot(&db, &conv.id, None).unwrap().unwrap();
        assert_eq!(latest_any.id, gemini_snapshot.id);
    }
}
