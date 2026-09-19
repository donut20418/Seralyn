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
    for r in rows {
        let rec = r.map_err(|e| AppError::Database(e.to_string()))?;
        if let Some(ref meta) = rec.metadata_json {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(meta) {
                if v.get("account").and_then(|a| a.as_str()) == Some(target_account) {
                    return Ok(Some(rec));
                }
            }
        } else if target_account == "default" {
            // Legacy record without metadata_json belongs strictly to default account
            return Ok(Some(rec));
        }
    }
    Ok(None)
}

pub fn get_active_session_for_account_model(
    db: &Database,
    conversation_id: &str,
    provider: &str,
    account: Option<&str>,
    model: Option<&str>,
) -> Result<Option<ProviderSessionRecord>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, conversation_id, provider, provider_session_id, model, created_at, last_used_at, status, synced_through_seq, metadata_json
         FROM provider_sessions 
         WHERE conversation_id = ?1 AND provider = ?2 AND status = 'active'
         ORDER BY last_used_at DESC, created_at DESC"
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
    for r in rows {
        let rec = r.map_err(|e| AppError::Database(e.to_string()))?;
        let account_matches = if let Some(ref meta) = rec.metadata_json {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(meta) {
                v.get("account").and_then(|a| a.as_str()) == Some(target_account)
            } else {
                false
            }
        } else {
            target_account == "default"
        };

        if !account_matches {
            continue;
        }

        let model_matches = match (model, rec.model.as_deref()) {
            (Some(req_m), Some(rec_m)) => req_m == rec_m,
            (None, None) => true,
            (None, Some(_)) => true, // If caller did not constrain model, accept any
            _ => false,
        };

        if model_matches {
            return Ok(Some(rec));
        }
    }
    Ok(None)
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

    #[test]
    fn test_active_session_for_account_model_isolation() {
        let db = Database::new_in_memory().unwrap();
        db.run_migrations().unwrap();

        let conv = create_conversation(&db, Some("Model Isolation Test")).unwrap();

        // 1. Create Work Opus 5 session (cursor 500)
        let work_meta = serde_json::json!({ "account": "work" }).to_string();
        let opus_sess = create_provider_session_with_metadata(
            &db,
            &conv.id,
            "claude",
            Some("native-opus-123"),
            Some("opus"),
            Some(&work_meta),
        ).unwrap();
        update_synced_seq(&db, &opus_sess.id, 500).unwrap();

        // 2. Create Work Haiku 4.5 session (cursor 530)
        let haiku_sess = create_provider_session_with_metadata(
            &db,
            &conv.id,
            "claude",
            Some("native-haiku-456"),
            Some("haiku"),
            Some(&work_meta),
        ).unwrap();
        update_synced_seq(&db, &haiku_sess.id, 530).unwrap();

        // 3. Lookup Work Opus: must find opus_sess with cursor 500 (NOT haiku_sess!)
        let opus_lookup = get_active_session_for_account_model(
            &db,
            &conv.id,
            "claude",
            Some("work"),
            Some("opus"),
        ).unwrap().unwrap();
        assert_eq!(opus_lookup.id, opus_sess.id);
        assert_eq!(opus_lookup.synced_through_seq, 500);
        assert_eq!(opus_lookup.model.as_deref(), Some("opus"));

        // 4. Lookup Work Haiku: must find haiku_sess with cursor 530
        let haiku_lookup = get_active_session_for_account_model(
            &db,
            &conv.id,
            "claude",
            Some("work"),
            Some("haiku"),
        ).unwrap().unwrap();
        assert_eq!(haiku_lookup.id, haiku_sess.id);
        assert_eq!(haiku_lookup.synced_through_seq, 530);
        assert_eq!(haiku_lookup.model.as_deref(), Some("haiku"));

        // 5. Lookup non-existent model Sonnet for Work: returns None cleanly
        let sonnet_lookup = get_active_session_for_account_model(
            &db,
            &conv.id,
            "claude",
            Some("work"),
            Some("sonnet"),
        ).unwrap();
        assert!(sonnet_lookup.is_none(), "Unused model Sonnet must return None");

        // 6. Lookup Opus under Personal: returns None cleanly
        let personal_lookup = get_active_session_for_account_model(
            &db,
            &conv.id,
            "claude",
            Some("personal"),
            Some("opus"),
        ).unwrap();
        assert!(personal_lookup.is_none(), "Personal account must not find Work session");
    }
}
