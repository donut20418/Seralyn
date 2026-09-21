pub mod context;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Mutex};

use crate::app::conversation::context::{
    build_adaptive_context, calculate_input_budget, resolve_model_context_window,
};
use crate::app::tokens::TokenManager;
pub use crate::app::db::conversations::{Conversation, ConversationSummary};
use crate::app::db::conversations;
pub use crate::app::db::messages::Message;
use crate::app::db::messages;
pub use crate::app::db::provider_sessions::ProviderSessionRecord;
use crate::app::db::provider_sessions;
pub use crate::app::db::usage_snapshots::UsageSnapshotRecord;
use crate::app::db::usage_snapshots;
use crate::app::db::Database;
use crate::app::error::Result;
use crate::app::events::{EventPayload, EventType, NormalizedEvent, ProviderKind};
use crate::app::providers::{
    PermissionMode, ProviderManager, ProviderMessage, ProviderSession, ProviderStatus, SessionConfig,
};

/// Attachment info returned when saving attachments
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentInfo {
    pub id: String,
    pub name: String,
    pub path: String,
    pub size: u64,
    pub mime_type: String,
}

/// Conversation with full history and active provider sessions (used by frontend / Tauri IPC)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationWithMessages {
    pub conversation: Conversation,
    pub messages: Vec<Message>,
    pub provider_sessions: Vec<ProviderSessionRecord>,
}

/// Key for tracking active provider sessions per conversation, provider, profile/account, and model
type SessionKey = (String, ProviderKind, String, String);

#[derive(Clone)]
pub struct ActiveSessionEntry {
    pub session: Arc<dyn ProviderSession>,
    pub db_session_id: String,
    pub event_tx: Arc<tokio::sync::RwLock<Option<mpsc::Sender<NormalizedEvent>>>>,
    pub turn_input_tokens: Arc<AtomicU64>,
}

pub struct ConversationManager {
    db: Arc<Database>,
    provider_manager: Arc<ProviderManager>,
    active_sessions: Arc<Mutex<HashMap<SessionKey, ActiveSessionEntry>>>,
}

