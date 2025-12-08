//! MCP Discovery Session Management
//!
//! Implements proper MCP session management for discovery operations according to the MCP specification:
//! https://modelcontextprotocol.io/specification/2025-03-26/basic/transports#session-management
//!
//! This module provides ephemeral, async session management for discovery/introspection.
//! For runtime execution sessions (with pooling), see `capabilities::mcp_session_handler`.
//!
//! ## Key Features
//!
//! 1. Initialize MCP session and obtain session ID
//! 2. Include session ID in all subsequent requests via Mcp-Session-Id header
//! 3. Handle session expiration (404) by re-initializing
//! 4. Support graceful session termination via DELETE
//!
//! ## Usage
//!
//! Used by the unified discovery service to establish temporary sessions
//! for querying MCP servers during capability discovery.

use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use serde::{Deserialize, Serialize};
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPServerInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
        println!("🔄 Initializing MCP session with {}", server_url);

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
            .header("Accept", "application/json")
            .header("Accept", "text/event-stream")
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
            println!("✅ Received session ID: {}...", &sid[..sid.len().min(20)]);
        } else {
            println!("ℹ️  No session ID provided (stateless mode)");
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
        let init_response: serde_json::Value = response.json().await.map_err(|e| {
            RuntimeError::Generic(format!("Failed to parse initialize response: {}", e))
        })?;

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

        println!("✅ MCP session initialized");
        println!("   Server: {} v{}", server_info.name, server_info.version);
        println!("   Protocol: {}", protocol_version);

        Ok(MCPSession {
            server_url: server_url.to_string(),
            session_id,
            protocol_version,
            server_info,
            capabilities,
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
            .header("Accept", "application/json")
            .header("Accept", "text/event-stream")
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
        serde_json::from_str(&body_text).map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to parse MCP response: {} (body: {})",
                e,
                &body_text[..body_text.len().min(200)]
            ))
        })
    }

    /// Terminate an MCP session gracefully
    ///
    /// Sends DELETE request with session ID as per spec
    pub async fn terminate_session(&self, session: &MCPSession) -> RuntimeResult<()> {
        if let Some(ref session_id) = session.session_id {
            println!("🔚 Terminating MCP session...");

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
                        println!("ℹ️  Server does not support explicit session termination");
                    } else if response.status().is_success() {
                        println!("✅ Session terminated successfully");
                    }
                }
                Err(e) => {
                    println!("⚠️  Failed to terminate session: {}", e);
                }
            }
        }

        Ok(())
    }
}
