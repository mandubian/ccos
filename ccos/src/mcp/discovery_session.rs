use crate::ccos_println;
use crate::mcp::stdio_client::StdioClient;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use std::collections::HashMap;
use std::sync::Arc;

/// MCP session information
#[derive(Debug, Clone)]
pub struct MCPSession {
    pub server_url: String,
    pub session_id: Option<String>,
    pub protocol_version: String,
    pub server_info: MCPServerInfo,
    pub capabilities: MCPCapabilities,
    pub stdio_client: Option<Arc<StdioClient>>,
}

#[derive(Debug, Clone, ::serde::Serialize, ::serde::Deserialize)]
pub struct MCPServerInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, ::serde::Serialize, ::serde::Deserialize, Default)]
pub struct MCPCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<serde_json::Value>,
}

/// MCP session manager handles initialization and session lifecycle
pub struct MCPSessionManager {
    client: Arc<reqwest::Client>,
    auth_headers: Option<HashMap<String, String>>,
}

impl MCPSessionManager {
    /// Create a new MCP session manager with a new HTTP client
    pub fn new(auth_headers: Option<HashMap<String, String>>) -> Self {
        Self {
            client: Arc::new(reqwest::Client::new()),
            auth_headers,
        }
    }

    /// Create a new MCP session manager with a shared HTTP client (for connection pooling)
    pub fn with_client(
        client: Arc<reqwest::Client>,
        auth_headers: Option<HashMap<String, String>>,
    ) -> Self {
        Self {
            client,
            auth_headers,
        }
    }

