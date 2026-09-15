use crate::app::db::Database;
use crate::app::db::messages::get_recent_messages;
use crate::app::error::Result;
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

pub fn estimate_context_tokens(context: &[ContextMessage], _token_manager: &TokenManager) -> u64 {
    context.iter().map(|msg| TokenManager::estimate_tokens(&msg.content)).sum()
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
    fn test_estimate_context_tokens() {
        let msg1 = ContextMessage { role: "user".into(), content: "hello".into(), provider: None };
        let msg2 = ContextMessage { role: "assistant".into(), content: "world".into(), provider: None };
        let tokens = estimate_context_tokens(&[msg1, msg2], &TokenManager::new());
        assert!(tokens > 0);
    }
}
