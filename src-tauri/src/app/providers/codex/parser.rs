use chrono::Utc;
use serde_json::Value;
use uuid::Uuid;
use crate::app::error::{AppError, Result};
use crate::app::events::{EventPayload, EventType, NormalizedEvent, ProviderKind, TokenConfidence};
use super::protocol::{JsonRpcMessage, JsonRpcNotification, JsonRpcResponse};

#[derive(Debug, Clone)]
pub enum CodexMessage {
    Response(JsonRpcResponse),
    Notification(JsonRpcNotification),
    Unknown(Value),
}

pub fn parse_codex_line(line: &str) -> Result<Option<CodexMessage>> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }

    let value: Value = serde_json::from_str(line).map_err(AppError::Json)?;

    let msg = serde_json::from_value::<JsonRpcMessage>(value.clone());
    match msg {
        Ok(JsonRpcMessage::Response(res)) => Ok(Some(CodexMessage::Response(res))),
        Ok(JsonRpcMessage::Notification(not)) => Ok(Some(CodexMessage::Notification(not))),
        Ok(JsonRpcMessage::Request(_)) => Ok(Some(CodexMessage::Unknown(value))),
        Err(_) => Ok(Some(CodexMessage::Unknown(value))),
    }
}

pub fn codex_notification_to_normalized(
    notification: &JsonRpcNotification,
    conversation_id: &str,
    session_id: Option<&str>,
) -> Option<NormalizedEvent> {
    let provider = ProviderKind::Codex;
    let mut event = NormalizedEvent {
        id: Uuid::new_v4().to_string(),
        event_type: EventType::TextDelta, // Placeholder
        provider: provider.clone(),
        conversation_id: conversation_id.to_string(),
        provider_session_id: session_id.map(|s| s.to_string()),
        timestamp: Utc::now(),
        payload: EventPayload::Empty,
        raw_provider_event: None,
    };

    match notification.method.as_str() {
        "turn/update" => {
            if let Some(params) = &notification.params {
                if let Some(type_val) = params.get("type").and_then(|v| v.as_str()) {
                    match type_val {
                        "text_delta" => {
                            if let Some(text) = params.get("text").and_then(|v| v.as_str()) {
                                event.event_type = EventType::TextDelta;
                                event.payload = EventPayload::Text {
                                    content: text.to_string(),
                                };
                                return Some(event);
                            }
                        }
                        "thinking_delta" => {
                            if let Some(text) = params.get("text").and_then(|v| v.as_str()) {
                                event.event_type = EventType::ThinkingDelta;
                                event.payload = EventPayload::Thinking {
                                    content: text.to_string(),
                                };
                                return Some(event);
                            }
                        }
                        "tool_call_begin" => {
                            let tool_id = params.get("tool_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let tool_name = params.get("tool_name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            event.event_type = EventType::ToolStarted;
                            event.payload = EventPayload::Tool {
                                tool_id,
                                tool_name,
                                input: params.get("input").cloned(),
                                output: None,
                                status: Some("started".to_string()),
                            };
                            return Some(event);
                        }
                        "tool_call_output" => {
                            let tool_id = params.get("tool_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            event.event_type = EventType::ToolResult;
                            event.payload = EventPayload::Tool {
                                tool_id,
                                tool_name: "".to_string(), // Missing from this event in some cases
                                input: None,
                                output: params.get("output").cloned(),
                                status: Some("finished".to_string()),
                            };
                            return Some(event);
                        }
                        _ => {}
                    }
                }
            }
        }
        "item/agentMessage/delta" => {
            if let Some(params) = &notification.params {
                let text = params.get("delta").or_else(|| params.get("text"))
                    .and_then(|v| v.as_str());
                if let Some(t) = text {
                    event.event_type = EventType::TextDelta;
                    event.payload = EventPayload::Text {
                        content: t.to_string(),
                    };
                    return Some(event);
                }
            }
        }
        "item/started" => {
            if let Some(params) = &notification.params {
                let tool_id = params.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let tool_name = params.get("name").or_else(|| params.get("type"))
                    .and_then(|v| v.as_str()).unwrap_or("tool").to_string();
                event.event_type = EventType::ToolStarted;
                event.payload = EventPayload::Tool {
                    tool_id,
                    tool_name,
                    input: params.get("input").or_else(|| params.get("arguments")).cloned(),
                    output: None,
                    status: Some("started".to_string()),
                };
                return Some(event);
            }
        }
        "item/completed" => {
            if let Some(params) = &notification.params {
                let tool_id = params.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let tool_name = params.get("name").or_else(|| params.get("type"))
                    .and_then(|v| v.as_str()).unwrap_or("").to_string();
                event.event_type = EventType::ToolResult;
                event.payload = EventPayload::Tool {
                    tool_id,
                    tool_name,
                    input: None,
                    output: params.get("output").or_else(|| params.get("result")).cloned(),
                    status: Some("completed".to_string()),
                };
                return Some(event);
            }
        }
        "turn/completed" => {
            event.event_type = EventType::SessionFinished;
            event.payload = EventPayload::Empty;
            return Some(event);
        }
        "turn/finished" => {
            event.event_type = EventType::SessionFinished;
            event.payload = EventPayload::Empty;
            
            if let Some(params) = &notification.params {
                if let Some(usage_val) = params.get("usage") {
                    let prompt_tokens = usage_val.get("prompt_tokens").or_else(|| usage_val.get("input_tokens")).and_then(|v| v.as_u64());
                    let completion_tokens = usage_val.get("completion_tokens").or_else(|| usage_val.get("output_tokens")).and_then(|v| v.as_u64());
                    
                    event.event_type = EventType::UsageUpdated;
                    event.payload = EventPayload::Usage {
                        input_tokens: prompt_tokens,
                        output_tokens: completion_tokens,
                        cache_read_tokens: None,
                        reasoning_tokens: None,
                        context_tokens: None,
                        context_window: None,
                        confidence: TokenConfidence::Exact,
                    };
                }
            }
            
            if event.event_type == EventType::TextDelta {
                event.event_type = EventType::SessionFinished;
            }
            return Some(event);
        }
        "approval/request" => {
            if let Some(params) = &notification.params {
                let id = params.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let summary = params.get("summary").and_then(|v| v.as_str()).unwrap_or("").to_string();
                event.event_type = EventType::ApprovalRequired;
                event.payload = EventPayload::Approval {
                    approval_id: id,
                    tool_name: params.get("tool_name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    description: summary,
                    input: params.get("input").cloned(),
                };
                return Some(event);
            }
        }
        _ => {}
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_codex_line_notification() {
        let line = r#"{"jsonrpc":"2.0","method":"turn/update","params":{"type":"text_delta","text":"hello"}}"#;
        let msg = parse_codex_line(line).unwrap().unwrap();
        match msg {
            CodexMessage::Notification(not) => {
                assert_eq!(not.method, "turn/update");
            }
            _ => panic!("expected notification"),
        }
    }
}