    /// Initialize an MCP session with the server
    ///
    /// According to the MCP spec, this:
    /// 1. Sends an initialize request to the server
    /// 2. Receives InitializeResult with optional Mcp-Session-Id header
    /// 3. Stores the session ID for use in subsequent requests
    pub async fn initialize_session(
        &self,
        server_url: &str,
        client_info: &MCPServerInfo,
    ) -> RuntimeResult<MCPSession> {
        ccos_println!("🔄 Initializing MCP session with {}", server_url);

        let trimmed = server_url.trim();
        // Detect if this is a stdio transport (command line instead of URL)
        let is_stdio = trimmed.starts_with("npx ")
            || trimmed.starts_with("node ")
            || trimmed.starts_with("python ")
            || trimmed.starts_with("python3 ")
            || trimmed.starts_with("/")
            || trimmed.starts_with("./")
            || trimmed.starts_with("sh ")
            || trimmed.starts_with("bash ")
            || (!trimmed.contains("://") && !trimmed.is_empty());

        if is_stdio {
            ccos_println!("🔍 Detected stdio transport for: {}", trimmed);
            return self.initialize_stdio_session(trimmed, client_info).await;
        }

        // Build initialize request according to MCP spec
        let init_request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "init_1",
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "clientInfo": {
                    "name": client_info.name,
                    "version": client_info.version
                },
                "capabilities": {}
            }
        });

        // Send POST request with Accept header as required by spec
        let mut request = self
            .client
            .post(server_url)
            .json(&init_request)
            .header("Accept", "application/json, text/event-stream")
            .timeout(std::time::Duration::from_secs(30));

        // Add authentication headers if provided
        if let Some(headers) = &self.auth_headers {
            for (key, value) in headers {
                request = request.header(key, value);
            }
        }

        let response = request.send().await.map_err(|e| {
            RuntimeError::Generic(format!("Failed to connect to MCP server: {}", e))
        })?;

        // Extract session ID from Mcp-Session-Id header if present
        let session_id = response
            .headers()
            .get("Mcp-Session-Id")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        if let Some(ref sid) = session_id {
            ccos_println!("✅ Received session ID: {}...", &sid[..sid.len().min(20)]);
        } else {
            ccos_println!("ℹ️  No session ID provided (stateless mode)");
        }

        // Check response status
        let status = response.status();

        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unable to read error response".to_string());

            // Provide helpful error messages for common auth failures
            let error_msg = if status.as_u16() == 401 {
                format!(
                    "MCP initialization failed (401 Unauthorized): {}\n\
                     💡 Authentication failed. Please verify:\n\
                     • Token is valid and not expired\n\
                     • Token has required permissions/scopes\n\
                     • Environment variable is set correctly (MCP_AUTH_TOKEN)",
                    error_text
                )
            } else {
                format!("MCP initialization failed ({}): {}", status, error_text)
            };

            return Err(RuntimeError::Generic(error_msg));
        }

        // Parse initialize result
        let body_text = response.text().await.map_err(|e| {
            RuntimeError::Generic(format!("Failed to read initialize response body: {}", e))
        })?;

        // Try parsing as raw JSON first (even if content-type says SSE)
        let init_response: serde_json::Value = match serde_json::from_str(&body_text) {
            Ok(v) => v,
            Err(_) => {
                // If raw JSON fails, try SSE extraction
                if let Some(data) = extract_sse_data(&body_text) {
                    serde_json::from_str(&data).map_err(|e| {
                        RuntimeError::Generic(format!(
                            "Failed to parse SSE initialization data: {}\n\nRaw body: {}\n\nExtracted data: {}",
                            e, body_text, data
                        ))
                    })?
                } else {
                    return Err(RuntimeError::Generic(format!(
                        "Failed to parse initialize response as JSON and no SSE data found.\n\nRaw response body: {}",
                        body_text
                    )));
                }
            }
        };

        // Extract server info and capabilities from result
        let result = init_response.get("result").ok_or_else(|| {
            RuntimeError::Generic("Initialize response missing result".to_string())
        })?;

        let server_info = MCPServerInfo {
            name: result
                .get("serverInfo")
                .and_then(|s| s.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("unknown")
                .to_string(),
            version: result
                .get("serverInfo")
                .and_then(|s| s.get("version"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
        };

        let capabilities: MCPCapabilities = result
            .get("capabilities")
            .and_then(|c| serde_json::from_value(c.clone()).ok())
            .unwrap_or_default();

        let protocol_version = result
            .get("protocolVersion")
            .and_then(|v| v.as_str())
            .unwrap_or("2024-11-05")
            .to_string();

        ccos_println!("✅ MCP session initialized");
        ccos_println!("   Server: {} v{}", server_info.name, server_info.version);
        ccos_println!("   Protocol: {}", protocol_version);

        Ok(MCPSession {
            server_url: server_url.to_string(),
            session_id,
            protocol_version,
            server_info,
            capabilities,
            stdio_client: None,
        })
    }

    /// Internal helper to initialize a stdio-based session
    async fn initialize_stdio_session(
        &self,
        command: &str,
        client_info: &MCPServerInfo,
    ) -> RuntimeResult<MCPSession> {
        ccos_println!("🚀 Spawning stdio MCP server: {}", command);

        let client = StdioClient::spawn(command).await?;
        let client = Arc::new(client);

        let init_params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "clientInfo": {
                "name": client_info.name,
                "version": client_info.version
            },
            "capabilities": {}
        });

        let response = client.make_request("initialize", init_params).await?;

        let result = response.get("result").ok_or_else(|| {
            RuntimeError::Generic("Stdio Initialize response missing result".to_string())
        })?;

        let server_info = MCPServerInfo {
            name: result
                .get("serverInfo")
                .and_then(|s| s.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("unknown")
                .to_string(),
            version: result
                .get("serverInfo")
                .and_then(|s| s.get("version"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
        };

        let capabilities: MCPCapabilities = result
            .get("capabilities")
            .and_then(|c| serde_json::from_value(c.clone()).ok())
            .unwrap_or_default();

        let protocol_version = result
            .get("protocolVersion")
            .and_then(|v| v.as_str())
            .unwrap_or("2024-11-05")
            .to_string();

        // Stdio is stateless in terms of headers, but we might still have a session ID
        // (though usually not for stdio)
        let session_id = None;

        ccos_println!("✅ Stdio MCP session initialized");
        Ok(MCPSession {
            server_url: command.to_string(),
            session_id,
            protocol_version,
            server_info,
            capabilities,
            stdio_client: Some(client),
        })
    }

    /// Make a request to an MCP server using an established session
    ///
    /// Includes the Mcp-Session-Id header if a session ID is present
    pub async fn make_request(
        &self,
        session: &MCPSession,
        method: &str,
        params: serde_json::Value,
    ) -> RuntimeResult<serde_json::Value> {
        // Dispatch to stdio client if available
        if let Some(ref stdio) = session.stdio_client {
            return stdio.make_request(method, params).await;
        }

        let request_payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": format!("{}_{}", method, uuid::Uuid::new_v4()),
            "method": method,
            "params": params
        });

        let mut request = self
            .client
            .post(&session.server_url)
            .json(&request_payload)
            .header("Accept", "application/json, text/event-stream")
            .timeout(std::time::Duration::from_secs(30));

        // Add session ID header if present (required by spec for stateful sessions)
        if let Some(ref session_id) = session.session_id {
            request = request.header("Mcp-Session-Id", session_id);
        }

        // Add authentication headers if provided
        if let Some(headers) = &self.auth_headers {
            for (key, value) in headers {
                request = request.header(key, value);
            }
        }

        let response = request
            .send()
            .await
            .map_err(|e| RuntimeError::Generic(format!("MCP request failed: {}", e)))?;

        let status = response.status();
        let _content_type = response
            .headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("")
            .to_string();

        // Handle session expiration (404) according to spec
        if status == reqwest::StatusCode::NOT_FOUND && session.session_id.is_some() {
            return Err(RuntimeError::Generic(
                "Session expired (404). Client should re-initialize.".to_string(),
            ));
        }

        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unable to read error response".to_string());
            return Err(RuntimeError::Generic(format!(
                "MCP server returned error ({}): {}",
                status, error_text
            )));
        }

        let body_text = response
            .text()
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to read MCP response: {}", e)))?;

        // Try parsing as raw JSON first
        match serde_json::from_str(&body_text) {
            Ok(v) => Ok(v),
            Err(_) => {
                // If raw JSON fails, try SSE extraction
                if let Some(data) = extract_sse_data(&body_text) {
                    serde_json::from_str(&data).map_err(|e| {
                        RuntimeError::Generic(format!(
                            "Failed to parse SSE data: {}\n\nRaw body: {}\n\nExtracted data: {}",
                            e, body_text, data
                        ))
                    })
                } else {
                    Err(RuntimeError::Generic(format!(
                        "Failed to parse MCP response as JSON and no SSE data found.\n\nRaw response body: {}",
                        body_text
                    )))
                }
            }
        }
    }

    /// Terminate an MCP session gracefully
    ///
    /// Sends DELETE request with session ID as per spec
    pub async fn terminate_session(&self, session: &MCPSession) -> RuntimeResult<()> {
        if let Some(ref session_id) = session.session_id {
            ccos_println!("🔚 Terminating MCP session...");

            let mut request = self
                .client
                .delete(&session.server_url)
                .header("Mcp-Session-Id", session_id)
                .timeout(std::time::Duration::from_secs(10));

            // Add authentication headers if provided
            if let Some(headers) = &self.auth_headers {
                for (key, value) in headers {
                    request = request.header(key, value);
                }
            }

            match request.send().await {
                Ok(response) => {
                    if response.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED {
                        ccos_println!("ℹ️  Server does not support explicit session termination");
                    } else if response.status().is_success() {
                        ccos_println!("✅ Session terminated successfully");
                    }
                }
                Err(e) => {
                    ccos_println!("⚠️  Failed to terminate session: {}", e);
                }
            }
        }

        Ok(())
    }
}

