//! Provider abstraction layer.
//!
//! Defines the common interface that all AI CLI providers (Claude, Codex, Gemini)
//! must implement. The application communicates exclusively through this interface,
//! keeping provider-specific details confined to individual adapter modules.

pub mod claude;
pub mod codex;
pub mod gemini;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::app::error::{AppError, Result};
use crate::app::events::{NormalizedEvent, ProviderKind};

// ─── Provider Trait ───────────────────────────────────────────────────────────

/// Common interface for all AI CLI providers.
///
/// Each provider implementation handles:
/// - CLI detection and version checking
/// - Authentication verification
/// - Session creation and resumption
/// - Message sending with streaming events
#[async_trait]
pub trait Provider: Send + Sync {
    /// Returns the kind of this provider.
    fn kind(&self) -> ProviderKind;

    /// Check if the CLI executable is installed and get version info.
    async fn detect_installation(&self) -> Result<InstallationInfo>;

    /// Check if the CLI is authenticated/logged in.
    async fn check_authentication(&self) -> Result<AuthStatus>;

    /// Get the set of capabilities this provider supports.
    fn capabilities(&self) -> ProviderCapabilities;

    /// Create a new provider session for a conversation.
    async fn create_session(
        &self,
        config: SessionConfig,
    ) -> Result<Box<dyn ProviderSession>>;

    /// Resume an existing native session by its provider-specific ID.
    async fn resume_session(
        &self,
        native_session_id: &str,
        config: SessionConfig,
    ) -> Result<Box<dyn ProviderSession>>;
}

/// Active session with a provider.
///
/// Represents a running interaction with a CLI process.
/// Events are streamed through the mpsc channel returned by `events()`.
#[async_trait]
pub trait ProviderSession: Send + Sync {
    /// Send a message to the provider and start streaming the response.
    /// Events will be emitted through the events channel.
    async fn send(&self, message: ProviderMessage) -> Result<()>;

    /// Interrupt the current generation (provider may continue from interruption point).
    async fn interrupt(&self) -> Result<()>;

    /// Cancel the current operation entirely.
    async fn cancel(&self) -> Result<()>;

    /// Close the session gracefully.
    async fn close(&self) -> Result<()>;

    /// Get the native provider session ID (if available).
    fn native_session_id(&self) -> Option<String>;

    /// Respond to a pending approval or permission request.
    async fn respond_to_approval(&self, _request_id: &str, _approved: bool) -> Result<()> {
        Ok(())
    }

    /// Get session metadata.
    fn metadata(&self) -> SessionMetadata;

    /// Check if the session is still active.
    fn is_active(&self) -> bool;
}

// ─── Types ────────────────────────────────────────────────────────────────────

/// Information about a provider CLI installation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallationInfo {
    pub installed: bool,
    pub executable_path: Option<PathBuf>,
    pub version: Option<String>,
}

/// Authentication status for a provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AuthStatus {
    Authenticated,
    NotAuthenticated,
    /// Cannot determine auth status (provider doesn't support check).
    Unknown,
}

/// Capabilities supported by a provider/model combination.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    pub text: bool,
    pub image: bool,
    pub file: bool,
    pub tools: bool,
    pub skills: bool,
    pub mcp: bool,
    pub resume: bool,
    pub native_compact: bool,
    pub token_usage: bool,
    pub context_window: bool,
    pub reasoning: bool,
    pub streaming: bool,
}

impl Default for ProviderCapabilities {
    fn default() -> Self {
        Self {
            text: true,
            image: false,
            file: false,
            tools: false,
            skills: false,
            mcp: false,
            resume: false,
            native_compact: false,
            token_usage: false,
            context_window: false,
            reasoning: false,
            streaming: true,
        }
    }
}

/// Configuration for creating a provider session.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Our application's conversation ID.
    pub conversation_id: String,
    /// Working directory for the CLI process.
    pub working_dir: Option<PathBuf>,
    /// Permission mode to use.
    pub permission_mode: PermissionMode,
    /// System/project instructions to inject.
    pub system_prompt: Option<String>,
    /// Model override (if None, use provider default).
    pub model: Option<String>,
    /// Account / Profile identifier.
    pub account: Option<String>,
    /// Reasoning effort level.
    pub effort: Option<String>,
    /// Additional environment variables.
    pub env: HashMap<String, String>,
    /// Channel to send normalized events through.
    pub event_sender: mpsc::Sender<NormalizedEvent>,
}

