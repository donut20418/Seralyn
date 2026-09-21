use serde::{Serialize, Deserialize};
use crate::app::db::Database;
use crate::app::db::attachments::AttachmentRecord;
use crate::app::error::{AppError, Result};
use chrono::Utc;
use uuid::Uuid;
use rusqlite::params;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub seq: i64,
    pub parent_id: Option<String>,
    pub role: String,
    pub content: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub provider_session_id: Option<String>,
    pub created_at: String,
    pub token_estimate: Option<i64>,
    pub metadata_json: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<AttachmentRecord>,
}

#[allow(clippy::too_many_arguments)]
pub fn create_message(
    db: &Database,
    conversation_id: &str,
    parent_id: Option<&str>,
    role: &str,
    content: &str,
    provider: Option<&str>,
    model: Option<&str>,
    provider_session_id: Option<&str>,
    token_estimate: Option<i64>,
) -> Result<Message> {
    create_message_with_metadata(
        db,
        conversation_id,
        parent_id,
        role,
        content,
        provider,
        model,
        provider_session_id,
        token_estimate,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn create_message_with_metadata(
    db: &Database,
    conversation_id: &str,
    parent_id: Option<&str>,
    role: &str,
    content: &str,
    provider: Option<&str>,
    model: Option<&str>,
    provider_session_id: Option<&str>,
    token_estimate: Option<i64>,
    metadata_json: Option<&str>,
) -> Result<Message> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    let conn = db.conn.lock().unwrap();

    let next_seq: i64 = conn.query_row(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM messages WHERE conversation_id = ?1",
        params![conversation_id],
        |row| row.get(0),
    ).map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "INSERT INTO messages (
            id, conversation_id, seq, parent_id, role, content, provider, model, provider_session_id, created_at, token_estimate, metadata_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![id, conversation_id, next_seq, parent_id, role, content, provider, model, provider_session_id, now, token_estimate, metadata_json],
    ).map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
        params![now, conversation_id],
    ).map_err(|e| AppError::Database(e.to_string()))?;

    Ok(Message {
        id,
        conversation_id: conversation_id.to_string(),
        seq: next_seq,
        parent_id: parent_id.map(|s| s.to_string()),
        role: role.to_string(),
        content: content.to_string(),
        provider: provider.map(|s| s.to_string()),
        model: model.map(|s| s.to_string()),
        provider_session_id: provider_session_id.map(|s| s.to_string()),
        created_at: now,
        token_estimate,
        metadata_json: metadata_json.map(|s| s.to_string()),
        attachments: Vec::new(),
    })
}

pub fn get_messages(db: &Database, conversation_id: &str) -> Result<Vec<Message>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, conversation_id, seq, parent_id, role, content, provider, model, provider_session_id, created_at, token_estimate, metadata_json
         FROM messages WHERE conversation_id = ?1 ORDER BY seq ASC"
    ).map_err(|e| AppError::Database(e.to_string()))?;

    let iter = stmt.query_map(params![conversation_id], |row| {
        Ok(Message {
            id: row.get(0)?,
            conversation_id: row.get(1)?,
            seq: row.get(2)?,
            parent_id: row.get(3)?,
            role: row.get(4)?,
            content: row.get(5)?,
            provider: row.get(6)?,
            model: row.get(7)?,
            provider_session_id: row.get(8)?,
            created_at: row.get(9)?,
            token_estimate: row.get(10)?,
            metadata_json: row.get(11)?,
            attachments: Vec::new(),
        })
    }).map_err(|e| AppError::Database(e.to_string()))?;

    let mut res = Vec::new();
    for row in iter {
        res.push(row.map_err(|e| AppError::Database(e.to_string()))?);
    }

    let has_attachments_table: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='attachments')",
        [],
        |row| row.get(0),
    ).unwrap_or(false);

    if has_attachments_table {
        let mut att_stmt = conn.prepare(
            "SELECT id, conversation_id, message_id, name, stored_name, mime_type, size_bytes, sha256, kind, state, created_at
             FROM attachments
             WHERE conversation_id = ?1 AND state = 'attached' AND message_id IS NOT NULL
             ORDER BY rowid ASC",
        ).map_err(|e| AppError::Database(e.to_string()))?;

        let att_iter = att_stmt.query_map(params![conversation_id], |row| {
            let kind_str: String = row.get(8)?;
            let kind = kind_str.parse::<crate::app::attachments::AttachmentKind>()
                .unwrap_or(crate::app::attachments::AttachmentKind::Binary);
            let size_raw: i64 = row.get(6)?;
            let msg_id: String = row.get(2)?;

            Ok((msg_id, AttachmentRecord {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                message_id: Some(row.get(2)?),
                name: row.get(3)?,
                stored_name: row.get(4)?,
                mime_type: row.get(5)?,
                size_bytes: size_raw as u64,
                size: size_raw as u64,
                sha256: row.get(7)?,
                kind,
                state: row.get(9)?,
                created_at: row.get(10)?,
            }))
        }).map_err(|e| AppError::Database(e.to_string()))?;

        let mut att_map: std::collections::HashMap<String, Vec<AttachmentRecord>> = std::collections::HashMap::new();
        for r in att_iter {
            if let Ok((mid, att)) = r {
                att_map.entry(mid).or_default().push(att);
            }
        }

        for msg in &mut res {
            if let Some(atts) = att_map.remove(&msg.id) {
                msg.attachments = atts;
            }
        }
    }

    Ok(res)
}

