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
        
        context.push(ContextMessage::new(msg.role, msg.content, msg.provider));
    }
    
    Ok(context)
}

/// Builds the context delta between a provider session's sync cursor (`after_seq`)
/// and the current user message (`before_seq`).
/// Only messages in range (after_seq, before_seq) are returned, including their canonical attachments.
pub fn build_context_delta(
    db: &Database,
    conversation_id: &str,
    after_seq: i64,
    before_seq: i64,
) -> Result<Vec<ContextMessage>> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, role, content, provider
         FROM messages
         WHERE conversation_id = ?1 AND seq > ?2 AND seq < ?3
         ORDER BY seq ASC"
    ).map_err(|e| AppError::Database(e.to_string()))?;

    let iter = stmt.query_map(params![conversation_id, after_seq, before_seq], |row| {
        let msg_id: String = row.get(0)?;
        let role: String = row.get(1)?;
        let content: String = row.get(2)?;
        let provider: Option<String> = row.get(3)?;
        Ok((msg_id, role, content, provider))
    }).map_err(|e| AppError::Database(e.to_string()))?;

    let mut raw_rows = Vec::new();
    for row in iter {
        raw_rows.push(row.map_err(|e| AppError::Database(e.to_string()))?);
    }

    // Query canonical attachments associated with messages in this conversation
    let mut att_stmt = conn.prepare(
        "SELECT id, message_id, name, mime_type, size_bytes, sha256, kind
         FROM attachments
         WHERE conversation_id = ?1 AND state = 'attached' AND message_id IS NOT NULL
         ORDER BY rowid ASC"
    ).map_err(|e| AppError::Database(e.to_string()))?;

    let att_iter = att_stmt.query_map(params![conversation_id], |row| {
        let id: String = row.get(0)?;
        let message_id: String = row.get(1)?;
        let name: String = row.get(2)?;
        let mime_type: String = row.get(3)?;
        let size_raw: i64 = row.get(4)?;
        let sha256: String = row.get(5)?;
        let kind_str: String = row.get(6)?;
        let kind = kind_str.parse::<crate::app::attachments::AttachmentKind>()
            .unwrap_or(crate::app::attachments::AttachmentKind::Binary);
        Ok((message_id, crate::app::providers::AttachmentDescriptor {
            id,
            name,
            mime_type,
            size_bytes: size_raw as u64,
            sha256,
            kind,
        }))
    }).map_err(|e| AppError::Database(e.to_string()))?;

    let mut att_map: std::collections::HashMap<String, Vec<crate::app::providers::AttachmentDescriptor>> = std::collections::HashMap::new();
    for r in att_iter {
        if let Ok((mid, desc)) = r {
            att_map.entry(mid).or_default().push(desc);
        }
    }

    let mut context = Vec::new();
    for (msg_id, role, content, provider) in raw_rows {
        if role == "system" || role == "tool" {
            continue;
        }
        let attachments = att_map.remove(&msg_id).unwrap_or_default();
        context.push(ContextMessage {
            role,
            content,
            provider,
            attachments,
        });
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

pub fn calculate_available_input_budget(
    context_window: u64,
    existing_native_tokens: u64,
    current_prompt_tokens: u64,
) -> u64 {
    let base_budget = calculate_input_budget(context_window);
    base_budget
        .saturating_sub(existing_native_tokens)
        .saturating_sub(current_prompt_tokens)
}

pub fn safe_truncate_chars(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

pub fn safe_suffix_chars(s: &str, max_chars: usize) -> String {
    let total_chars = s.chars().count();
    if total_chars <= max_chars {
        s.to_string()
    } else {
        s.chars().skip(total_chars - max_chars).collect()
    }
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
        let char_count = trimmed.chars().count();
        let condensed = if char_count > 600 {
            let prefix = safe_truncate_chars(trimmed, 300);
            let suffix = safe_suffix_chars(trimmed, 250);
            format!("{} ... [truncated for handoff] ... {}", prefix.trim(), suffix.trim())
        } else {
            trimmed.to_string()
        };

        let att_descriptors = if !msg.attachments.is_empty() {
            let parts: Vec<String> = msg.attachments
                .iter()
                .map(|a| {
                    let hash_prefix = if a.sha256.len() >= 8 { &a.sha256[..8] } else { &a.sha256 };
                    format!("[Attachment: {} | {} | sha256:{}]", a.name, a.mime_type, hash_prefix)
                })
                .collect();
            format!(" {}", parts.join(" "))
        } else {
            String::new()
        };

        summary.push_str(&format!("- Turn {}: [{}] {}{}\n", i + 1, role_label, condensed, att_descriptors));
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
    existing_native_tokens: u64,
    current_prompt: &str,
) -> Result<AdaptiveContextResult> {
    let raw_delta = build_context_delta(db, conversation_id, after_seq, before_seq)?;
    let context_window = resolve_model_context_window(provider, model);
    let tm = TokenManager::new();
    let current_prompt_tokens = TokenManager::estimate_tokens(current_prompt);
    let input_budget = calculate_available_input_budget(context_window, existing_native_tokens, current_prompt_tokens);
    let canonical_tokens = estimate_context_tokens(&raw_delta, &tm);

    if input_budget == 0 {
        return Ok(AdaptiveContextResult {
            messages: Vec::new(),
            compacted: !raw_delta.is_empty(),
            canonical_tokens,
            working_tokens: 0,
            context_window,
            input_budget: 0,
        });
    }

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
    // Allocate up to ~60% of available input budget to recent raw messages
    let recent_quota = (input_budget * 6) / 10;
    let mut recent_messages = Vec::new();
    let mut recent_tokens: u64 = 0;
    let mut split_idx = raw_delta.len();

    for (idx, msg) in raw_delta.iter().enumerate().rev() {
        let mut msg_content = msg.content.clone();
        let mut msg_tokens = TokenManager::estimate_tokens(&msg_content);

        // Edge case protection: if a single message exceeds recent_quota, safely truncate its content
        if msg_tokens > recent_quota && recent_messages.is_empty() {
            let mut target_chars = msg_content.chars().count();
            while msg_tokens > recent_quota && target_chars > 0 {
                let step = (msg_tokens.saturating_sub(recent_quota)).max(1) as usize;
                target_chars = target_chars.saturating_sub(step);
                msg_content = safe_truncate_chars(&msg.content, target_chars);
                msg_tokens = TokenManager::estimate_tokens(&msg_content);
            }
        }

        if recent_tokens + msg_tokens <= recent_quota {
            recent_tokens += msg_tokens;
            recent_messages.push(ContextMessage {
                role: msg.role.clone(),
                content: msg_content,
                provider: msg.provider.clone(),
                attachments: msg.attachments.clone(),
            });
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
        "[Context Hand-off: Prior ~{} tokens compacted from canonical history to fit {} input budget ({} tokens)]\n\n### Condensed Prior Context:\n{}\n[End of Compacted Summary — Following messages are recent raw turns]",
        older_tokens, model_name, input_budget, summary_digest
    );

    let mut result_messages = Vec::new();
    result_messages.push(ContextMessage {
        role: "assistant".to_string(),
        content: handoff_content,
        provider: Some("handoff-manager".to_string()),
        attachments: Vec::new(),
    });
    result_messages.extend(recent_messages);

    let mut working_tokens = estimate_context_tokens(&result_messages, &tm);

    // Hard budget guarantee: shrink recent messages if summary + recent still exceeds input_budget
    while working_tokens > input_budget && result_messages.len() > 1 {
        result_messages.remove(1);
        working_tokens = estimate_context_tokens(&result_messages, &tm);
    }

    // If summary header alone exceeds budget, iteratively truncate it to fit
    if working_tokens > input_budget && !result_messages.is_empty() {
        while working_tokens > input_budget && !result_messages[0].content.is_empty() {
            let current_chars = result_messages[0].content.chars().count();
            let step = (working_tokens.saturating_sub(input_budget)).max(1) as usize;
            let target_chars = current_chars.saturating_sub(step);
            result_messages[0].content = safe_truncate_chars(&result_messages[0].content, target_chars);
            working_tokens = estimate_context_tokens(&result_messages, &tm);
        }
        if working_tokens > input_budget {
            result_messages.clear();
            working_tokens = 0;
        }
    }

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
        let mut msg_text = msg.content.trim().to_string();
        if !msg.attachments.is_empty() {
            let att_strs: Vec<String> = msg.attachments.iter()
                .map(|a| {
                    let hash_prefix = if a.sha256.len() >= 8 { &a.sha256[..8] } else { &a.sha256 };
                    format!("[Attachment: {} | {} | sha256:{}]", a.name, a.mime_type, hash_prefix)
                })
                .collect();
            if !msg_text.is_empty() {
                msg_text.push('\n');
            }
            msg_text.push_str(&att_strs.join("\n"));
        }
        out.push_str(&format!("{label}: {}\n\n", msg_text));
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
        let msg1 = ContextMessage::new("user", "hello", None);
        let msg2 = ContextMessage::new("assistant", "world", None);
        let tokens = estimate_context_tokens(&[msg1, msg2], &TokenManager::new());
        assert!(tokens > 0);
    }
}
