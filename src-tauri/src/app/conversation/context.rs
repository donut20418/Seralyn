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

use crate::app::events::ProviderKind;

#[derive(Debug, Clone)]
pub struct AdaptiveContextResult {
    pub messages: Vec<ContextMessage>,
    pub compacted: bool,
    pub canonical_tokens: u64,
    pub working_tokens: u64,
    pub context_window: u64,
    pub input_budget: u64,
}

pub fn resolve_model_context_window(provider: ProviderKind, model: Option<&str>) -> u64 {
    let m = model.unwrap_or("").to_lowercase();
    match provider {
        ProviderKind::Claude => {
            if m.contains("haiku") {
                200_000
            } else {
                // opus, sonnet, or default
                1_000_000
            }
        }
        ProviderKind::Codex => {
            if m.contains("4.5") || m.contains("4o") {
                128_000
            } else {
                // o3, o3-mini, o1, or default
                200_000
            }
        }
        ProviderKind::Gemini => {
            // gemini-2.5, 3.1, 2.0 all support 1M window
            1_000_000
        }
    }
}

pub fn calculate_input_budget(context_window: u64) -> u64 {
    let output_reserve = ((context_window as f64) * 0.15).round() as u64;
    let clamped_output_reserve = output_reserve.clamp(16_000, 64_000);
    let overhead = 10_000;
    let safety_margin = 10_000;
    let total_reserve = clamped_output_reserve + overhead + safety_margin;
    context_window.saturating_sub(total_reserve)
}

pub fn compact_older_messages(older: &[ContextMessage]) -> String {
    let mut summary = String::new();
    for (i, msg) in older.iter().enumerate() {
        let role_label = match (msg.role.as_str(), msg.provider.as_deref()) {
            ("user", _) => "User",
            ("assistant", Some(p)) => p,
            ("assistant", None) => "Assistant",
            (other, _) => other,
        };

        let trimmed = msg.content.trim();
        let condensed = if trimmed.len() > 600 {
            let prefix = &trimmed[..300];
            let suffix = &trimmed[trimmed.len() - 250..];
            format!("{} ... [truncated for handoff] ... {}", prefix.trim(), suffix.trim())
        } else {
            trimmed.to_string()
        };

        summary.push_str(&format!("- Turn {}: [{}] {}\n", i + 1, role_label, condensed));
    }
    summary
}

pub fn build_adaptive_context(
    db: &Database,
    conversation_id: &str,
    after_seq: i64,
    before_seq: i64,
    provider: ProviderKind,
    model: Option<&str>,
) -> Result<AdaptiveContextResult> {
    let raw_delta = build_context_delta(db, conversation_id, after_seq, before_seq)?;
    let context_window = resolve_model_context_window(provider, model);
    let input_budget = calculate_input_budget(context_window);
    let tm = TokenManager::new();
    let canonical_tokens = estimate_context_tokens(&raw_delta, &tm);

    if canonical_tokens <= input_budget {
        return Ok(AdaptiveContextResult {
            messages: raw_delta,
            compacted: false,
            canonical_tokens,
            working_tokens: canonical_tokens,
            context_window,
            input_budget,
        });
    }

    // Compaction required:
    // Allocate up to ~60% of input budget to recent raw messages
    let recent_quota = (input_budget * 6) / 10;
    let mut recent_messages = Vec::new();
    let mut recent_tokens: u64 = 0;
    let mut split_idx = raw_delta.len();

    for (idx, msg) in raw_delta.iter().enumerate().rev() {
        let msg_tokens = TokenManager::estimate_tokens(&msg.content);
        if recent_tokens + msg_tokens <= recent_quota || recent_messages.is_empty() {
            recent_tokens += msg_tokens;
            recent_messages.push(msg.clone());
            split_idx = idx;
        } else {
            break;
        }
    }
    recent_messages.reverse();

    let older_messages = &raw_delta[..split_idx];
    let older_tokens = estimate_context_tokens(older_messages, &tm);
    let summary_digest = compact_older_messages(older_messages);

    let model_name = model.unwrap_or("target model");
    let handoff_content = format!(
        "[Context Hand-off: Prior ~{} tokens compacted from canonical history to fit {} input budget ({} tokens)]\n\n### Summary of Previous Context & Key Decisions:\n{}\n[End of Compacted Summary — Following messages are recent raw turns]",
        older_tokens, model_name, input_budget, summary_digest
    );

    let mut result_messages = Vec::new();
    result_messages.push(ContextMessage {
        role: "assistant".to_string(),
        content: handoff_content,
        provider: Some("handoff-manager".to_string()),
    });
    result_messages.extend(recent_messages);

    let working_tokens = estimate_context_tokens(&result_messages, &tm);

    Ok(AdaptiveContextResult {
        messages: result_messages,
        compacted: true,
        canonical_tokens,
        working_tokens,
        context_window,
        input_budget,
    })
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
