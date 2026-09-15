use serde_json::Value;

use crate::app::error::Result;
use crate::app::events::{EventPayload, EventType, NormalizedEvent, ProviderKind};
pub use super::protocol::{AcpMessage, AcpNotification, AcpRequest, AcpResponse};

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
    let method = notification.method.as_str();

    match method {
        "session/update" | "agent/progress" => {
            let params = notification.params.as_ref()?;

            // Real ACP protocol: params contains "update" object with "sessionUpdate" discriminator
            if let Some(update) = params.get("update") {
                if let Some(update_type) = update.get("sessionUpdate").and_then(|v| v.as_str()) {
                    match update_type {
                        "agent_message_chunk" => {
                            let text = extract_content_text(update.get("content"));
                            if let Some(t) = text {
                                return Some(create_event(
                                    EventType::TextDelta,
                                    conversation_id,
                                    session_id,
                                    EventPayload::Text { content: t },
                                ));
                            }
                        }
                        "agent_thought_chunk" => {
                            let text = extract_content_text(update.get("content"))
                                .or_else(|| update.get("thought").and_then(|v| v.as_str()).map(ToString::to_string));
                            if let Some(t) = text {
                                return Some(create_event(
                                    EventType::ThinkingDelta,
                                    conversation_id,
                                    session_id,
                                    EventPayload::Thinking { content: t },
                                ));
                            }
                        }
                        "tool_call" => {
                            let tool_call = update.get("toolCall").or_else(|| update.get("tool"));
                            let id = tool_call.and_then(|tc| tc.get("id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let name = tool_call.and_then(|tc| tc.get("name")).and_then(|v| v.as_str()).unwrap_or("tool").to_string();
                            let input = tool_call.and_then(|tc| tc.get("arguments").or_else(|| tc.get("input"))).cloned();

                            return Some(create_event(
                                EventType::ToolStarted,
                                conversation_id,
                                session_id,
                                EventPayload::Tool {
                                    tool_id: id,
                                    tool_name: name,
                                    input,
                                    output: None,
                                    status: Some("started".to_string()),
                                },
                            ));
                        }
                        "tool_call_update" => {
                            let tool_call = update.get("toolCallUpdate").or_else(|| update.get("toolCall"));
                            let id = tool_call.and_then(|tc| tc.get("id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let output = tool_call.and_then(|tc| tc.get("output").or_else(|| tc.get("result"))).cloned();
                            let status = tool_call.and_then(|tc| tc.get("status")).and_then(|v| v.as_str()).map(ToString::to_string);

                            return Some(create_event(
                                EventType::ToolResult,
                                conversation_id,
                                session_id,
                                EventPayload::Tool {
                                    tool_id: id,
                                    tool_name: "".to_string(),
                                    input: None,
                                    output,
                                    status,
                                },
                            ));
                        }
                        _ => {}
                    }
                }
            }

            // Fallback / flat format:
            if let Some(content) = params.get("content")
                .or_else(|| params.get("delta"))
                .or_else(|| params.get("text"))
                .and_then(|v| v.as_str())
            {
                return Some(create_event(
                    EventType::TextDelta,
                    conversation_id,
                    session_id,
                    EventPayload::Text { content: content.to_string() },
                ));
            } else if let Some(thinking) = params.get("thinking").and_then(|v| v.as_str()) {
                return Some(create_event(
                    EventType::ThinkingDelta,
                    conversation_id,
                    session_id,
                    EventPayload::Thinking { content: thinking.to_string() },
                ));
            }
            None
        }
        "session/toolCall" | "agent/toolCall" => {
            let params = notification.params.as_ref()?;
            let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
            let tool_id = params.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let input = params.get("arguments").or_else(|| params.get("input")).cloned();
            
            Some(create_event(
                EventType::ToolStarted,
                conversation_id,
                session_id,
                EventPayload::Tool {
                    tool_id,
                    tool_name,
                    input,
                    output: None,
                    status: Some("started".to_string()),
                },
            ))
        }
        "session/approval" | "agent/approval" => {
            let params = notification.params.as_ref()?;
            let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
            let approval_id = params.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let description = params.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let input = params.get("arguments").or_else(|| params.get("input")).cloned();
            
            Some(create_event(
                EventType::ApprovalRequired,
                conversation_id,
                session_id,
                EventPayload::Approval {
                    approval_id,
                    tool_name,
                    description,
                    input,
                },
            ))
        }
        "session/complete" | "session/cancel" | "agent/complete" => {
            Some(create_event(
                EventType::SessionFinished,
                conversation_id,
                session_id,
                EventPayload::Empty,
            ))
        }
        _ => None,
    }
}

/// Converts an incoming Gemini ACP Server -> Client permission request into a NormalizedEvent
pub fn gemini_server_request_to_normalized(
    request: &AcpRequest,
    conversation_id: &str,
    session_id: Option<&str>,
) -> Option<NormalizedEvent> {
    if request.method == "session/request_permission" {
        let params = request.params.as_ref();
        let tool_call = params.and_then(|p| p.get("toolCall"));
        let tool_name = tool_call
            .and_then(|tc| tc.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("run_cmd")
            .to_string();

        let description = tool_call
            .and_then(|tc| tc.get("arguments"))
            .and_then(|args| args.get("command").or_else(|| args.get("description")))
            .and_then(|v| v.as_str())
            .unwrap_or(&tool_name)
            .to_string();

        let approval_id = match &request.id {
            Value::Number(n) => n.to_string(),
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };

        return Some(create_event(
            EventType::ApprovalRequired,
            conversation_id,
            session_id,
            EventPayload::Approval {
                approval_id,
                tool_name,
                description,
                input: tool_call.and_then(|tc| tc.get("arguments")).cloned(),
            },
        ));
    }
    None
}

fn create_event(
    event_type: EventType,
    conversation_id: &str,
    session_id: Option<&str>,
    payload: EventPayload,
) -> NormalizedEvent {
    let mut event = NormalizedEvent::new(
        event_type,
        ProviderKind::Gemini,
        conversation_id.to_string(),
        payload,
    );
    event.provider_session_id = session_id.map(|s| s.to_string());
    event
}

fn extract_content_text(val: Option<&Value>) -> Option<String> {
    match val {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Object(map)) => {
            map.get("text")
                .or_else(|| map.get("content"))
                .and_then(|v| v.as_str())
                .map(ToString::to_string)
        }
        Some(Value::Array(arr)) => {
            let mut buf = String::new();
            for item in arr {
                if let Some(t) = extract_content_text(Some(item)) {
                    buf.push_str(&t);
                }
            }
            if buf.is_empty() { None } else { Some(buf) }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_gemini_real_acp_session_update() {
        let json_str = r#"{
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "sessionId": "sess_123",
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": {
                        "type": "text",
                        "text": "Hello from Gemini ACP"
                    }
                }
            }
        }"#;
        let notif: AcpNotification = serde_json::from_str(json_str).unwrap();
        let event = gemini_notification_to_normalized(&notif, "conv_1", Some("sess_123")).unwrap();
        assert_eq!(event.event_type, EventType::TextDelta);
        if let EventPayload::Text { content } = event.payload {
            assert_eq!(content, "Hello from Gemini ACP");
        } else {
            panic!("expected text payload");
        }
    }

    #[test]
    fn test_parse_gemini_permission_request() {
        let json_str = r#"{
            "jsonrpc": "2.0",
            "id": 10,
            "method": "session/request_permission",
            "params": {
                "sessionId": "sess_123",
                "options": [{"id": "allow", "title": "Allow"}],
                "toolCall": {
                    "id": "tc_1",
                    "name": "run_terminal_cmd",
                    "arguments": {"command": "git status"}
                }
            }
        }"#;
        let req: AcpRequest = serde_json::from_str(json_str).unwrap();
        let event = gemini_server_request_to_normalized(&req, "conv_1", Some("sess_123")).unwrap();
        assert_eq!(event.event_type, EventType::ApprovalRequired);
        if let EventPayload::Approval { approval_id, tool_name, description, .. } = event.payload {
            assert_eq!(approval_id, "10");
            assert_eq!(tool_name, "run_terminal_cmd");
            assert_eq!(description, "git status");
        } else {
            panic!("expected approval payload");
        }
    }
}
