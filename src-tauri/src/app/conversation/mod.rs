pub mod context;

use std::collections::HashMap;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Mutex};

use crate::app::conversation::context::build_context_delta;
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

/// Key for tracking active provider sessions per conversation and provider
type SessionKey = (String, ProviderKind);

#[derive(Clone)]
pub struct ActiveSessionEntry {
    pub session: Arc<dyn ProviderSession>,
    pub event_tx: Arc<tokio::sync::RwLock<Option<mpsc::Sender<NormalizedEvent>>>>,
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
                .filter(|(c_id, _)| c_id == id)
                .cloned()
                .collect();
            keys.into_iter().filter_map(|k| sessions.remove(&k)).collect()
        };
            
        for entry in entries {
            let _ = entry.session.close().await;
        }
        
        conversations::delete_conversation(&self.db, id)
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
        event_sender: mpsc::Sender<NormalizedEvent>,
    ) -> Result<()> {
        // 1. Save user message to DB with attachments in metadata -> returns user_msg with assigned sequence number
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
            None,
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
        
        let session_key = (conversation_id.to_string(), provider_kind.clone());
        
        // 2. Lookup existing provider session record in SQLite to determine sync cursor
        let existing_record = provider_sessions::get_active_session(&self.db, conversation_id, &provider_str)?;
        let synced_through_seq = existing_record.as_ref().map(|r| r.synced_through_seq).unwrap_or(0);

        // 3. Get existing session or create outside active_sessions lock
        let existing_entry = {
            let sessions = self.active_sessions.lock().await;
            sessions.get(&session_key).cloned()
        };

        let session: Arc<dyn ProviderSession> = if let Some(entry) = existing_entry {
            // Dynamic event routing: update active event sender to the new channel for this turn!
            let mut tx_guard = entry.event_tx.write().await;
            *tx_guard = Some(event_sender.clone());
            entry.session.clone()
        } else {
            // Creation/resumption happens OUTSIDE active_sessions lock!
            let (internal_tx, mut internal_rx) = mpsc::channel(100);
            
            let config = SessionConfig {
                conversation_id: conversation_id.to_string(),
                working_dir: None,
                permission_mode: PermissionMode::Safe,
                system_prompt: None,
                model: None,
                env: HashMap::new(),
                event_sender: internal_tx,
            };
            
            let (sess, provider_session_id): (Arc<dyn ProviderSession>, String) = match &existing_record {
                Some(record) if record.provider_session_id.is_some() => {
                    let native_id = record.provider_session_id.as_ref().unwrap();
                    match provider.resume_session(native_id, config.clone()).await {
                        Ok(sess) => (Arc::from(sess), record.id.clone()),
                        Err(_) => {
                            let sess: Arc<dyn ProviderSession> = Arc::from(provider.create_session(config).await?);
                            let rec = provider_sessions::create_provider_session(
                                &self.db,
                                conversation_id,
                                &provider_str,
                                sess.native_session_id().as_deref(),
                                sess.metadata().model.as_deref(),
                            )?;
                            (sess, rec.id)
                        }
                    }
                }
                Some(record) => {
                    let sess: Arc<dyn ProviderSession> = Arc::from(provider.create_session(config).await?);
                    (sess, record.id.clone())
                }
                None => {
                    let sess: Arc<dyn ProviderSession> = Arc::from(provider.create_session(config).await?);
                    let rec = provider_sessions::create_provider_session(
                        &self.db,
                        conversation_id,
                        &provider_str,
                        sess.native_session_id().as_deref(),
                        sess.metadata().model.as_deref(),
                    )?;
                    (sess, rec.id)
                }
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
            
            // Dedicated tokio task listening for events from the session lifecycle
            tokio::spawn(async move {
                let mut current_text = String::new();
                let mut current_thinking = String::new();
                
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
                        let conf_str = match confidence {
                            crate::app::events::TokenConfidence::Exact => "EXACT",
                            crate::app::events::TokenConfidence::Estimated => "ESTIMATED",
                        };
                        let _ = usage_snapshots::create_usage_snapshot(
                            &db_clone,
                            &conv_id,
                            Some(&ps_id),
                            input_tokens.map(|v| *v as i64),
                            output_tokens.map(|v| *v as i64),
                            cache_read_tokens.map(|v| *v as i64),
                            None,
                            reasoning_tokens.map(|v| *v as i64),
                            context_tokens.map(|v| *v as i64),
                            context_window.map(|v| *v as i64),
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
                            }
                            current_text.clear();
                            current_thinking.clear();
                        }
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
                event_tx,
            };

            // Register in active_sessions - brief lock only!
            let mut sessions = self.active_sessions.lock().await;
            if let Some(existing) = sessions.get(&session_key) {
                let mut tx_guard = existing.event_tx.write().await;
                *tx_guard = Some(event_sender.clone());
                existing.session.clone()
            } else {
                sessions.insert(session_key.clone(), entry);
                sess
            }
        };

        // 4. Calculate exact context delta between sync cursor and current message
        let context_delta = build_context_delta(&self.db, conversation_id, synced_through_seq, user_msg.seq)?;

        // 5. Create ProviderMessage with content + missing context delta + attachments
        let attachment_refs: Vec<crate::app::providers::AttachmentRef> = attachments
            .iter()
            .map(|a| crate::app::providers::AttachmentRef {
                id: a.id.clone(),
                path: std::path::PathBuf::from(&a.path),
                mime_type: a.mime_type.clone(),
            })
            .collect();

        let msg = ProviderMessage {
            content: content.to_string(),
            context: context_delta,
            attachments: attachment_refs,
        };

        // 6. Call session.send(message) WITHOUT holding active_sessions lock!
        // This completely prevents approval and interrupt deadlocks!
        session.send(msg).await?;
        
        if let Some(record) = provider_sessions::get_active_session(&self.db, conversation_id, &provider_str)? {
            provider_sessions::update_session_used(&self.db, &record.id)?;
        }

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
        approval_id: &str,
        approved: bool,
    ) -> Result<()> {
        let session = {
            let sessions = self.active_sessions.lock().await;
            sessions.get(&(conversation_id.to_string(), provider)).map(|e| e.session.clone())
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

    pub fn get_conversation_usage(&self, conversation_id: &str) -> Result<Option<UsageSnapshotRecord>> {
        usage_snapshots::get_latest_usage_snapshot(&self.db, conversation_id)
    }

    pub async fn interrupt_turn(
        &self,
        conversation_id: &str,
        provider: ProviderKind,
    ) -> Result<()> {
        let session = {
            let sessions = self.active_sessions.lock().await;
            sessions.get(&(conversation_id.to_string(), provider)).map(|e| e.session.clone())
        };
        if let Some(session) = session {
            session.interrupt().await?;
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
        // 1. File size limit: 25 MB max
        const MAX_ATTACHMENT_SIZE: usize = 25 * 1024 * 1024;
        if file_data.len() > MAX_ATTACHMENT_SIZE {
            return Err(crate::app::error::AppError::InvalidInput(format!(
                "File size ({} bytes) exceeds maximum limit of 25MB",
                file_data.len()
            )));
        }

        // 2. Filename sanitization: extract leaf and reject control / path separator characters
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

        let id = uuid::Uuid::new_v4().to_string();
        let mut base_path = dirs::data_local_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
        base_path.push("Seralyn");
        base_path.push("attachments");
        base_path.push(conversation_id);
        std::fs::create_dir_all(&base_path).map_err(crate::app::error::AppError::Io)?;

        let dest_filename = format!("{}_{}", id, safe_name);
        let dest_path = base_path.join(&dest_filename);

        // 3. Security: Path traversal check ensuring dest_path is strictly inside base_path
        if let (Ok(canon_base), Ok(canon_parent)) = (base_path.canonicalize(), dest_path.parent().unwrap().canonicalize()) {
            if canon_parent != canon_base {
                return Err(crate::app::error::AppError::InvalidInput(
                    "Path traversal attempt detected in attachment filename".to_string(),
                ));
            }
        }

        std::fs::write(&dest_path, file_data).map_err(crate::app::error::AppError::Io)?;

        let mime = mime_type.unwrap_or("application/octet-stream").to_string();

        Ok(AttachmentInfo {
            id,
            name: safe_name.to_string(),
            path: dest_path.to_string_lossy().to_string(),
            size: file_data.len() as u64,
            mime_type: mime,
        })
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