/// Helper to extract last `data: ...` JSON chunk from SSE-like bodies, including multi-line data
fn extract_sse_data(body: &str) -> Option<String> {
    let mut candidates: Vec<String> = Vec::new();
    let mut collecting = false;
    let mut buf = String::new();
    for line in body.lines() {
        let line = line.trim_end_matches(['\r', '\n']);

        // Handle "data: " or "data:" prefix
        let data_opt = if line.starts_with("data: ") {
            Some(&line[6..])
        } else if line.starts_with("data:") {
            Some(&line[5..])
        } else {
            None
        };

        if let Some(data) = data_opt {
            // Start new data block
            buf.clear();
            buf.push_str(data.trim());
            collecting = true;
            continue;
        }
        if collecting {
            // Accumulate until blank line ends the event
            if line.trim().is_empty() {
                candidates.push(buf.clone());
                collecting = false;
            } else {
                buf.push_str(line.trim());
            }
        }
    }
    if collecting && !buf.is_empty() {
        candidates.push(buf);
    }
    // Prefer the last JSON-looking candidate (most likely the final result)
    candidates
        .into_iter()
        .rev()
        .find(|s| s.trim_start().starts_with('{') || s.trim_start().starts_with('['))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct McpToolCallResult {
    pub content: Vec<McpContent>,
    #[serde(default, rename = "isError")]
    pub is_error: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct McpContent {
    pub r#type: String,
    pub text: Option<String>,
}

impl MCPSessionManager {
    /// Call an MCP tool
    pub async fn call_tool(
        &self,
        session: &MCPSession,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> RuntimeResult<McpToolCallResult> {
        let params = serde_json::json!({
            "name": tool_name,
            "arguments": arguments
        });

        // This returns the full JSON-RPC response
        let response = self.make_request(session, "tools/call", params).await?;

        // Check for JSON-RPC error
        if let Some(error) = response.get("error") {
            let msg = error
                .get("message")
                .and_then(|s| s.as_str())
                .unwrap_or("Unknown error");
            return Err(RuntimeError::Generic(format!(
                "Tool execution error: {}",
                msg
            )));
        }

        let result = response.get("result").ok_or_else(|| {
            RuntimeError::Generic("Tool call response missing 'result' field".to_string())
        })?;

        serde_json::from_value(result.clone())
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse tool call result: {}", e)))
    }
}
