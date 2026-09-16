use chrono::Utc;
use serde_json::Value;
use uuid::Uuid;
use crate::app::error::Result;
use crate::app::events::{EventPayload, EventType, NormalizedEvent, ProviderKind, TokenConfidence};
use super::protocol::{JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};

#[derive(Debug, Clone)]
pub enum CodexMessage {
    Response(JsonRpcResponse),
    Notification(JsonRpcNotification),
    ServerRequest(JsonRpcRequest),
    Unknown(Value),
}

pub fn parse_codex_line(line: &str) -> Result<Option<CodexMessage>> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }

    let value: Value = serde_json::from_str(line)?;

    let msg = serde_json::from_value::<JsonRpcMessage>(value.clone());
    match msg {
        Ok(JsonRpcMessage::Response(res)) => Ok(Some(CodexMessage::Response(res))),
        Ok(JsonRpcMessage::Notification(not)) => Ok(Some(CodexMessage::Notification(not))),
        Ok(JsonRpcMessage::Request(req)) => Ok(Some(CodexMessage::ServerRequest(req))),
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
        event_type: EventType::TextDelta,
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
                                tool_name: "".to_string(),
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
                let text = params.get("delta")
                    .or_else(|| params.get("text"))
                    .or_else(|| params.get("item").and_then(|it| it.get("delta")))
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
                let item = params.get("item");
                let item_type = item.and_then(|it| it.get("type")).or_else(|| params.get("type")).and_then(|v| v.as_str()).unwrap_or("tool");
                let tool_id = item.and_then(|it| it.get("id")).or_else(|| params.get("id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let tool_name = item.and_then(|it| it.get("name")).or_else(|| params.get("name")).and_then(|v| v.as_str()).unwrap_or(item_type).to_string();
                
                // Do not emit ToolStarted for agentMessage
                if item_type != "agentMessage" && item_type != "message" {
                    event.event_type = EventType::ToolStarted;
                    event.payload = EventPayload::Tool {
                        tool_id,
                        tool_name,
                        input: item.and_then(|it| it.get("input").or_else(|| it.get("arguments"))).or_else(|| params.get("input")).cloned(),
                        output: None,
                        status: Some("started".to_string()),
                    };
                    return Some(event);
                }
            }
        }
        "item/completed" => {
            if let Some(params) = &notification.params {
                let item = params.get("item");
                let item_type = item.and_then(|it| it.get("type")).or_else(|| params.get("type")).and_then(|v| v.as_str()).unwrap_or("");
                let tool_id = item.and_then(|it| it.get("id")).or_else(|| params.get("id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let tool_name = item.and_then(|it| it.get("name")).or_else(|| params.get("name")).and_then(|v| v.as_str()).unwrap_or(item_type).to_string();
                
                // Only emit ToolResult if item was an actual tool or command execution (not an agent message)
                if item_type != "agentMessage" && item_type != "message" && !item_type.is_empty() {
                    event.event_type = EventType::ToolResult;
                    event.payload = EventPayload::Tool {
                        tool_id,
                        tool_name,
                        input: None,
                        output: item.and_then(|it| it.get("output").or_else(|| it.get("result"))).or_else(|| params.get("output")).cloned(),
                        status: Some("completed".to_string()),
                    };
                    return Some(event);
                }
            }
        }
        "turn/completed" => {
            event.event_type = EventType::SessionFinished;
            event.payload = EventPayload::Empty;

            if let Some(params) = &notification.params {
                if let Some(usage_val) = params.get("usage") {
                    let prompt_tokens = usage_val.get("prompt_tokens").or_else(|| usage_val.get("input_tokens")).and_then(|v| v.as_u64());
                    let completion_tokens = usage_val.get("completion_tokens").or_else(|| usage_val.get("output_tokens")).and_then(|v| v.as_u64());

                    if prompt_tokens.is_some() || completion_tokens.is_some() {
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
            }
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
            return Some(event);
        }
        "approval/request" => {
            if let Some(params) = &notification.params {
                let id = params.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let summary = params.get("summary").or_else(|| params.get("description")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                event.event_type = EventType::ApprovalRequired;
                event.payload = EventPayload::Approval {
                    approval_id: id,
                    tool_name: params.get("tool_name").and_then(|v| v.as_str()).unwrap_or("tool").to_string(),
                    description: summary,
                    input: params.get("input").cloned(),
                };
                return Some(event);
            }
        }
        "error" => {
            let msg = notification.params.as_ref()
                .and_then(|p| p.get("error"))
                .and_then(|e| e.get("message").or_else(|| e.get("msg")))
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown Codex error")
                .to_string();
            event.event_type = EventType::Error;
            event.payload = EventPayload::Error {
                code: None,
                message: msg,
            };
            return Some(event);
        }
        _ => {}
    }
    None
}

/// Converts an incoming Server -> Client approval request into a NormalizedEvent
pub fn codex_server_request_to_normalized(
    request: &JsonRpcRequest,
    conversation_id: &str,
    session_id: Option<&str>,
) -> Option<NormalizedEvent> {
    let provider = ProviderKind::Codex;
    let id_str = match &request.id {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };

    let params = request.params.as_ref();
    let tool_name = match request.method.as_str() {
        "item/commandExecution/requestApproval" => "commandExecution",
        "item/fileChange/requestApproval" => "fileChange",
        "item/permissions/requestApproval" => "permissions",
        other => other,
    };

    let description = params
        .and_then(|p| {
            p.get("command")
                .or_else(|| p.get("reason"))
                .or_else(|| p.get("description"))
                .or_else(|| p.get("summary"))
        })
        .and_then(|v| v.as_str())
        .unwrap_or(tool_name)
        .to_string();

    Some(NormalizedEvent {
        id: Uuid::new_v4().to_string(),
        event_type: EventType::ApprovalRequired,
        provider,
        conversation_id: conversation_id.to_string(),
        provider_session_id: session_id.map(|s| s.to_string()),
        timestamp: Utc::now(),
        payload: EventPayload::Approval {
            approval_id: id_str,
            tool_name: tool_name.to_string(),
            description,
            input: params.cloned(),
        },
        raw_provider_event: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_parse_codex_server_request_approval() {
        let line = r#"{"jsonrpc":"2.0","id":42,"method":"item/commandExecution/requestApproval","params":{"command":"npm test"}}"#;
        let msg = parse_codex_line(line).unwrap().unwrap();
        match msg {
            CodexMessage::ServerRequest(req) => {
                assert_eq!(req.method, "item/commandExecution/requestApproval");
                let event = codex_server_request_to_normalized(&req, "c1", Some("s1")).unwrap();
                assert_eq!(event.event_type, EventType::ApprovalRequired);
                if let EventPayload::Approval { approval_id, description, .. } = event.payload {
                    assert_eq!(approval_id, "42");
                    assert_eq!(description, "npm test");
                } else {
                    panic!("expected approval payload");
                }
            }
            _ => panic!("expected server request"),
        }
    }
}
