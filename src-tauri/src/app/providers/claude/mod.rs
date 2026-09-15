use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use tracing::{error, info};

use crate::app::error::{AppError, Result};
use crate::app::events::{NormalizedEvent, ProviderKind};
use crate::app::process::{detect_executable, spawn, SpawnConfig};
use crate::app::providers::{
    AuthStatus, InstallationInfo, Provider, ProviderCapabilities, ProviderMessage,
    ProviderSession, SessionConfig, SessionMetadata,
};

pub mod parser;

use parser::{claude_event_to_normalized, parse_claude_line, ClaudeEvent};

pub struct ClaudeProvider {}

impl ClaudeProvider {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl Provider for ClaudeProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Claude
    }

    async fn detect_installation(&self) -> Result<InstallationInfo> {
        match detect_executable("claude").await {
            Ok(info) => Ok(InstallationInfo {
                installed: true,
                executable_path: Some(info.path),
                version: info.version,
            }),
            Err(_) => Ok(InstallationInfo {
                installed: false,
                executable_path: None,
                version: None,
            }),
        }
    }

    async fn check_authentication(&self) -> Result<AuthStatus> {
        let output = tokio::process::Command::new("claude")
            .arg("auth")
            .arg("status")
            .output()
            .await;

        match output {
            Ok(out) if out.status.success() => Ok(AuthStatus::Authenticated),
            Ok(_) => Ok(AuthStatus::NotAuthenticated),
            Err(_) => Ok(AuthStatus::Unknown),
        }
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            text: true,
            image: false,
            file: false,
            tools: true,
            skills: false,
            mcp: false,
            resume: true,
            native_compact: false,
            token_usage: true,
            context_window: false,
            reasoning: true,
            streaming: true,
        }
    }

    async fn create_session(
        &self,
        config: SessionConfig,
    ) -> Result<Box<dyn ProviderSession>> {
        Ok(Box::new(ClaudeSession::new(config, None)))
    }

    async fn resume_session(
        &self,
        native_session_id: &str,
        config: SessionConfig,
    ) -> Result<Box<dyn ProviderSession>> {
        Ok(Box::new(ClaudeSession::new(
            config,
            Some(native_session_id.to_string()),
        )))
    }
}

pub struct ClaudeSession {
    config: SessionConfig,
    native_session_id: Option<String>,
    model: Option<String>,
    active: bool,
    created_at: String,
}

impl ClaudeSession {
    pub fn new(config: SessionConfig, native_session_id: Option<String>) -> Self {
        Self {
            model: config.model.clone(),
            config,
            native_session_id,
            active: true,
            created_at: Utc::now().to_rfc3339(),
        }
    }
}

#[async_trait]
impl ProviderSession for ClaudeSession {
    async fn send(&mut self, message: ProviderMessage) -> Result<()> {
        let mut args = vec![
            "-p".to_string(),
            message.content,
            "--output-format".to_string(),
            "stream-json".to_string(),
        ];

        if let Some(sid) = &self.native_session_id {
            args.push("--session-id".to_string());
            args.push(sid.clone());
            args.push("--resume".to_string());
        }

        let spawn_config = SpawnConfig {
            executable: "claude".to_string(),
            args,
            working_dir: self.config.working_dir.clone(),
            env: self.config.env.clone(),
            startup_timeout: Duration::from_secs(10),
        };

        let mut process = spawn(spawn_config).await?;
        let event_sender = self.config.event_sender.clone();
        let provider = ProviderKind::Claude;
        let conv_id = self.config.conversation_id.clone();
        
        // Read the first line synchronously to get the init event and extract session_id
        if let Ok(Some(line)) = process.recv_stdout_line().await {
            if let Ok(Some(event)) = parse_claude_line(&line) {
                if let ClaudeEvent::Init { session_id, model } = &event {
                    if let Some(sid) = session_id {
                        self.native_session_id = Some(sid.clone());
                    }
                    if let Some(m) = model {
                        self.model = Some(m.clone());
                    }
                }
                
                if let Some(normalized) = claude_event_to_normalized(
                    event,
                    provider.clone(),
                    &conv_id,
                    self.native_session_id.as_deref(),
                ) {
                    let _ = event_sender.send(normalized).await;
                }
            }
        }

        let mut latest_sid = self.native_session_id.clone();

        tokio::spawn(async move {
            let mut finished_sent = false;
            while let Ok(Some(line)) = process.recv_stdout_line().await {
                if let Ok(Some(event)) = parse_claude_line(&line) {
                    if let ClaudeEvent::Result { session_id, .. } = &event {
                        if let Some(sid) = session_id {
                            latest_sid = Some(sid.clone());
                        }
                    }

                    if let Some(normalized) = claude_event_to_normalized(
                        event,
                        provider.clone(),
                        &conv_id,
                        latest_sid.as_deref(),
                    ) {
                        if normalized.event_type == crate::app::events::EventType::SessionFinished {
                            finished_sent = true;
                        }
                        let _ = event_sender.send(normalized).await;
                    }
                }
            }

            if !finished_sent {
                let finish_event = NormalizedEvent::session_finished(provider.clone(), conv_id.clone());
                let _ = event_sender.send(finish_event).await;
            }

            let _ = process.shutdown(Duration::from_secs(5)).await;
        });

        Ok(())
    }

    async fn interrupt(&mut self) -> Result<()> {
        Ok(())
    }

    async fn cancel(&mut self) -> Result<()> {
        Ok(())
    }

    async fn close(&mut self) -> Result<()> {
        self.active = false;
        Ok(())
    }

    fn native_session_id(&self) -> Option<&str> {
        self.native_session_id.as_deref()
    }

    fn metadata(&self) -> SessionMetadata {
        SessionMetadata {
            provider: ProviderKind::Claude,
            native_session_id: self.native_session_id.clone(),
            model: self.model.clone(),
            created_at: self.created_at.clone(),
        }
    }

    fn is_active(&self) -> bool {
        self.active
    }
}
