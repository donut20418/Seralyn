use serde::{Serialize, Deserialize};
use crate::app::db::Database;
use crate::app::error::{AppError, Result};
use chrono::Utc;
use uuid::Uuid;
use rusqlite::{params, OptionalExtension};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub title: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub archived: bool,
    pub metadata_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationSummary {
    pub id: String,
    pub title: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_message_preview: Option<String>,
    pub message_count: i64,
    pub provider: Option<String>,
}

pub fn create_conversation(db: &Database, title: Option<&str>) -> Result<Conversation> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let title_str = title.map(|s| s.to_string());
    
    let conn = db.conn.lock().unwrap();
    conn.execute(
        "INSERT INTO conversations (id, title, created_at, updated_at, archived, metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, title_str, now, now, 0, None::<String>],
    ).map_err(|e| AppError::Database(e.to_string()))?;

    Ok(Conversation {
        id,
        title: title_str,
        created_at: now.clone(),
        updated_at: now,
        archived: false,
        metadata_json: None,
    })
}

pub fn get_conversation(db: &Database, id: &str) -> Result<Conversation> {
    let conn = db.conn.lock().unwrap();
    conn.query_row(
        "SELECT id, title, created_at, updated_at, archived, metadata_json FROM conversations WHERE id = ?1",
        params![id],
        |row| {
            Ok(Conversation {
                id: row.get(0)?,
                title: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
                archived: row.get::<_, i64>(4)? != 0,
                metadata_json: row.get(5)?,
            })
        },
    ).map_err(|e| AppError::Database(e.to_string()))
}

pub fn list_conversations(db: &Database) -> Result<Vec<ConversationSummary>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT c.id, c.title, c.created_at, c.updated_at,
            (SELECT content FROM messages WHERE conversation_id = c.id ORDER BY created_at DESC LIMIT 1) as last_message_preview,
            (SELECT COUNT(*) FROM messages WHERE conversation_id = c.id) as message_count,
            (SELECT provider FROM messages WHERE conversation_id = c.id AND provider IS NOT NULL ORDER BY created_at DESC LIMIT 1) as provider
         FROM conversations c
         WHERE c.archived = 0
         ORDER BY c.updated_at DESC"
    ).map_err(|e| AppError::Database(e.to_string()))?;

    let iter = stmt.query_map([], |row| {
        Ok(ConversationSummary {
            id: row.get(0)?,
            title: row.get(1)?,
            created_at: row.get(2)?,
            updated_at: row.get(3)?,
            last_message_preview: row.get::<_, Option<String>>(4)?.map(|s| s.chars().take(50).collect()),
            message_count: row.get(5)?,
            provider: row.get(6)?,
        })
    }).map_err(|e| AppError::Database(e.to_string()))?;

    let mut res = Vec::new();
    for row in iter {
        res.push(row.map_err(|e| AppError::Database(e.to_string()))?);
    }
    Ok(res)
}

pub fn update_conversation_title(db: &Database, id: &str, title: &str) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let conn = db.conn.lock().unwrap();
    conn.execute(
        "UPDATE conversations SET title = ?1, updated_at = ?2 WHERE id = ?3",
        params![title, now, id],
    ).map_err(|e| AppError::Database(e.to_string()))?;
    Ok(())
}

pub fn delete_conversation(db: &Database, id: &str) -> Result<()> {
    let conn = db.conn.lock().unwrap();
    conn.execute(
        "DELETE FROM conversations WHERE id = ?1",
        params![id],
    ).map_err(|e| AppError::Database(e.to_string()))?;
    Ok(())
}

pub fn archive_conversation(db: &Database, id: &str) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let conn = db.conn.lock().unwrap();
    conn.execute(
        "UPDATE conversations SET archived = 1, updated_at = ?1 WHERE id = ?2",
        params![now, id],
    ).map_err(|e| AppError::Database(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conversations_crud() {
        let db = Database::new_in_memory().unwrap();
        db.run_migrations().unwrap();

        let conv = create_conversation(&db, Some("Test Title")).unwrap();
        assert_eq!(conv.title.unwrap(), "Test Title");

        let fetched = get_conversation(&db, &conv.id).unwrap();
        assert_eq!(fetched.id, conv.id);

        let list = list_conversations(&db).unwrap();
        assert_eq!(list.len(), 1);

        update_conversation_title(&db, &conv.id, "New Title").unwrap();
        let updated = get_conversation(&db, &conv.id).unwrap();
        assert_eq!(updated.title.unwrap(), "New Title");

        archive_conversation(&db, &conv.id).unwrap();
        let list2 = list_conversations(&db).unwrap();
        assert_eq!(list2.len(), 0);

        delete_conversation(&db, &conv.id).unwrap();
        assert!(get_conversation(&db, &conv.id).is_err());
    }
}
