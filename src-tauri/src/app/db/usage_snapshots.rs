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
    account: Option<&str>,
) -> Result<Option<UsageSnapshotRecord>> {
    if let Some(prov) = provider {
        let target_account = account.unwrap_or("default");
        let conn = db.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT u.id, u.conversation_id, u.provider_session_id,
                    u.input_tokens, u.output_tokens, u.cache_read_tokens, u.cache_write_tokens,
                    u.reasoning_tokens, u.context_tokens, u.context_window,
                    u.confidence, u.created_at, ps.metadata_json
             FROM usage_snapshots u
             JOIN provider_sessions ps ON u.provider_session_id = ps.id
             WHERE u.conversation_id = ?1 AND ps.provider = ?2
             ORDER BY u.created_at DESC",
        ).map_err(|e| AppError::Database(e.to_string()))?;

        let rows = stmt.query_map(params![conversation_id, prov], |row| {
            let meta: Option<String> = row.get(12)?;
            let rec = UsageSnapshotRecord {
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
            };
            Ok((rec, meta))
        }).map_err(|e| AppError::Database(e.to_string()))?;

        for r in rows {
            let (rec, meta_opt) = r.map_err(|e| AppError::Database(e.to_string()))?;
            if let Some(ref meta) = meta_opt {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(meta) {
                    if let Some(acc) = val.get("account").and_then(|v| v.as_str()) {
                        if acc == target_account {
                            return Ok(Some(rec));
                        }
                    }
                }
            } else if target_account == "default" {
                // Legacy session without metadata_json belongs to default
                return Ok(Some(rec));
            }
        }
        return Ok(None);
    }

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

pub fn get_latest_usage_snapshot_for_session(
    db: &Database,
    conversation_id: &str,
    provider_session_id: &str,
) -> Result<Option<UsageSnapshotRecord>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, conversation_id, provider_session_id,
                input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                reasoning_tokens, context_tokens, context_window,
                confidence, created_at
         FROM usage_snapshots
         WHERE conversation_id = ?1 AND provider_session_id = ?2
         ORDER BY created_at DESC
         LIMIT 1",
    ).map_err(|e| AppError::Database(e.to_string()))?;

    let mut iter = stmt.query_map(params![conversation_id, provider_session_id], |row| {
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

        let latest = get_latest_usage_snapshot(&db, &conv.id, None, None).unwrap().unwrap();
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
        let claude_res = get_latest_usage_snapshot(&db, &conv.id, Some("claude"), None).unwrap().unwrap();
        assert_eq!(claude_res.id, claude_snapshot.id);
        assert_eq!(claude_res.context_tokens, Some(600));

        // 4. Query Gemini: must return Gemini's snapshot (1500 tokens)
        let gemini_res = get_latest_usage_snapshot(&db, &conv.id, Some("gemini"), None).unwrap().unwrap();
        assert_eq!(gemini_res.id, gemini_snapshot.id);
        assert_eq!(gemini_res.context_tokens, Some(1500));

        // 5. Query Codex: hasn't run yet in this conversation -> must return None, NOT Claude or Gemini's usage!
        let codex_res = get_latest_usage_snapshot(&db, &conv.id, Some("codex"), None).unwrap();
        assert!(codex_res.is_none());

        // 6. Query None: returns latest across conversation (Gemini)
        let latest_any = get_latest_usage_snapshot(&db, &conv.id, None, None).unwrap().unwrap();
        assert_eq!(latest_any.id, gemini_snapshot.id);
    }

    #[test]
    fn test_usage_snapshot_account_and_session_isolation() {
        use crate::app::db::provider_sessions::create_provider_session_with_metadata;

        let db = Database::new_in_memory().unwrap();
        db.run_migrations().unwrap();

        let conv = create_conversation(&db, Some("Account Scope Test")).unwrap();

        // Work session: Claude Opus 5 with 400K context tokens
        let work_meta = serde_json::json!({ "account": "work" }).to_string();
        let work_ps = create_provider_session_with_metadata(
            &db,
            &conv.id,
            "claude",
            Some("native-work"),
            Some("opus"),
            Some(&work_meta),
        ).unwrap();
        let work_snapshot = create_usage_snapshot(
            &db,
            &conv.id,
            Some(&work_ps.id),
            Some(380_000),
            Some(20_000),
            None,
            None,
            None,
            Some(400_000),
            Some(1_000_000),
            "EXACT",
        ).unwrap();

        // Personal session: Claude Haiku 4.5 with 120K context tokens
        let personal_meta = serde_json::json!({ "account": "personal" }).to_string();
        let personal_ps = create_provider_session_with_metadata(
            &db,
            &conv.id,
            "claude",
            Some("native-personal"),
            Some("haiku"),
            Some(&personal_meta),
        ).unwrap();
        let personal_snapshot = create_usage_snapshot(
            &db,
            &conv.id,
            Some(&personal_ps.id),
            Some(110_000),
            Some(10_000),
            None,
            None,
            None,
            Some(120_000),
            Some(200_000),
            "EXACT",
        ).unwrap();

        // Exact query by account "work" returns Opus snapshot (400K)
        let work_query = get_latest_usage_snapshot(&db, &conv.id, Some("claude"), Some("work")).unwrap().unwrap();
        assert_eq!(work_query.id, work_snapshot.id);
        assert_eq!(work_query.context_tokens, Some(400_000));
        assert_eq!(work_query.context_window, Some(1_000_000));

        // Exact query by account "personal" returns Haiku snapshot (120K)
        let personal_query = get_latest_usage_snapshot(&db, &conv.id, Some("claude"), Some("personal")).unwrap().unwrap();
        assert_eq!(personal_query.id, personal_snapshot.id);
        assert_eq!(personal_query.context_tokens, Some(120_000));
        assert_eq!(personal_query.context_window, Some(200_000));

        // Query by session ID directly
        let work_sess_query = get_latest_usage_snapshot_for_session(&db, &conv.id, &work_ps.id).unwrap().unwrap();
        assert_eq!(work_sess_query.id, work_snapshot.id);

        let personal_sess_query = get_latest_usage_snapshot_for_session(&db, &conv.id, &personal_ps.id).unwrap().unwrap();
        assert_eq!(personal_sess_query.id, personal_snapshot.id);
    }
}
