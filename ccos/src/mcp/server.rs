//! MCP Server Implementation
//!
//! Provides the core infrastructure for running CCOS as an MCP server over stdio.
//! Implements JSON-RPC 2.0 protocol for MCP communication.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use rtfs::runtime::error::{RuntimeError, RuntimeResult};

/// Type alias for async tool handler functions
pub type ToolHandler =
    Box<dyn Fn(Value) -> Pin<Box<dyn Future<Output = RuntimeResult<Value>> + Send>> + Send + Sync>;

/// MCP JSON-RPC Request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPRequest {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// MCP JSON-RPC Response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPResponse {
    pub jsonrpc: String,
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<MCPError>,
}

/// MCP JSON-RPC Error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Tool definition for MCP
#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

/// MCP Server
pub struct MCPServer {
    name: String,
    version: String,
    tools: HashMap<String, (ToolDefinition, Arc<ToolHandler>)>,
}

impl MCPServer {
    pub fn new(name: &str, version: &str) -> Self {
        Self {
            name: name.to_string(),
            version: version.to_string(),
            tools: HashMap::new(),
        }
    }

    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    /// Get all tool definitions
    pub fn get_tools(&self) -> Vec<&ToolDefinition> {
        self.tools.values().map(|(def, _)| def).collect()
    }

    /// Call a tool by name with arguments
    pub async fn call_tool(&self, name: &str, arguments: Value) -> RuntimeResult<Value> {
        let (_, handler) = self
            .tools
            .get(name)
            .ok_or_else(|| RuntimeError::Generic(format!("Unknown tool: {}", name)))?;

        handler(arguments).await
    }

    /// Register a tool with its handler
    pub fn register_tool(
        &mut self,
        name: &str,
        description: &str,
        input_schema: Value,
        handler: ToolHandler,
    ) {
        let definition = ToolDefinition {
            name: name.to_string(),
            description: description.to_string(),
            input_schema,
        };
        self.tools
            .insert(name.to_string(), (definition, Arc::new(handler)));
    }

    /// Run the MCP server over stdio
    pub async fn run_stdio(&self) -> RuntimeResult<()> {
        let stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        let mut reader = BufReader::new(stdin);
        let mut line = String::new();

        loop {
            line.clear();
            let bytes_read = reader
                .read_line(&mut line)
                .await
                .map_err(|e| RuntimeError::Generic(format!("IO error: {}", e)))?;

            if bytes_read == 0 {
                // EOF - client disconnected
                break;
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Parse JSON-RPC request
            let response = match serde_json::from_str::<MCPRequest>(trimmed) {
                Ok(request) => self.handle_request(request).await,
                Err(e) => MCPResponse {
                    jsonrpc: "2.0".to_string(),
                    id: None,
                    result: None,
                    error: Some(MCPError {
                        code: -32700,
                        message: format!("Parse error: {}", e),
                        data: None,
                    }),
                },
            };

            // Write response
            let response_json = serde_json::to_string(&response)
                .map_err(|e| RuntimeError::Generic(format!("Serialize error: {}", e)))?;

            stdout
                .write_all(response_json.as_bytes())
                .await
                .map_err(|e| RuntimeError::Generic(format!("IO error: {}", e)))?;
            stdout
                .write_all(b"\n")
                .await
                .map_err(|e| RuntimeError::Generic(format!("IO error: {}", e)))?;
            stdout
                .flush()
                .await
                .map_err(|e| RuntimeError::Generic(format!("IO error: {}", e)))?;
        }

        Ok(())
    }

    /// Handle a single MCP request
    async fn handle_request(&self, request: MCPRequest) -> MCPResponse {
        let result = match request.method.as_str() {
            "initialize" => self.handle_initialize(&request.params),
            "tools/list" => self.handle_tools_list(),
            "tools/call" => self.handle_tools_call(&request.params).await,
            "ping" => Ok(json!({ "pong": true })),
            _ => Err(RuntimeError::Generic(format!(
                "Method not found: {}",
                request.method
            ))),
        };

        match result {
            Ok(result) => MCPResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id,
                result: Some(result),
                error: None,
            },
            Err(e) => MCPResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id,
                result: None,
                error: Some(MCPError {
                    code: -32603,
                    message: e.to_string(),
                    data: None,
                }),
            },
        }
    }

    fn handle_initialize(&self, _params: &Value) -> RuntimeResult<Value> {
        Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": self.name,
                "version": self.version
            }
        }))
    }

    fn handle_tools_list(&self) -> RuntimeResult<Value> {
        let tools: Vec<&ToolDefinition> = self.tools.values().map(|(def, _)| def).collect();

        Ok(json!({
            "tools": tools
        }))
    }

    async fn handle_tools_call(&self, params: &Value) -> RuntimeResult<Value> {
        let tool_name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RuntimeError::Generic("Missing tool name".to_string()))?;

        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        let (_, handler) = self
            .tools
            .get(tool_name)
            .ok_or_else(|| RuntimeError::Generic(format!("Unknown tool: {}", tool_name)))?;

        let result = handler(arguments).await?;

        Ok(json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result).unwrap_or_default()
            }]
        }))
    }
}