/// A message to send to a provider.
#[derive(Debug, Clone)]
pub struct ProviderMessage {
    /// The text content of the message.
    pub content: String,
    /// Conversation context (previous messages) for providers that need full context.
    pub context: Vec<ContextMessage>,
    /// Attached files/images (Phase 2).
    pub attachments: Vec<AttachmentRef>,
}

/// A message in the conversation context sent to providers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextMessage {
    pub role: String,
    pub content: String,
    pub provider: Option<String>,
}

/// Reference to an attachment (Phase 2 stub).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentRef {
    pub id: String,
    pub path: PathBuf,
    pub mime_type: String,
}

/// Session metadata returned by a provider session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub provider: ProviderKind,
    pub native_session_id: Option<String>,
    pub model: Option<String>,
    pub created_at: String,
}

impl Default for SessionMetadata {
    fn default() -> Self {
        Self {
            provider: ProviderKind::Claude,
            native_session_id: None,
            model: None,
            created_at: String::new(),
        }
    }
}

/// Permission mode for provider operations.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PermissionMode {
    /// Ask before filesystem writes or command execution.
    Safe,
    /// Permit operations inside approved project directory.
    Workspace,
    /// Must be explicitly enabled — no safety restrictions.
    FullAccess,
}

impl Default for PermissionMode {
    fn default() -> Self {
        Self::Safe
    }
}

// ─── Provider Manager ─────────────────────────────────────────────────────────

/// Manages all registered providers and active sessions.
pub struct ProviderManager {
    providers: HashMap<ProviderKind, Arc<dyn Provider>>,
}

impl ProviderManager {
    /// Create a new ProviderManager and register all built-in providers.
    pub fn new() -> Self {
        let mut providers: HashMap<ProviderKind, Arc<dyn Provider>> = HashMap::new();

        providers.insert(
            ProviderKind::Claude,
            Arc::new(claude::ClaudeProvider::new()),
        );
        providers.insert(
            ProviderKind::Codex,
            Arc::new(codex::CodexProvider::new()),
        );
        providers.insert(
            ProviderKind::Gemini,
            Arc::new(gemini::GeminiProvider::new()),
        );

        Self { providers }
    }

    /// Creates a ProviderManager with custom providers (used in testing).
    pub fn with_providers(providers: HashMap<ProviderKind, Arc<dyn Provider>>) -> Self {
        Self { providers }
    }

    /// Get a provider by kind.
    pub fn get(&self, kind: ProviderKind) -> Result<Arc<dyn Provider>> {
        self.providers
            .get(&kind)
            .cloned()
            .ok_or_else(|| AppError::InvalidInput(format!("Unknown provider: {kind}")))
    }

    /// Detect all installed providers and their status.
    pub async fn detect_all(&self) -> Vec<ProviderStatus> {
        let mut results = Vec::new();
        for (kind, provider) in &self.providers {
            let installation = provider.detect_installation().await;
            let auth = if installation.as_ref().map(|i| i.installed).unwrap_or(false) {
                provider.check_authentication().await.ok()
            } else {
                None
            };

            results.push(ProviderStatus {
                provider: *kind,
                installed: installation.as_ref().map(|i| i.installed).unwrap_or(false),
                version: installation
                    .as_ref()
                    .ok()
                    .and_then(|i| i.version.clone()),
                authenticated: auth.unwrap_or(AuthStatus::Unknown),
                capabilities: provider.capabilities(),
                error: installation.err().map(|e| e.to_string()),
            });
        }
        results
    }
}

/// Status information about a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderStatus {
    pub provider: ProviderKind,
    pub installed: bool,
    pub version: Option<String>,
    pub authenticated: AuthStatus,
    pub capabilities: ProviderCapabilities,
    pub error: Option<String>,
}

/// Resolves an isolated profile directory for a given provider and account.
/// Returns None if account is None, empty, or "default".
pub fn resolve_profile_dir(provider: ProviderKind, account: Option<&str>) -> Option<PathBuf> {
    let acc = account?;
    if acc.trim().is_empty() || acc == "default" {
        return None;
    }
    let safe_name: String = acc
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    if safe_name.is_empty() {
        return None;
    }

    let mut path = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("Seralyn");
    path.push("profiles");
    path.push(provider.to_string());
    path.push(safe_name);

    let _ = std::fs::create_dir_all(&path);
    Some(path)
}
