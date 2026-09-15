use serde::{Serialize, Deserialize};
use crate::app::db::Database;
use crate::app::error::{AppError, Result};
use chrono::Utc;
use uuid::Uuid;
use rusqlite::params;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub parent_id: Option<String>,
    pub role: String,
    pub content: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub provider_session_id: Option<String>,
    pub created_at: String,
    pub token_estimate: Option<i64>,
    pub metadata_json: Option<String>,
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
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    let conn = db.conn.lock().unwrap();
    conn.execute(
        "INSERT INTO messages (
            id, conversation_id, parent_id, role, content, provider, model, provider_session_id, created_at, token_estimate
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![id, conversation_id, parent_id, role, content, provider, model, provider_session_id, now, token_estimate],
    ).map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
        params![now, conversation_id],
    ).map_err(|e| AppError::Database(e.to_string()))?;

    Ok(Message {
        id,
        conversation_id: conversation_id.to_string(),
        parent_id: parent_id.map(|s| s.to_string()),
        role: role.to_string(),
        content: content.to_string(),
        provider: provider.map(|s| s.to_string()),
        model: model.map(|s| s.to_string()),
        provider_session_id: provider_session_id.map(|s| s.to_string()),
        created_at: now,
        token_estimate,
        metadata_json: None,
    })
}

pub fn get_messages(db: &Database, conversation_id: &str) -> Result<Vec<Message>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, conversation_id, parent_id, role, content, provider, model, provider_session_id, created_at, token_estimate, metadata_json
         FROM messages WHERE conversation_id = ?1 ORDER BY created_at ASC"
    ).map_err(|e| AppError::Database(e.to_string()))?;

    let iter = stmt.query_map(params![conversation_id], |row| {
        Ok(Message {
            id: row.get(0)?,
            conversation_id: row.get(1)?,
            parent_id: row.get(2)?,
            role: row.get(3)?,
            content: row.get(4)?,
            provider: row.get(5)?,
            model: row.get(6)?,
            provider_session_id: row.get(7)?,
            created_at: row.get(8)?,
            token_estimate: row.get(9)?,
            metadata_json: row.get(10)?,
        })
    }).map_err(|e| AppError::Database(e.to_string()))?;

    let mut res = Vec::new();
    for row in iter {
        res.push(row.map_err(|e| AppError::Database(e.to_string()))?);
    }
    Ok(res)
}

pub fn get_message(db: &Database, id: &str) -> Result<Message> {
    let conn = db.conn.lock().unwrap();
    conn.query_row(
        "SELECT id, conversation_id, parent_id, role, content, provider, model, provider_session_id, created_at, token_estimate, metadata_json
         FROM messages WHERE id = ?1",
        params![id],
        |row| {
            Ok(Message {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                parent_id: row.get(2)?,
                role: row.get(3)?,
                content: row.get(4)?,
                provider: row.get(5)?,
                model: row.get(6)?,
                provider_session_id: row.get(7)?,
                created_at: row.get(8)?,
                token_estimate: row.get(9)?,
                metadata_json: row.get(10)?,
            })
        },
    ).map_err(|e| AppError::Database(e.to_string()))
}

pub fn get_recent_messages(db: &Database, conversation_id: &str, limit: usize) -> Result<Vec<Message>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT * FROM (
            SELECT id, conversation_id, parent_id, role, content, provider, model, provider_session_id, created_at, token_estimate, metadata_json
            FROM messages WHERE conversation_id = ?1 ORDER BY created_at DESC LIMIT ?2
         ) ORDER BY created_at ASC"
    ).map_err(|e| AppError::Database(e.to_string()))?;

    let iter = stmt.query_map(params![conversation_id, limit], |row| {
        Ok(Message {
            id: row.get(0)?,
            conversation_id: row.get(1)?,
            parent_id: row.get(2)?,
            role: row.get(3)?,
            content: row.get(4)?,
            provider: row.get(5)?,
            model: row.get(6)?,
            provider_session_id: row.get(7)?,
            created_at: row.get(8)?,
            token_estimate: row.get(9)?,
            metadata_json: row.get(10)?,
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

        let fetched = get_message(&db, &msg.id).unwrap();
        assert_eq!(fetched.id, msg.id);

        let msgs = get_messages(&db, &conv.id).unwrap();
        assert_eq!(msgs.len(), 1);

        let count = count_messages(&db, &conv.id).unwrap();
        assert_eq!(count, 1);

        create_message(&db, &conv.id, None, "assistant", "hi", None, None, None, None).unwrap();
        let recent = get_recent_messages(&db, &conv.id, 1).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].content, "hi");
    }
}
