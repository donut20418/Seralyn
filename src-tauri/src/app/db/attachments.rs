use rusqlite::params;
use serde::{Deserialize, Serialize};
use chrono::Utc;
use uuid::Uuid;

use crate::app::attachments::AttachmentKind;
use crate::app::db::Database;
use crate::app::error::{AppError, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttachmentRecord {
    pub id: String,
    pub conversation_id: String,
    pub message_id: Option<String>,
    pub name: String,
    pub stored_name: String,
    pub mime_type: String,
    pub size_bytes: u64,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub path: String,
    pub sha256: String,
    pub kind: AttachmentKind,
    pub state: String,
    pub created_at: String,
}

/// Creates a new attachment record in the 'staged' state.
#[allow(clippy::too_many_arguments)]
pub fn create_staged_attachment(
    db: &Database,
    conversation_id: &str,
    name: &str,
    stored_name: &str,
    mime_type: &str,
    size_bytes: u64,
    sha256: &str,
    kind: AttachmentKind,
) -> Result<AttachmentRecord> {
    create_staged_attachment_with_id(
        db,
        &Uuid::new_v4().to_string(),
        conversation_id,
        name,
        stored_name,
        mime_type,
        size_bytes,
        sha256,
        kind,
    )
}

/// Creates a staged attachment record with a predetermined ID (used when ID is generated before file write).
#[allow(clippy::too_many_arguments)]
pub fn create_staged_attachment_with_id(
    db: &Database,
    id: &str,
    conversation_id: &str,
    name: &str,
    stored_name: &str,
    mime_type: &str,
    size_bytes: u64,
    sha256: &str,
    kind: AttachmentKind,
) -> Result<AttachmentRecord> {
    let now = Utc::now().to_rfc3339();
    let conn = db.conn.lock().unwrap();

    conn.execute(
        "INSERT INTO attachments (
            id, conversation_id, message_id, name, stored_name, mime_type, size_bytes, sha256, kind, state, created_at
        ) VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, ?8, 'staged', ?9)",
        params![
            id,
            conversation_id,
            name,
            stored_name,
            mime_type,
            size_bytes as i64,
            sha256,
            kind.to_string(),
            now,
        ],
    ).map_err(|e| AppError::Database(e.to_string()))?;

    let path = crate::app::attachments::get_conversation_attachment_dir(conversation_id)
        .map(|d| d.join(stored_name).to_string_lossy().to_string())
        .unwrap_or_default();

    Ok(AttachmentRecord {
        id: id.to_string(),
        conversation_id: conversation_id.to_string(),
        message_id: None,
        name: name.to_string(),
        stored_name: stored_name.to_string(),
        mime_type: mime_type.to_string(),
        size_bytes,
        size: size_bytes,
        path,
        sha256: sha256.to_string(),
        kind,
        state: "staged".to_string(),
        created_at: now,
    })
}

fn map_attachment_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AttachmentRecord> {
    let id: String = row.get(0)?;
    let conversation_id: String = row.get(1)?;
    let message_id: Option<String> = row.get(2)?;
    let name: String = row.get(3)?;
    let stored_name: String = row.get(4)?;
    let mime_type: String = row.get(5)?;
    let size_raw: i64 = row.get(6)?;
    let sha256: String = row.get(7)?;
    let kind_str: String = row.get(8)?;
    let kind = kind_str.parse::<AttachmentKind>().map_err(|_| rusqlite::Error::InvalidQuery)?;
    let state: String = row.get(9)?;
    let created_at: String = row.get(10)?;

    let path = crate::app::attachments::get_conversation_attachment_dir(&conversation_id)
        .map(|d| d.join(&stored_name).to_string_lossy().to_string())
        .unwrap_or_default();

    Ok(AttachmentRecord {
        id,
        conversation_id,
        message_id,
        name,
        stored_name,
        mime_type,
        size_bytes: size_raw as u64,
        size: size_raw as u64,
        path,
        sha256,
        kind,
        state,
        created_at,
    })
}

