use rusqlite::params;

use crate::app::db::Database;
use crate::app::db::messages::get_recent_messages;
use crate::app::error::{AppError, Result};
use crate::app::providers::ContextMessage;
use crate::app::tokens::TokenManager;

pub fn build_context(db: &Database, conversation_id: &str, max_messages: usize) -> Result<Vec<ContextMessage>> {
    let recent = get_recent_messages(db, conversation_id, max_messages)?;
    
    let mut context = Vec::new();
    for msg in recent {
        if msg.role == "system" || msg.role == "tool" {
            continue;
        }
        
        context.push(ContextMessage {
            role: msg.role,
            content: msg.content,
            provider: msg.provider,
        });
    }
    
    Ok(context)
}

/// Builds the context delta between a provider session's sync cursor (`after_seq`)
/// and the current user message (`before_seq`).
/// Only messages in range (after_seq, before_seq) are returned.
pub fn build_context_delta(
    db: &Database,
    conversation_id: &str,
    after_seq: i64,
    before_seq: i64,
) -> Result<Vec<ContextMessage>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT role, content, provider
         FROM messages
         WHERE conversation_id = ?1 AND seq > ?2 AND seq < ?3
         ORDER BY seq ASC"
    ).map_err(|e| AppError::Database(e.to_string()))?;

    let iter = stmt.query_map(params![conversation_id, after_seq, before_seq], |row| {
        Ok(ContextMessage {
            role: row.get(0)?,
            content: row.get(1)?,
            provider: row.get(2)?,
        })
    }).map_err(|e| AppError::Database(e.to_string()))?;

    let mut context = Vec::new();
    for row in iter {
        let msg = row.map_err(|e| AppError::Database(e.to_string()))?;
        if msg.role == "system" || msg.role == "tool" {
            continue;
        }
        context.push(msg);
    }

    Ok(context)
}

pub fn estimate_context_tokens(context: &[ContextMessage], _token_manager: &TokenManager) -> u64 {
    context.iter().map(|msg| TokenManager::estimate_tokens(&msg.content)).sum()
}

/// Formats prior cross-provider conversation history and current prompt into a unified message.
pub fn format_context_for_prompt(context: &[ContextMessage], current_prompt: &str) -> String {
    if context.is_empty() {
        return current_prompt.to_string();
    }

    let mut out = String::from("[Prior Conversation History Across Providers]\n");
    for msg in context {
        let label = match (msg.role.as_str(), msg.provider.as_deref()) {
            ("user", _) => "User".to_string(),
            ("assistant", Some(p)) => format!("Assistant ({p})"),
            ("assistant", None) => "Assistant".to_string(),
            (other, _) => other.to_string(),
        };
        out.push_str(&format!("{label}: {}\n\n", msg.content.trim()));
    }
    out.push_str("[End of Prior History]\n\n");
    out.push_str(current_prompt);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::db::Database;
    use crate::app::db::conversations::create_conversation;
    use crate::app::db::messages::create_message;

    #[test]
    fn test_build_context() {
        let db = Database::new_in_memory().unwrap();
        db.run_migrations().unwrap();
        
        let conv = create_conversation(&db, None).unwrap();
        create_message(&db, &conv.id, None, "user", "Hello", None, None, None, None).unwrap();
        
        let ctx = build_context(&db, &conv.id, 10).unwrap();
        assert_eq!(ctx.len(), 1);
        assert_eq!(ctx[0].role, "user");
        assert_eq!(ctx[0].content, "Hello");
    }

    #[test]
    fn test_build_context_delta() {
        let db = Database::new_in_memory().unwrap();
        db.run_migrations().unwrap();

        let conv = create_conversation(&db, None).unwrap();
        // seq 1
        create_message(&db, &conv.id, None, "user", "Turn 1", None, None, None, None).unwrap();
        // seq 2
        create_message(&db, &conv.id, None, "assistant", "Answer 1", Some("claude"), None, None, None).unwrap();
        // seq 3
        create_message(&db, &conv.id, None, "user", "Turn 2", None, None, None, None).unwrap();
        // seq 4
        create_message(&db, &conv.id, None, "assistant", "Answer 2", Some("codex"), None, None, None).unwrap();
        // seq 5
        create_message(&db, &conv.id, None, "user", "Turn 3", None, None, None, None).unwrap();

        // If provider synced up to seq 2 (e.g. Claude), and current turn is seq 5:
        // delta should contain seq 3 (user) and seq 4 (assistant codex)
        let delta = build_context_delta(&db, &conv.id, 2, 5).unwrap();
        assert_eq!(delta.len(), 2);
        assert_eq!(delta[0].content, "Turn 2");
        assert_eq!(delta[1].content, "Answer 2");
        assert_eq!(delta[1].provider.as_deref(), Some("codex"));

        // If provider synced up to seq 4, and current turn is seq 5: delta is empty
        let delta_none = build_context_delta(&db, &conv.id, 4, 5).unwrap();
        assert!(delta_none.is_empty());
    }
    
    #[test]
    fn test_estimate_context_tokens() {
        let msg1 = ContextMessage { role: "user".into(), content: "hello".into(), provider: None };
        let msg2 = ContextMessage { role: "assistant".into(), content: "world".into(), provider: None };
        let tokens = estimate_context_tokens(&[msg1, msg2], &TokenManager::new());
        assert!(tokens > 0);
    }
}