impl ConversationManager {
    pub fn new(db: Arc<Database>, provider_manager: Arc<ProviderManager>) -> Self {
        Self {
            db,
            provider_manager,
            active_sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn create_conversation(&self, title: Option<&str>) -> Result<Conversation> {
        conversations::create_conversation(&self.db, title)
    }

    pub fn get_conversation(&self, id: &str) -> Result<ConversationWithMessages> {
        let conversation = conversations::get_conversation(&self.db, id)?;
        let messages = messages::get_messages(&self.db, id)?;
        let provider_sessions = provider_sessions::get_sessions_for_conversation(&self.db, id)?;
        Ok(ConversationWithMessages {
            conversation,
            messages,
            provider_sessions,
        })
    }

    pub fn list_conversations(&self) -> Result<Vec<ConversationSummary>> {
        conversations::list_conversations(&self.db)
    }

    pub fn get_messages(&self, conversation_id: &str) -> Result<Vec<Message>> {
        messages::get_messages(&self.db, conversation_id)
    }

    pub async fn delete_conversation(&self, id: &str) -> Result<()> {
        let entries: Vec<ActiveSessionEntry> = {
            let mut sessions = self.active_sessions.lock().await;
            let keys: Vec<SessionKey> = sessions
                .keys()
                .filter(|(c_id, _, _, _)| c_id == id)
                .cloned()
                .collect();
            keys.into_iter().filter_map(|k| sessions.remove(&k)).collect()
        };
            
        for entry in entries {
            let _ = entry.session.close().await;
        }
        
        let res = conversations::delete_conversation(&self.db, id)?;

        if uuid::Uuid::parse_str(id).is_ok() {
            let mut root_path = dirs::data_local_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
            root_path.push("Seralyn");
            root_path.push("attachments");
            if let Ok(canon_root) = root_path.canonicalize() {
                let conv_dir = canon_root.join(id);
                if conv_dir.exists() {
                    if let Ok(canon_conv_dir) = conv_dir.canonicalize() {
                        if canon_conv_dir.starts_with(&canon_root) && canon_conv_dir.parent() == Some(canon_root.as_path()) {
                            let _ = std::fs::remove_dir_all(&canon_conv_dir);
                        }
                    }
                }
            }
        }

        Ok(res)
    }

    pub fn archive_conversation(&self, id: &str) -> Result<()> {
        conversations::archive_conversation(&self.db, id)
    }

    pub async fn send_message(
        &self,
        conversation_id: &str,
        content: &str,
        provider_kind: ProviderKind,
        event_sender: mpsc::Sender<NormalizedEvent>,
    ) -> Result<()> {
        self.send_message_with_attachments(
            conversation_id,
            content,
            provider_kind,
            vec![],
            None,
            None,
            None,
            event_sender,
        )
        .await
    }

    pub async fn send_message_with_attachments(
        &self,
        conversation_id: &str,
        content: &str,
        provider_kind: ProviderKind,
        attachments: Vec<AttachmentInfo>,
        model: Option<String>,
        account: Option<String>,
        effort: Option<String>,
        event_sender: mpsc::Sender<NormalizedEvent>,
    ) -> Result<()> {
        // 1. Build provider_prompt with attachment header and validate prompt budget BEFORE writing to DB
        let provider_prompt = if !attachments.is_empty() {
            let attachment_header = attachments
                .iter()
                .map(|a| format!("[Attached File: {} ({}, {:.1} KB)]", a.path, a.name, a.size as f64 / 1024.0))
                .collect::<Vec<_>>()
                .join("\n");
            format!("{}\n\n{}", attachment_header, content)
        } else {
            content.to_string()
        };

        let context_window = resolve_model_context_window(provider_kind.clone(), model.as_deref());
        let base_input_budget = calculate_input_budget(context_window);
        let prompt_tokens = TokenManager::estimate_tokens(&provider_prompt);

        if prompt_tokens > base_input_budget {
            return Err(crate::app::error::AppError::InvalidInput(format!(
                "Prompt size ({} tokens) exceeds model's maximum input budget ({} tokens)",
                prompt_tokens, base_input_budget
            )));
        }

        // 2. Save user message to DB with attachments in metadata -> returns user_msg with assigned sequence number
        let user_metadata = if !attachments.is_empty() {
            Some(serde_json::to_string(&serde_json::json!({ "attachments": attachments }))?)
        } else {
            None
        };

        let user_msg = messages::create_message_with_metadata(
            &self.db,
            conversation_id,
            None,
            "user",
            content,
            None,
            model.as_deref(),
            None,
            None,
            user_metadata.as_deref(),
        )?;

        // Auto-title conversation if title is unset or default
        if let Ok(conv) = conversations::get_conversation(&self.db, conversation_id) {
            let needs_title = conv.title.as_ref().map(|t| t.trim().is_empty() || t == "New Conversation").unwrap_or(true);
            if needs_title {
                let first_line = content.lines().next().unwrap_or("").trim();
                let clean_title: String = first_line.chars().take(40).collect();
                if !clean_title.is_empty() {
                    let _ = conversations::update_conversation_title(&self.db, conversation_id, &clean_title);
                }
            }
        }

        let provider = self.provider_manager.get(provider_kind.clone())?;
        let provider_str = provider_kind.to_string();
        
        let profile_id = account.clone().unwrap_or_else(|| "default".to_string());
        let model_id = model.clone().unwrap_or_else(|| "default".to_string());
        let session_key = (conversation_id.to_string(), provider_kind.clone(), profile_id.clone(), model_id.clone());
        
        // 3. Lookup existing provider session record in SQLite for this exact (account, model)
        let mut existing_record = provider_sessions::get_active_session_for_account_model(
            &self.db,
            conversation_id,
            &provider_str,
            Some(&profile_id),
            model.as_deref(),
        )?;

        let mut existing_entry = {
            let sessions = self.active_sessions.lock().await;
            sessions.get(&session_key).cloned()
        };

        let existing_db_id = existing_entry
            .as_ref()
            .map(|e| e.db_session_id.clone())
            .or_else(|| existing_record.as_ref().map(|r| r.id.clone()));

        let synced_through_seq = existing_record.as_ref().map(|r| r.synced_through_seq).unwrap_or(0);

        let existing_native_tokens: Option<u64> = if let Some(ref db_id) = existing_db_id {
            let snapshot_tokens = usage_snapshots::get_latest_usage_snapshot_for_session(&self.db, conversation_id, db_id)
                .ok()
                .flatten()
                .and_then(|s| s.context_tokens)
                .map(|v| v as u64);

            if snapshot_tokens.is_some() {
                snapshot_tokens
            } else if synced_through_seq > 0 {
                // Conservative fallback when provider lacks native telemetry (e.g. Gemini):
                // Estimate accumulated native context from canonical SQLite history synced through `synced_through_seq`
                let all_messages = messages::get_messages(&self.db, conversation_id).unwrap_or_default();
                let est_tokens: u64 = all_messages
                    .into_iter()
                    .filter(|m| m.seq <= synced_through_seq)
                    .map(|m| TokenManager::estimate_tokens(&m.content))
                    .sum();
                Some(est_tokens)
            } else {
                Some(0)
            }
        } else {
            None
        };

        // Hard Budget Saturation Check:
        // If the current native session cannot accommodate the prompt without exceeding the model's base input budget,
        // transparently roll over: retire the saturated native session and start a fresh session with a compacted handoff.
        let current_native = existing_native_tokens.unwrap_or(0);
        let is_saturated = current_native > 0
            && current_native.saturating_add(prompt_tokens) > base_input_budget;

        if is_saturated {
            tracing::info!(
                conversation_id = %conversation_id,
                provider = %provider_str,
                model = ?model,
                existing_native_tokens = current_native,
                prompt_tokens = prompt_tokens,
                base_input_budget = base_input_budget,
                "Native session saturated; rolling over to fresh session with compacted handoff"
            );

            // Close old in-memory session if active and remove from active_sessions
            if let Some(entry) = existing_entry.take() {
                let _ = entry.session.close().await;
            }
            {
                let mut sessions = self.active_sessions.lock().await;
                sessions.remove(&session_key);
            }

            // Close old session record in SQLite
            if let Some(ref db_id) = existing_db_id {
                let _ = provider_sessions::close_session(&self.db, db_id);
            }

            existing_record = None;
        }

        let synced_through_seq = existing_record.as_ref().map(|r| r.synced_through_seq).unwrap_or(0);
        let effective_existing_native_tokens = if is_saturated { 0 } else { current_native };
        let should_reuse_session = existing_entry.is_some();

        let (session, active_db_session_id, active_turn_input_tokens): (Arc<dyn ProviderSession>, String, Arc<AtomicU64>) = if should_reuse_session {
            let entry = existing_entry.unwrap();
            // Dynamic event routing: update active event sender to the new channel for this turn!
            let mut tx_guard = entry.event_tx.write().await;
            *tx_guard = Some(event_sender.clone());
            (entry.session.clone(), entry.db_session_id.clone(), entry.turn_input_tokens.clone())
        } else {
            // Creation/resumption happens OUTSIDE active_sessions lock!
            let (internal_tx, mut internal_rx) = mpsc::channel(100);
            let turn_input_tokens = Arc::new(AtomicU64::new(0));
            let turn_input_tokens_clone = turn_input_tokens.clone();
            
            let mut env = HashMap::new();
            if let Some(eff) = &effort {
                env.insert("REASONING_EFFORT".to_string(), eff.clone());
            }
            if let Some(acc) = &account {
                env.insert("PROVIDER_ACCOUNT".to_string(), acc.clone());
            }

            let config = SessionConfig {
                conversation_id: conversation_id.to_string(),
                working_dir: None,
                permission_mode: PermissionMode::Safe,
                system_prompt: None,
                model: model.clone(),
                account: account.clone(),
                effort: effort.clone(),
                env,
                event_sender: internal_tx,
            };
            
            let can_resume = if let Some(ref record) = existing_record {
                if record.provider_session_id.is_some() {
                    if let Some(ref req_model) = model {
                        if let Some(ref curr_model) = record.model {
                            curr_model == req_model
                        } else {
                            true
                        }
                    } else {
                        true
                    }
                } else {
                    false
                }
            } else {
                false
            };

            let (sess, provider_session_id): (Arc<dyn ProviderSession>, String) = if can_resume {
                let record = existing_record.as_ref().unwrap();
                let native_id = record.provider_session_id.as_ref().unwrap();
                match provider.resume_session(native_id, config.clone()).await {
                    Ok(sess) => (Arc::from(sess), record.id.clone()),
                    Err(_) => {
                        let sess: Arc<dyn ProviderSession> = Arc::from(provider.create_session(config).await?);
                        let meta_json = serde_json::json!({ "account": profile_id }).to_string();
                        let rec = provider_sessions::create_provider_session_with_metadata(
                            &self.db,
                            conversation_id,
                            &provider_str,
                            sess.native_session_id().as_deref(),
                            model.as_deref().or(sess.metadata().model.as_deref()),
                            Some(&meta_json),
                        )?;
                        (sess, rec.id)
                    }
                }
            } else {
                let sess: Arc<dyn ProviderSession> = Arc::from(provider.create_session(config).await?);
                let meta_json = serde_json::json!({ "account": profile_id }).to_string();
                let rec = provider_sessions::create_provider_session_with_metadata(
                    &self.db,
                    conversation_id,
                    &provider_str,
                    sess.native_session_id().as_deref(),
                    model.as_deref().or(sess.metadata().model.as_deref()),
                    Some(&meta_json),
                )?;
                (sess, rec.id)
            };

            // If session reports a native ID immediately, persist it
            if let Some(nid) = sess.native_session_id() {
                let _ = provider_sessions::update_native_session_id(&self.db, &provider_session_id, &nid);
            }
            
            let event_tx = Arc::new(tokio::sync::RwLock::new(Some(event_sender.clone())));
            let event_tx_clone = event_tx.clone();
            let db_clone = self.db.clone();
            let conv_id = conversation_id.to_string();
            let pk = provider_kind.clone();
            let ps_id = provider_session_id.clone();
            let cw = context_window;
            
            // Dedicated tokio task listening for events from the session lifecycle
            tokio::spawn(async move {
                let mut current_text = String::new();
                let mut current_thinking = String::new();
                let mut saw_usage_this_turn = false;
                
                while let Some(event) = internal_rx.recv().await {
                    // Update native session ID in DB when reported by provider
                    if let EventPayload::Session { session_id: Some(sid), .. } = &event.payload {
                        if !sid.trim().is_empty() {
                            let _ = provider_sessions::update_native_session_id(&db_clone, &ps_id, sid);
                        }
                    } else if let Some(sid) = &event.provider_session_id {
                        if !sid.trim().is_empty() {
                            let _ = provider_sessions::update_native_session_id(&db_clone, &ps_id, sid);
                        }
                    }

                    if let EventPayload::Text { content } = &event.payload {
                        current_text.push_str(content);
                    }

                    if let EventPayload::Thinking { content } = &event.payload {
                        current_thinking.push_str(content);
                    }

                    if let EventPayload::Usage {
                        input_tokens,
                        output_tokens,
                        cache_read_tokens,
                        reasoning_tokens,
                        context_tokens,
                        context_window,
                        confidence,
                    } = &event.payload {
                        saw_usage_this_turn = true;
                        let conf_str = match confidence {
                            crate::app::events::TokenConfidence::Exact => "EXACT",
                            crate::app::events::TokenConfidence::Estimated => "ESTIMATED",
                        };
                        let _ = usage_snapshots::create_usage_snapshot(
                            &db_clone,
                            &conv_id,
                            Some(&ps_id),
                            input_tokens.map(|v| v as i64),
                            output_tokens.map(|v| v as i64),
                            cache_read_tokens.map(|v| v as i64),
                            None,
                            reasoning_tokens.map(|v| v as i64),
                            context_tokens.map(|v| v as i64),
                            context_window.map(|v| v as i64),
                            conf_str,
                        );
                    }

                    if event.event_type == EventType::SessionFinished || event.event_type == EventType::Error {
                        if !current_text.is_empty() || !current_thinking.is_empty() {
                            let metadata_json = if !current_thinking.is_empty() {
                                Some(serde_json::to_string(&serde_json::json!({ "thinking": current_thinking })).unwrap())
                            } else {
                                None
                            };

                            let msg_res = messages::create_message_with_metadata(
                                &db_clone,
                                &conv_id,
                                None,
                                "assistant",
                                &current_text,
                                Some(&pk.to_string()),
                                None,
                                Some(&ps_id),
                                None,
                                metadata_json.as_deref(),
                            );

                            if let Ok(msg) = msg_res {
                                // Sync cursor update: this provider session is now synced through this assistant response!
                                let _ = provider_sessions::update_synced_seq(&db_clone, &ps_id, msg.seq);

                                // If this provider didn't report native telemetry for this turn (e.g. Gemini),
                                // record an estimated usage snapshot based on accumulated native context sent to this session
                                if !saw_usage_this_turn {
                                    let assistant_tokens = TokenManager::estimate_tokens(&current_text);
                                    let input_tokens = turn_input_tokens_clone.load(Ordering::SeqCst);
                                    let total_context = input_tokens.saturating_add(assistant_tokens);

                                    let _ = usage_snapshots::create_usage_snapshot(
                                        &db_clone,
                                        &conv_id,
                                        Some(&ps_id),
                                        Some(input_tokens as i64),
                                        Some(assistant_tokens as i64),
                                        None,
                                        None,
                                        None,
                                        Some(total_context as i64),
                                        Some(cw as i64),
                                        "ESTIMATED",
                                    );
                                }
                            }
                            current_text.clear();
                            current_thinking.clear();
                        }
                        saw_usage_this_turn = false;
                    }
                    
                    // Forward to active caller channel if available; do NOT terminate loop on send error!
                    let active_tx = event_tx_clone.read().await.clone();
                    if let Some(tx) = active_tx {
                        let _ = tx.send(event).await;
                    }
                }
            });
            
            let entry = ActiveSessionEntry {
                session: sess.clone(),
                db_session_id: provider_session_id.clone(),
                event_tx,
                turn_input_tokens: turn_input_tokens.clone(),
            };

            // Register in active_sessions - brief lock only!
            let mut sessions = self.active_sessions.lock().await;
            if let Some(existing) = sessions.get(&session_key) {
                let mut tx_guard = existing.event_tx.write().await;
                *tx_guard = Some(event_sender.clone());
                (existing.session.clone(), existing.db_session_id.clone(), existing.turn_input_tokens.clone())
            } else {
                sessions.insert(session_key.clone(), entry);
                (sess, provider_session_id, turn_input_tokens)
            }
        };

        // 5. Calculate adaptive context delta between sync cursor and current message,
        // factoring in effective existing native session context tokens and current prompt tokens
        let adaptive_ctx = build_adaptive_context(
            &self.db,
            conversation_id,
            synced_through_seq,
            user_msg.seq,
            provider_kind,
            model.as_deref(),
            effective_existing_native_tokens,
            &provider_prompt,
        )?;

        // 7. Create ProviderMessage with content + adaptive context delta + attachments
        let attachment_refs: Vec<crate::app::providers::AttachmentRef> = attachments
            .iter()
            .map(|a| crate::app::providers::AttachmentRef {
                id: a.id.clone(),
                path: std::path::PathBuf::from(&a.path),
                mime_type: a.mime_type.clone(),
            })
            .collect();

        let msg = ProviderMessage {
            content: provider_prompt,
            context: adaptive_ctx.messages,
            attachments: attachment_refs,
        };

        let working_tokens: u64 = msg.context.iter().map(|m| TokenManager::estimate_tokens(&m.content)).sum();
        let accumulated_input = effective_existing_native_tokens
            .saturating_add(working_tokens)
            .saturating_add(prompt_tokens);
        active_turn_input_tokens.store(accumulated_input, Ordering::SeqCst);

        // 8. Call session.send(message) WITHOUT holding active_sessions lock!
        // This completely prevents approval and interrupt deadlocks!
        session.send(msg).await?;
        
        let _ = provider_sessions::update_session_used(&self.db, &active_db_session_id);

        Ok(())
    }

    pub fn switch_provider(&self, _conversation_id: &str, _new_provider: ProviderKind) -> Result<()> {
        // Just updates internal state, actual session creation happens on next send
        Ok(())
    }

    pub async fn respond_to_approval(
        &self,
        conversation_id: &str,
        provider: ProviderKind,
        account: Option<&str>,
        model: Option<&str>,
        approval_id: &str,
        approved: bool,
    ) -> Result<()> {
        let profile_id = account.unwrap_or("default");
        let session = {
            let sessions = self.active_sessions.lock().await;
            let matches: Vec<_> = sessions
                .iter()
                .filter(|((cid, p, acc, m), _)| {
                    cid == conversation_id
                        && *p == provider
                        && (account.is_none() || acc == profile_id)
                        && (model.is_none() || m == model.unwrap_or(""))
                })
                .map(|(_, e)| e.session.clone())
                .collect();

            if model.is_none() && matches.len() > 1 {
                return Err(crate::app::error::AppError::InvalidInput(
                    "Ambiguous approval target: multiple active model sessions exist for this account; specify model".to_string(),
                ));
            }
            matches.into_iter().next()
        };
        if let Some(session) = session {
            session.respond_to_approval(approval_id, approved).await?;
        }
        Ok(())
    }

    pub async fn detect_providers(&self) -> Vec<ProviderStatus> {
        self.provider_manager.detect_all().await
    }

    pub fn update_conversation_title(&self, id: &str, title: &str) -> Result<()> {
        conversations::update_conversation_title(&self.db, id, title)
    }

    pub fn get_conversation_usage(
        &self,
        conversation_id: &str,
        provider: Option<&str>,
        account: Option<&str>,
        model: Option<&str>,
    ) -> Result<Option<UsageSnapshotRecord>> {
        usage_snapshots::get_latest_usage_snapshot(&self.db, conversation_id, provider, account, model)
    }

    pub async fn interrupt_turn(
        &self,
        conversation_id: &str,
        provider: ProviderKind,
        account: Option<&str>,
        model: Option<&str>,
    ) -> Result<()> {
        let profile_id = account.unwrap_or("default");
        let sessions_to_interrupt: Vec<Arc<dyn ProviderSession>> = {
            let sessions = self.active_sessions.lock().await;
            let matches: Vec<_> = sessions
                .iter()
                .filter(|((cid, p, acc, m), _)| {
                    cid == conversation_id
                        && *p == provider
                        && (account.is_none() || acc == profile_id)
                        && (model.is_none() || m == model.unwrap_or(""))
                })
                .map(|(_, e)| e.session.clone())
                .collect();

            if model.is_none() && matches.len() > 1 {
                return Err(crate::app::error::AppError::InvalidInput(
                    "Ambiguous interrupt target: multiple active model sessions exist for this account; specify model".to_string(),
                ));
            }
            matches
        };
        for session in sessions_to_interrupt {
            let _ = session.interrupt().await;
        }
        Ok(())
    }

    pub fn save_attachment(
        &self,
        conversation_id: &str,
        file_name: &str,
        file_data: &[u8],
        mime_type: Option<&str>,
    ) -> Result<AttachmentInfo> {
        // 1. Validate conversation_id format (must be valid UUID)
        uuid::Uuid::parse_str(conversation_id)
            .map_err(|_| crate::app::error::AppError::InvalidInput("Invalid conversation id: must be a valid UUID".to_string()))?;

        // 2. Validate that conversation exists in database
        conversations::get_conversation(&self.db, conversation_id)?;

        // 3. File size limit: 25 MB max
        const MAX_ATTACHMENT_SIZE: usize = 25 * 1024 * 1024;
        if file_data.len() > MAX_ATTACHMENT_SIZE {
            return Err(crate::app::error::AppError::InvalidInput(format!(
                "File size ({} bytes) exceeds maximum limit of 25MB",
                file_data.len()
            )));
        }

        // 4. Filename sanitization: extract leaf and reject control / path separator characters
        let raw_leaf = std::path::Path::new(file_name)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment.bin");

        let sanitized: String = raw_leaf
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '.' || *c == '_' || *c == '-' || *c == ' ')
            .collect();

        let safe_name = if sanitized.trim().is_empty() {
            "attachment.bin"
        } else {
            sanitized.trim()
        };

        // 5. Build and canonicalize attachments root directory first
        let mut root_path = dirs::data_local_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
        root_path.push("Seralyn");
        root_path.push("attachments");
        std::fs::create_dir_all(&root_path)?;
        let canon_root = root_path.canonicalize()?;

        let conv_dir = canon_root.join(conversation_id);
        // Ensure conv_dir is strictly inside canon_root and is a direct child
        if !conv_dir.starts_with(&canon_root) || conv_dir.parent() != Some(canon_root.as_path()) {
            return Err(crate::app::error::AppError::InvalidInput(
                "Path traversal attempt detected in conversation id".to_string(),
            ));
        }

        std::fs::create_dir_all(&conv_dir)?;
        let canon_conv_dir = conv_dir.canonicalize()?;

        // Verify resolved path against symlink / junction escapes
        if !canon_conv_dir.starts_with(&canon_root) || canon_conv_dir.parent() != Some(canon_root.as_path()) {
            return Err(crate::app::error::AppError::InvalidInput(
                "Junction/symlink traversal attempt detected in conversation directory".to_string(),
            ));
        }

        let id = uuid::Uuid::new_v4().to_string();
        let dest_filename = format!("{}_{}", id, safe_name);
        let dest_path = canon_conv_dir.join(&dest_filename);

        // Ensure dest_path is strictly inside canon_conv_dir
        if dest_path.parent() != Some(canon_conv_dir.as_path()) || !dest_path.starts_with(&canon_conv_dir) {
            return Err(crate::app::error::AppError::InvalidInput(
                "Path traversal attempt detected in attachment filename".to_string(),
            ));
        }

        std::fs::write(&dest_path, file_data)?;

        let mime = mime_type.unwrap_or("application/octet-stream").to_string();

        Ok(AttachmentInfo {
            id,
            name: safe_name.to_string(),
            path: dest_path.to_string_lossy().to_string(),
            size: file_data.len() as u64,
            mime_type: mime,
        })
    }

    pub fn delete_attachment(
        &self,
        conversation_id: &str,
        attachment_id: &str,
    ) -> Result<()> {
        uuid::Uuid::parse_str(conversation_id)
            .map_err(|_| crate::app::error::AppError::InvalidInput("Invalid conversation id".to_string()))?;
        uuid::Uuid::parse_str(attachment_id)
            .map_err(|_| crate::app::error::AppError::InvalidInput("Invalid attachment id".to_string()))?;

        let mut root_path = dirs::data_local_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
        root_path.push("Seralyn");
        root_path.push("attachments");
        if !root_path.exists() {
            return Ok(());
        }
        let canon_root = root_path.canonicalize()?;
        let conv_dir = canon_root.join(conversation_id);

        if conv_dir.exists() {
            let canon_dir = conv_dir.canonicalize()?;
            if !canon_dir.starts_with(&canon_root) || canon_dir.parent() != Some(canon_root.as_path()) {
                return Err(crate::app::error::AppError::InvalidInput(
                    "Junction/symlink traversal detected in attachment deletion".to_string(),
                ));
            }
            if let Ok(entries) = std::fs::read_dir(&canon_dir) {
                for entry in entries.flatten() {
                    let file_name = entry.file_name().to_string_lossy().to_string();
                    if file_name.starts_with(&format!("{}_", attachment_id)) {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn close_all_sessions(&self) -> Result<()> {
        let sessions_to_close: Vec<Arc<dyn ProviderSession>> = {
            let mut sessions = self.active_sessions.lock().await;
            sessions.drain().map(|(_, e)| e.session).collect()
        };
        for session in sessions_to_close {
            let _ = session.close().await;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_save_attachment_security_and_lifecycle() {
        let db = Arc::new(Database::new_in_memory().unwrap());
        db.run_migrations().unwrap();
        let provider_manager = Arc::new(crate::app::providers::ProviderManager::with_providers(HashMap::new()));
        let manager = ConversationManager::new(db.clone(), provider_manager);

        let conv = conversations::create_conversation(&db, Some("Attachment Security Test")).unwrap();

        // 1. Path traversal in conversation_id -> REJECT
        let err1 = manager.save_attachment("../../outside", "test.txt", b"content", None);
        assert!(err1.is_err(), "Must reject traversal in conversation_id");

        // 2. Non-existent conversation -> REJECT
        let fake_id = uuid::Uuid::new_v4().to_string();
        let err2 = manager.save_attachment(&fake_id, "test.txt", b"content", None);
        assert!(err2.is_err(), "Must reject non-existent conversation");

        // 3. File size limit > 25MB -> REJECT
        let large_data = vec![0u8; 26 * 1024 * 1024];
        let err3 = manager.save_attachment(&conv.id, "large.bin", &large_data, None);
        assert!(err3.is_err(), "Must reject file larger than 25MB");

        // 4. Traversal in filename -> leaf extracted & safe
        let att_traversal = manager.save_attachment(&conv.id, "../../evil.txt", b"safe content", None).unwrap();
        assert_eq!(att_traversal.name, "evil.txt");
        assert!(std::path::Path::new(&att_traversal.path).exists());
        // Clean up via delete_attachment
        manager.delete_attachment(&conv.id, &att_traversal.id).unwrap();
        assert!(!std::path::Path::new(&att_traversal.path).exists());

        // 5. Valid attachment save, delete attachment, and delete conversation cleanup
        let att = manager.save_attachment(&conv.id, "report.pdf", b"%PDF-1.4 test report", Some("application/pdf")).unwrap();
        let att_path = std::path::PathBuf::from(&att.path);
        assert!(att_path.exists());
        let parent_dir = att_path.parent().unwrap().to_path_buf();
        assert!(parent_dir.exists());

        // Delete conversation cleans up the attachment directory from disk
        manager.delete_conversation(&conv.id).await.unwrap();
        assert!(!att_path.exists());
        assert!(!parent_dir.exists());
    }
}
