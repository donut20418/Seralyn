use serde::{Serialize, Deserialize};
use crate::app::db::Database;
use crate::app::error::{AppError, Result};
use chrono::Utc;
use uuid::Uuid;
use rusqlite::params;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSessionRecord {
    pub id: String,
    pub conversation_id: String,
    pub provider: String,
    pub provider_session_id: Option<String>,
    pub model: Option<String>,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub status: String,
    pub synced_through_seq: i64,
    pub metadata_json: Option<String>,
}

pub fn create_provider_session(
    db: &Database,
    conversation_id: &str,
    provider: &str,
    provider_session_id: Option<&str>,
    model: Option<&str>,
) -> Result<ProviderSessionRecord> {
    create_provider_session_with_metadata(db, conversation_id, provider, provider_session_id, model, None)
}

pub fn create_provider_session_with_metadata(
    db: &Database,
    conversation_id: &str,
    provider: &str,
    provider_session_id: Option<&str>,
    model: Option<&str>,
    metadata_json: Option<&str>,
) -> Result<ProviderSessionRecord> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    let conn = db.conn.lock().unwrap();
    conn.execute(
        "INSERT INTO provider_sessions (id, conversation_id, provider, provider_session_id, model, created_at, status, synced_through_seq, metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![id, conversation_id, provider, provider_session_id, model, now, "active", 0, metadata_json],
    ).map_err(|e| AppError::Database(e.to_string()))?;

    Ok(ProviderSessionRecord {
        id,
        conversation_id: conversation_id.to_string(),
        provider: provider.to_string(),
        provider_session_id: provider_session_id.map(|s| s.to_string()),
        model: model.map(|s| s.to_string()),
        created_at: now,
        last_used_at: None,
        status: "active".to_string(),
        synced_through_seq: 0,
        metadata_json: metadata_json.map(|s| s.to_string()),
    })
}

pub fn get_active_session(db: &Database, conversation_id: &str, provider: &str) -> Result<Option<ProviderSessionRecord>> {
    get_active_session_for_account(db, conversation_id, provider, None)
}

pub fn get_active_session_for_account(
    db: &Database,
    conversation_id: &str,
    provider: &str,
    account: Option<&str>,
) -> Result<Option<ProviderSessionRecord>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, conversation_id, provider, provider_session_id, model, created_at, last_used_at, status, synced_through_seq, metadata_json
         FROM provider_sessions 
         WHERE conversation_id = ?1 AND provider = ?2 AND status = 'active'
         ORDER BY created_at DESC"
    ).map_err(|e| AppError::Database(e.to_string()))?;

    let rows = stmt.query_map(params![conversation_id, provider], |row| {
        Ok(ProviderSessionRecord {
            id: row.get(0)?,
            conversation_id: row.get(1)?,
            provider: row.get(2)?,
            provider_session_id: row.get(3)?,
            model: row.get(4)?,
            created_at: row.get(5)?,
            last_used_at: row.get(6)?,
            status: row.get(7)?,
            synced_through_seq: row.get(8)?,
            metadata_json: row.get(9)?,
        })
    }).map_err(|e| AppError::Database(e.to_string()))?;

    let target_account = account.unwrap_or("default");
    let mut fallback = None;
    for r in rows {
        let rec = r.map_err(|e| AppError::Database(e.to_string()))?;
        if let Some(ref meta) = rec.metadata_json {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(meta) {
                if v.get("account").and_then(|a| a.as_str()) == Some(target_account) {
                    return Ok(Some(rec));
                }
            }
        } else if target_account == "default" {
            return Ok(Some(rec));
        }
        if fallback.is_none() {
            fallback = Some(rec);
        }
    }
    if account.is_none() || target_account == "default" {
        Ok(fallback)
    } else {
        Ok(None)
    }
}

