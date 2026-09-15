pub mod context;

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};

use serde::{Deserialize, Serialize};

use crate::app::db::Database;
pub use crate::app::db::conversations::{Conversation, ConversationSummary};
use crate::app::db::conversations;
pub use crate::app::db::messages::Message;
use crate::app::db::messages;
pub use crate::app::db::provider_sessions::ProviderSessionRecord;
use crate::app::db::provider_sessions;
use crate::app::error::{AppError, Result};
use crate::app::events::{NormalizedEvent, ProviderKind, EventType, EventPayload};
use crate::app::providers::{ProviderManager, ProviderSession, SessionConfig, PermissionMode, ProviderMessage, ProviderStatus};
use crate::app::tokens::TokenManager;
use crate::app::conversation::context::build_context;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationWithMessages {
    pub conversation: Conversation,
    pub messages: Vec<Message>,
    pub provider_sessions: Vec<ProviderSessionRecord>,
}

pub struct ConversationManager {
    db: Arc<Database>,
    provider_manager: Arc<ProviderManager>,
    token_manager: TokenManager,
    /// Active provider sessions keyed by (conversation_id, provider)
    active_sessions: Arc<Mutex<HashMap<(String, ProviderKind), Box<dyn ProviderSession>>>>,
}

impl ConversationManager {
    pub fn new(db: Arc<Database>, provider_manager: Arc<ProviderManager>) -> Self {
        Self {
            db,
            provider_manager,
            token_manager: TokenManager::new(),
            active_sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn create_conversation(&self, title: Option<&str>) -> Result<Conversation> {
        conversations::create_conversation(&self.db, title)
    }

    pub fn list_conversations(&self) -> Result<Vec<ConversationSummary>> {
        conversations::list_conversations(&self.db)
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

    pub async fn delete_conversation(&self, id: &str) -> Result<()> {
        let mut sessions = self.active_sessions.lock().await;
        let keys: Vec<_> = sessions.keys().filter(|k| k.0 == id).cloned().collect();
        for k in keys {
            if let Some(mut session) = sessions.remove(&k) {
                let _ = session.close().await;
            }
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
        // 1. Save user message to DB
        messages::create_message(
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

        // 2. Get or create a provider session
        let provider = self.provider_manager.get(provider_kind.clone())?;
        
        let mut sessions = self.active_sessions.lock().await;
        let session_key = (conversation_id.to_string(), provider_kind.clone());
        
        if !sessions.contains_key(&session_key) {
            let (internal_tx, mut internal_rx) = mpsc::channel(100);
            
            let config = SessionConfig {
                conversation_id: conversation_id.to_string(),
                working_dir: None,
                permission_mode: PermissionMode::Workspace,
                system_prompt: None,
                model: None,
                env: HashMap::new(),
                event_sender: internal_tx,
            };
            
            let session = provider.create_session(config).await?;
            
            let record = provider_sessions::create_provider_session(
                &self.db,
                conversation_id,
                &provider_kind.to_string(),
                session.native_session_id(),
                session.metadata().model.as_deref(),
            )?;
            
            let db_clone = self.db.clone();
            let event_sender_clone = event_sender.clone();
            let conv_id = conversation_id.to_string();
            let pk = provider_kind.clone();
            let provider_session_id = record.id.clone();
            
            // 6. Spawn a tokio task that listens for events from the session
            tokio::spawn(async move {
                let mut current_text = String::new();
                
                while let Some(event) = internal_rx.recv().await {
                    if let EventPayload::Text { content } = &event.payload {
                        current_text.push_str(content);
                    }

                    if event.event_type == EventType::SessionFinished || event.event_type == EventType::Error {
                        if !current_text.is_empty() {
                            let _ = messages::create_message(
                                &db_clone,
                                &conv_id,
                                None,
                                "assistant",
                                &current_text,
                                Some(&pk.to_string()),
                                None,
                                Some(&provider_session_id),
                                None,
                            );
                            current_text.clear();
                        }
                    }
                    
                    if event_sender_clone.send(event).await.is_err() {
                        break;
                    }
                }
            });
            
            sessions.insert(session_key.clone(), session);
        }

        let session = sessions.get_mut(&session_key).unwrap();

        // 3. Build context from conversation history
        let context = build_context(&self.db, conversation_id, 50)?;

        // 4. Create ProviderMessage with content + context
        let msg = ProviderMessage {
            content: content.to_string(),
            context,
            attachments: vec![],
        };

        // 5. Call session.send(message)
        session.send(msg).await?;
        
        if let Some(record) = provider_sessions::get_active_session(&self.db, conversation_id, &provider_kind.to_string())? {
            provider_sessions::update_session_used(&self.db, &record.id)?;
        }

        Ok(())
    }

    pub fn switch_provider(&self, conversation_id: &str, new_provider: ProviderKind) -> Result<()> {
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
        // Forward approval response to active session (Phase 1 stub)
        Ok(())
    }

    pub async fn detect_providers(&self) -> Vec<ProviderStatus> {
        self.provider_manager.detect_all().await
    }

    pub async fn close_all_sessions(&self) -> Result<()> {
        let mut sessions = self.active_sessions.lock().await;
        for (_, mut session) in sessions.drain() {
            let _ = session.close().await;
        }
        Ok(())
    }
}