pub fn get_messages_for_conversation(db: &Database, conversation_id: &str) -> Result<Vec<Message>> {
    get_messages(db, conversation_id)
}

pub fn get_message(db: &Database, id: &str) -> Result<Message> {
    let mut msg = {
        let conn = db.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, conversation_id, seq, parent_id, role, content, provider, model, provider_session_id, created_at, token_estimate, metadata_json
             FROM messages WHERE id = ?1",
            params![id],
            |row| {
                Ok(Message {
                    id: row.get(0)?,
                    conversation_id: row.get(1)?,
                    seq: row.get(2)?,
                    parent_id: row.get(3)?,
                    role: row.get(4)?,
                    content: row.get(5)?,
                    provider: row.get(6)?,
                    model: row.get(7)?,
                    provider_session_id: row.get(8)?,
                    created_at: row.get(9)?,
                    token_estimate: row.get(10)?,
                    metadata_json: row.get(11)?,
                    attachments: Vec::new(),
                })
            },
        ).map_err(|e| AppError::Database(e.to_string()))?
    };

    if let Ok(atts) = crate::app::db::attachments::get_attachments_for_message(db, id) {
        msg.attachments = atts;
    }

    Ok(msg)
}

pub fn get_recent_messages(db: &Database, conversation_id: &str, limit: usize) -> Result<Vec<Message>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT * FROM (
            SELECT id, conversation_id, seq, parent_id, role, content, provider, model, provider_session_id, created_at, token_estimate, metadata_json
            FROM messages WHERE conversation_id = ?1 ORDER BY seq DESC LIMIT ?2
         ) ORDER BY seq ASC"
    ).map_err(|e| AppError::Database(e.to_string()))?;

    let iter = stmt.query_map(params![conversation_id, limit], |row| {
        Ok(Message {
            id: row.get(0)?,
            conversation_id: row.get(1)?,
            seq: row.get(2)?,
            parent_id: row.get(3)?,
            role: row.get(4)?,
            content: row.get(5)?,
            provider: row.get(6)?,
            model: row.get(7)?,
            provider_session_id: row.get(8)?,
            created_at: row.get(9)?,
            token_estimate: row.get(10)?,
            metadata_json: row.get(11)?,
            attachments: Vec::new(),
        })
    }).map_err(|e| AppError::Database(e.to_string()))?;

    let mut res = Vec::new();
    for row in iter {
        res.push(row.map_err(|e| AppError::Database(e.to_string()))?);
    }
    Ok(res)
}

pub fn count_messages(db: &Database, conversation_id: &str) -> Result<i64> {
    let conn = db.conn.lock().unwrap();
    conn.query_row(
        "SELECT COUNT(*) FROM messages WHERE conversation_id = ?1",
        params![conversation_id],
        |row| row.get(0),
    ).map_err(|e| AppError::Database(e.to_string()))
}

pub fn get_latest_seq(db: &Database, conversation_id: &str) -> Result<i64> {
    let conn = db.conn.lock().unwrap();
    conn.query_row(
        "SELECT COALESCE(MAX(seq), 0) FROM messages WHERE conversation_id = ?1",
        params![conversation_id],
        |row| row.get(0),
    ).map_err(|e| AppError::Database(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::db::conversations::create_conversation;

    #[test]
    fn test_messages_crud() {
        let db = Database::new_in_memory().unwrap();
        db.run_migrations().unwrap();

        let conv = create_conversation(&db, None).unwrap();
        let msg = create_message(&db, &conv.id, None, "user", "hello", None, None, None, None).unwrap();
        assert_eq!(msg.content, "hello");
        assert_eq!(msg.conversation_id, conv.id);
        assert_eq!(msg.seq, 1);

        let fetched = get_message(&db, &msg.id).unwrap();
        assert_eq!(fetched.id, msg.id);
        assert_eq!(fetched.seq, 1);

        let msgs = get_messages(&db, &conv.id).unwrap();
        assert_eq!(msgs.len(), 1);

        let count = count_messages(&db, &conv.id).unwrap();
        assert_eq!(count, 1);

        let msg2 = create_message(&db, &conv.id, None, "assistant", "hi", None, None, None, None).unwrap();
        assert_eq!(msg2.seq, 2);

        let recent = get_recent_messages(&db, &conv.id, 1).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].content, "hi");
        assert_eq!(recent[0].seq, 2);

        // Test metadata persistence (thinking and attachments)
        let meta_json = r#"{"thinking":"Step 1: Analyzed problem\nStep 2: Found solution"}"#;
        let msg3 = create_message_with_metadata(
            &db,
            &conv.id,
            None,
            "assistant",
            "Here is the solution",
            Some("claude"),
            Some("claude-opus-5"),
            None,
            Some(50),
            Some(meta_json),
        ).unwrap();
        assert_eq!(msg3.seq, 3);
        assert_eq!(msg3.metadata_json.as_deref(), Some(meta_json));

        let fetched3 = get_message(&db, &msg3.id).unwrap();
        assert_eq!(fetched3.metadata_json.as_deref(), Some(meta_json));
    }
}
