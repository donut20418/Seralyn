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
use crate::app::db::Database;
use crate::app::error::Result;
use crate::app::events::{EventPayload, EventType, NormalizedEvent, ProviderKind};
use crate::app::providers::{
    PermissionMode, ProviderManager, ProviderMessage, ProviderSession, ProviderStatus, SessionConfig,
};

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
        // 1. Save user message to DB -> returns user_msg with assigned sequence number
        let user_msg = messages::create_message(
            &self.db,
            conversation_id,
            None,
            "user",
            content,
            None,
            None,
            None,
            None,
        )?;

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

                    if event.event_type == EventType::SessionFinished || event.event_type == EventType::Error {
                        if !current_text.is_empty() {
                            let msg_res = messages::create_message(
                                &db_clone,
                                &conv_id,
                                None,
                                "assistant",
                                &current_text,
                                Some(&pk.to_string()),
                                None,
                                Some(&ps_id),
                                None,
                            );

                            if let Ok(msg) = msg_res {
                                // Sync cursor update: this provider session is now synced through this assistant response!
                                let _ = provider_sessions::update_synced_seq(&db_clone, &ps_id, msg.seq);
                            }
                            current_text.clear();
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

        // 5. Create ProviderMessage with content + missing context delta
        let msg = ProviderMessage {
            content: content.to_string(),
            context: context_delta,
            attachments: vec![],
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
