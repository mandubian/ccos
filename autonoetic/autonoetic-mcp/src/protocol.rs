//! Minimal MCP JSON-RPC protocol helpers.

use serde::{Deserialize, Serialize};

/// Generic JSON-RPC 2.0 request shape used by MCP.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

impl JsonRpcRequest {
    pub fn new(id: u64, method: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: serde_json::json!(id),
            method: method.into(),
            params,
        }
    }
}

/// Generic JSON-RPC 2.0 error envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
}

/// Generic JSON-RPC 2.0 response shape used by MCP.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    #[serde(default)]
    pub result: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn ok(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: serde_json::Value, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }

    pub fn into_result(self) -> anyhow::Result<serde_json::Value> {
        if let Some(err) = self.error {
            anyhow::bail!("MCP JSON-RPC error {}: {}", err.code, err.message);
        }
        self.result
            .ok_or_else(|| anyhow::anyhow!("MCP JSON-RPC response missing result"))
    }
}