pub fn update_session_used(db: &Database, id: &str) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let conn = db.conn.lock().unwrap();
    conn.execute(
        "UPDATE provider_sessions SET last_used_at = ?1 WHERE id = ?2",
        params![now, id],
    ).map_err(|e| AppError::Database(e.to_string()))?;
    Ok(())
}

pub fn update_native_session_id(db: &Database, id: &str, native_session_id: &str) -> Result<()> {
    if native_session_id.trim().is_empty() {
        return Ok(());
    }
    let conn = db.conn.lock().unwrap();
    conn.execute(
        "UPDATE provider_sessions SET provider_session_id = ?1 WHERE id = ?2",
        params![native_session_id, id],
    ).map_err(|e| AppError::Database(e.to_string()))?;
    Ok(())
}

pub fn update_synced_seq(db: &Database, id: &str, seq: i64) -> Result<()> {
    let conn = db.conn.lock().unwrap();
    conn.execute(
        "UPDATE provider_sessions SET synced_through_seq = ?1 WHERE id = ?2",
        params![seq, id],
    ).map_err(|e| AppError::Database(e.to_string()))?;
    Ok(())
}

pub fn close_session(db: &Database, id: &str) -> Result<()> {
    let conn = db.conn.lock().unwrap();
    conn.execute(
        "UPDATE provider_sessions SET status = 'closed' WHERE id = ?1",
        params![id],
    ).map_err(|e| AppError::Database(e.to_string()))?;
    Ok(())
}

pub fn get_sessions_for_conversation(db: &Database, conversation_id: &str) -> Result<Vec<ProviderSessionRecord>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, conversation_id, provider, provider_session_id, model, created_at, last_used_at, status, synced_through_seq, metadata_json
         FROM provider_sessions WHERE conversation_id = ?1 ORDER BY created_at DESC"
    ).map_err(|e| AppError::Database(e.to_string()))?;

    let iter = stmt.query_map(params![conversation_id], |row| {
        Ok(ProviderSessionRecord {
            id: row.get(0)?,
            conversation_id: row.get(1)?,
            provider: row.get(2)?,
            provider_session_id: row.get(3)?,
            model: row.get(4)?,
            created_at: row.get(5)?,
            last_used_at: row.get(6)?,
            status: row.get(7)?,
            synced_through_seq: row.get(8)?,
            metadata_json: row.get(9)?,
        })
    }).map_err(|e| AppError::Database(e.to_string()))?;

    let mut res = Vec::new();
    for row in iter {
        res.push(row.map_err(|e| AppError::Database(e.to_string()))?);
    }
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::db::conversations::create_conversation;

    #[test]
    fn test_provider_sessions_crud() {
        let db = Database::new_in_memory().unwrap();
        db.run_migrations().unwrap();

        let conv = create_conversation(&db, None).unwrap();
        let session = create_provider_session(&db, &conv.id, "codex", Some("sess_123"), Some("gpt-4")).unwrap();
        assert_eq!(session.provider, "codex");
        assert_eq!(session.synced_through_seq, 0);

        let active = get_active_session(&db, &conv.id, "codex").unwrap().unwrap();
        assert_eq!(active.id, session.id);
        assert_eq!(active.synced_through_seq, 0);

        update_session_used(&db, &session.id).unwrap();
        update_native_session_id(&db, &session.id, "sess_456").unwrap();
        update_synced_seq(&db, &session.id, 5).unwrap();

        let updated = get_active_session(&db, &conv.id, "codex").unwrap().unwrap();
        assert_eq!(updated.synced_through_seq, 5);
        
        let sessions = get_sessions_for_conversation(&db, &conv.id).unwrap();
        assert_eq!(sessions.len(), 1);
        assert!(sessions[0].last_used_at.is_some());
        assert_eq!(sessions[0].provider_session_id.as_deref(), Some("sess_456"));
        assert_eq!(sessions[0].synced_through_seq, 5);

        close_session(&db, &session.id).unwrap();
        let closed = get_active_session(&db, &conv.id, "codex").unwrap();
        assert!(closed.is_none());
    }
}
