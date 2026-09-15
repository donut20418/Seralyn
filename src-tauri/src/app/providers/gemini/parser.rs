use serde_json::Value;

use crate::app::error::Result;
use crate::app::events::{EventPayload, EventType, NormalizedEvent, ProviderKind};
use super::protocol::{AcpMessage, AcpNotification};

/// Parse a raw JSON-RPC line from Gemini ACP into an AcpMessage.
pub fn parse_gemini_line(line: &str) -> Result<Option<AcpMessage>> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }
    
    match AcpMessage::parse(line) {
        Ok(msg) => Ok(Some(msg)),
        Err(e) => {
            tracing::warn!("Failed to parse Gemini ACP message: {}", e);
            Ok(None)
        }
    }
}

/// Convert a Gemini ACP notification to a NormalizedEvent.
pub fn gemini_notification_to_normalized(
    notification: &AcpNotification,
    conversation_id: &str,
    session_id: Option<&str>,
) -> Option<NormalizedEvent> {
    let payload = match notification.method.as_str() {
        "agent/progress" => {
            if let Some(params) = &notification.params {
                if let Some(content) = params.get("content").and_then(|v| v.as_str()) {
                    EventPayload::Text {
                        content: content.to_string(),
                    }
                } else if let Some(thinking) = params.get("thinking").and_then(|v| v.as_str()) {
                    EventPayload::Thinking {
                        content: thinking.to_string(),
                    }
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }
        "agent/toolCall" => {
            if let Some(params) = &notification.params {
                let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                let tool_id = params.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let input = params.get("arguments").cloned();
                
                EventPayload::Tool {
                    tool_id,
                    tool_name,
                    input,
                    output: None,
                    status: Some("started".to_string()),
                }
            } else {
                return None;
            }
        }
        "agent/approval" => {
            if let Some(params) = &notification.params {
                let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                let approval_id = params.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let description = params.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let input = params.get("arguments").cloned();
                
                EventPayload::Approval {
                    approval_id,
                    tool_name,
                    description,
                    input,
                }
            } else {
                return None;
            }
        }
        "agent/complete" => {
            EventPayload::Empty
        }
        _ => return None,
    };
    
    let event_type = match payload {
        EventPayload::Text { .. } => EventType::TextDelta,
        EventPayload::Thinking { .. } => EventType::ThinkingDelta,
        EventPayload::Tool { .. } => EventType::ToolStarted,
        EventPayload::Approval { .. } => EventType::ApprovalRequired,
        EventPayload::Empty => EventType::SessionFinished,
        _ => return None,
    };
    
    let mut event = NormalizedEvent::new(
        event_type,
        ProviderKind::Gemini,
        conversation_id.to_string(),
        payload,
    );
    event.provider_session_id = session_id.map(|s| s.to_string());
    
    Some(event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_gemini_line() {
        let req_str = r#"{"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}"#;
        let res = parse_gemini_line(req_str).unwrap().unwrap();
        match res {
            AcpMessage::Request(req) => {
                assert_eq!(req.method, "initialize");
                assert_eq!(req.id, 1);
            }
            _ => panic!("Expected request"),
        }

        let notif_str = r#"{"jsonrpc": "2.0", "method": "agent/progress", "params": {"content": "hello"}}"#;
        let res = parse_gemini_line(notif_str).unwrap().unwrap();
        match res {
            AcpMessage::Notification(notif) => {
                assert_eq!(notif.method, "agent/progress");
            }
            _ => panic!("Expected notification"),
        }
    }

    #[test]
    fn test_gemini_notification_to_normalized_text() {
        let notif = AcpNotification {
            jsonrpc: "2.0".to_string(),
            method: "agent/progress".to_string(),
            params: Some(json!({
                "content": "Hello world"
            })),
        };

        let event = gemini_notification_to_normalized(&notif, "conv-1", Some("sess-1")).unwrap();
        assert_eq!(event.event_type, EventType::TextDelta);
        assert_eq!(event.provider_session_id.as_deref(), Some("sess-1"));
        
        match event.payload {
            EventPayload::Text { content } => assert_eq!(content, "Hello world"),
            _ => panic!("Expected Text payload"),
        }
    }
}
