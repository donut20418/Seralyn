use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::str::FromStr;
use uuid::Uuid;
use serde_json::Value;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ProviderKind {
    Claude,
    Codex,
    Gemini,
}

impl Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderKind::Claude => write!(f, "Claude"),
            ProviderKind::Codex => write!(f, "Codex"),
            ProviderKind::Gemini => write!(f, "Gemini"),
        }
    }
}

impl FromStr for ProviderKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "claude" => Ok(ProviderKind::Claude),
            "codex" => Ok(ProviderKind::Codex),
            "gemini" => Ok(ProviderKind::Gemini),
            _ => Err(format!("Unknown provider: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EventType {
    SessionStarted,
    TextDelta,
    ThinkingDelta,
    ToolStarted,
    ToolProgress,
    ToolResult,
    ApprovalRequired,
    AttachmentReceived,
    FileChange,
    UsageUpdated,
    ContextUpdated,
    Error,
    SessionFinished,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TokenConfidence {
    Exact,
    Estimated,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind")]
pub enum EventPayload {
    Text {
        content: String,
    },
    Thinking {
        content: String,
    },
    Tool {
        tool_id: String,
        tool_name: String,
        input: Option<Value>,
        output: Option<Value>,
        status: Option<String>,
    },
    Approval {
        approval_id: String,
        tool_name: String,
        description: String,
        input: Option<Value>,
    },
    Usage {
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
        cache_read_tokens: Option<u64>,
        reasoning_tokens: Option<u64>,
        context_tokens: Option<u64>,
        context_window: Option<u64>,
        confidence: TokenConfidence,
    },
    Error {
        code: Option<String>,
        message: String,
    },
    Session {
        session_id: Option<String>,
        model: Option<String>,
    },
    FileChange {
        path: String,
        change_type: String,
    },
    Empty,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NormalizedEvent {
    pub id: String,
    pub event_type: EventType,
    pub provider: ProviderKind,
    pub conversation_id: String,
    pub provider_session_id: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub payload: EventPayload,
    pub raw_provider_event: Option<Value>,
}

impl NormalizedEvent {
    pub fn new(
        event_type: EventType,
        provider: ProviderKind,
        conversation_id: String,
        payload: EventPayload,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            event_type,
            provider,
            conversation_id,
            provider_session_id: None,
            timestamp: Utc::now(),
            payload,
            raw_provider_event: None,
        }
    }

    pub fn text_delta(provider: ProviderKind, conv_id: String, content: String) -> Self {
        Self::new(
            EventType::TextDelta,
            provider,
            conv_id,
            EventPayload::Text { content },
        )
    }

    pub fn error(provider: ProviderKind, conv_id: String, message: String) -> Self {
        Self::new(
            EventType::Error,
            provider,
            conv_id,
            EventPayload::Error {
                code: None,
                message,
            },
        )
    }

    pub fn session_started(
        provider: ProviderKind,
        conv_id: String,
        session_id: Option<String>,
        model: Option<String>,
    ) -> Self {
        Self::new(
            EventType::SessionStarted,
            provider,
            conv_id,
            EventPayload::Session { session_id, model },
        )
    }

    pub fn session_finished(provider: ProviderKind, conv_id: String) -> Self {
        Self::new(
            EventType::SessionFinished,
            provider,
            conv_id,
            EventPayload::Empty,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialization_roundtrip() {
        let event = NormalizedEvent::text_delta(
            ProviderKind::Claude,
            "conv-123".to_string(),
            "Hello".to_string(),
        );

        let serialized = serde_json::to_string(&event).unwrap();
        let deserialized: NormalizedEvent = serde_json::from_str(&serialized).unwrap();

        assert_eq!(event.id, deserialized.id);
        assert_eq!(event.event_type, deserialized.event_type);
        assert_eq!(event.provider, deserialized.provider);
        assert_eq!(event.conversation_id, deserialized.conversation_id);
        assert_eq!(event.payload, deserialized.payload);
    }
}
