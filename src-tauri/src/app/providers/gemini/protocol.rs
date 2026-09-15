use serde::{Deserialize, Serialize};

/// JSON-RPC 2.0 Request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 Response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpResponse {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 Notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// Message types in ACP
#[derive(Debug, Clone)]
pub enum AcpMessage {
    Request(AcpRequest),
    Response(AcpResponse),
    Notification(AcpNotification),
    Unknown(serde_json::Value),
}

/// Helper to create a request
pub fn make_acp_request(id: u64, method: &str, params: Option<serde_json::Value>) -> AcpRequest {
    AcpRequest {
        jsonrpc: "2.0".to_string(),
        id,
        method: method.to_string(),
        params,
    }
}

/// Helper to create a notification
pub fn make_acp_notification(method: &str, params: Option<serde_json::Value>) -> AcpNotification {
    AcpNotification {
        jsonrpc: "2.0".to_string(),
        method: method.to_string(),
        params,
    }
}

/// Note: The actual method names for Gemini ACP should be verified against the binary.
/// Expected methods (client -> server):
/// - "initialize"
/// - "initialized"
/// - "agent/execute" or "textDocument/chat"
/// - "agent/cancel"
/// - "shutdown"
/// - "exit"
/// 
/// Expected notifications (server -> client):
/// - "agent/progress" (streaming)
/// - "agent/toolCall"
/// - "agent/approval"
/// - "agent/complete"

impl AcpMessage {
    pub fn parse(json: &str) -> serde_json::Result<Self> {
        let val: serde_json::Value = serde_json::from_str(json)?;
        
        if val.get("id").is_some() {
            if val.get("method").is_some() {
                Ok(AcpMessage::Request(serde_json::from_value(val)?))
            } else {
                Ok(AcpMessage::Response(serde_json::from_value(val)?))
            }
        } else if val.get("method").is_some() {
            Ok(AcpMessage::Notification(serde_json::from_value(val)?))
        } else {
            Ok(AcpMessage::Unknown(val))
        }
    }
}