/// Transitions staged attachments to the 'attached' state and associates them with a message.
pub fn attach_to_message(db: &Database, message_id: &str, attachment_ids: &[String]) -> Result<()> {
    if attachment_ids.is_empty() {
        return Ok(());
    }

    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "UPDATE attachments
         SET state = 'attached', message_id = ?1
         WHERE id = ?2 AND state = 'staged'",
    ).map_err(|e| AppError::Database(e.to_string()))?;

    for id in attachment_ids {
        let rows = stmt.execute(params![message_id, id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        if rows != 1 {
            return Err(AppError::Database(format!(
                "Failed to attach attachment {}: expected 1 affected row, got {}",
                id, rows
            )));
        }
    }

    Ok(())
}

/// Retrieves a single attachment by ID.
pub fn get_attachment(db: &Database, id: &str) -> Result<Option<AttachmentRecord>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, conversation_id, message_id, name, stored_name, mime_type, size_bytes, sha256, kind, state, created_at
         FROM attachments
         WHERE id = ?1",
    ).map_err(|e| AppError::Database(e.to_string()))?;

    let res = stmt.query_row(params![id], map_attachment_row);

    match res {
        Ok(rec) => Ok(Some(rec)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(AppError::Database(e.to_string())),
    }
}

/// Retrieves all attachments associated with a message in deterministic creation order.
pub fn get_attachments_for_message(db: &Database, message_id: &str) -> Result<Vec<AttachmentRecord>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, conversation_id, message_id, name, stored_name, mime_type, size_bytes, sha256, kind, state, created_at
         FROM attachments
         WHERE message_id = ?1
         ORDER BY rowid ASC",
    ).map_err(|e| AppError::Database(e.to_string()))?;

    let iter = stmt.query_map(params![message_id], map_attachment_row)
        .map_err(|e| AppError::Database(e.to_string()))?;

    let mut result = Vec::new();
    for r in iter {
        result.push(r.map_err(|e| AppError::Database(e.to_string()))?);
    }
    Ok(result)
}

/// Retrieves all attachments for a conversation.
pub fn get_attachments_for_conversation(db: &Database, conversation_id: &str) -> Result<Vec<AttachmentRecord>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, conversation_id, message_id, name, stored_name, mime_type, size_bytes, sha256, kind, state, created_at
         FROM attachments
         WHERE conversation_id = ?1
         ORDER BY rowid ASC",
    ).map_err(|e| AppError::Database(e.to_string()))?;

    let iter = stmt.query_map(params![conversation_id], map_attachment_row)
        .map_err(|e| AppError::Database(e.to_string()))?;

    let mut result = Vec::new();
    for r in iter {
        result.push(r.map_err(|e| AppError::Database(e.to_string()))?);
    }
    Ok(result)
}

/// Retrieves all staged (uncommitted) attachments for a conversation.
pub fn get_staged_attachments(db: &Database, conversation_id: &str) -> Result<Vec<AttachmentRecord>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, conversation_id, message_id, name, stored_name, mime_type, size_bytes, sha256, kind, state, created_at
         FROM attachments
         WHERE conversation_id = ?1 AND state = 'staged'
         ORDER BY rowid ASC",
    ).map_err(|e| AppError::Database(e.to_string()))?;

    let iter = stmt.query_map(params![conversation_id], map_attachment_row)
        .map_err(|e| AppError::Database(e.to_string()))?;

    let mut result = Vec::new();
    for r in iter {
        result.push(r.map_err(|e| AppError::Database(e.to_string()))?);
    }
    Ok(result)
}

/// Deletes an attachment record by ID.
pub fn delete_attachment_record(db: &Database, id: &str) -> Result<()> {
    let conn = db.conn.lock().unwrap();
    conn.execute("DELETE FROM attachments WHERE id = ?1", params![id])
        .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(())
}
