//! MCP Session Handler
//!
//! Provider-specific implementation of SessionHandler for MCP (Model Context Protocol).
//! Manages MCP session lifecycle: initialize, execute tools/call, terminate.

use super::session_pool::{SessionHandler, SessionId};
use crate::utils::value_conversion;
use rtfs::ast::MapKey;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// MCP Session data
#[derive(Clone, Debug)]
struct MCPSession {
    session_id: String,
    server_url: String,
    auth_token: Option<String>,
    created_at: std::time::Instant,
}

/// MCP Session Handler
///
/// Implements the SessionHandler trait for MCP servers.
/// Manages session pool, auth injection, and MCP protocol specifics.
pub struct MCPSessionHandler {
    /// Session pool: capability_id → session
    /// Simple 1:1 mapping for MVP, can be extended to pools per server
    sessions: Arc<Mutex<HashMap<String, MCPSession>>>,
    /// HTTP client for making MCP requests (async client; calls wrapped via block_in_place)
    http_client: reqwest::Client,
}

impl MCPSessionHandler {
    /// Create a new MCP session handler
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            http_client: reqwest::Client::new(),
        }
    }

    /// Get auth token from environment variable specified in metadata
    fn get_auth_token(&self, metadata: &HashMap<String, String>) -> Option<String> {
        // Check for explicit token in metadata first (passed from executor)
        if let Some(token) = metadata.get("mcp_auth_token") {
            return Some(token.clone());
        }

        // Fallback to env var lookup
        let env_var = metadata.get("mcp_auth_env_var")?;
        std::env::var(env_var).ok()
    }

    /// Get server URL from metadata (with fallback to env var)
    fn get_server_url(&self, metadata: &HashMap<String, String>) -> RuntimeResult<String> {
        // Priority: metadata → env var override → error
        if let Some(url_from_meta) = metadata.get("mcp_server_url") {
            // Check for environment variable override
            if let Some(override_env) = metadata.get("mcp_server_url_override_env") {
                if let Ok(url_from_env) = std::env::var(override_env) {
                    return Ok(url_from_env);
                }
            }
            Ok(url_from_meta.clone())
        } else {
            Err(RuntimeError::Generic(
                "Missing mcp_server_url in metadata".to_string(),
            ))
        }
    }

    /// Initialize MCP session (calls initialize endpoint)
    fn initialize_mcp_session(
        &self,
        server_url: &str,
        auth_token: Option<&str>,
    ) -> RuntimeResult<String> {
        eprintln!("🔌 Initializing MCP session with {}", server_url);

        // Build initialize request
        let init_request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "init",
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "clientInfo": {
                    "name": "ccos-rtfs",
                    "version": "0.1.0"
                },
                "capabilities": {}
            }
        });

        // Build headers
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("Content-Type", "application/json".parse().unwrap());
        if let Some(token) = auth_token {
            // Use token as-is; env should include scheme if required
            headers.insert("Authorization", token.parse().unwrap());
        }

        // Make request (wrap async send in a blocking section without creating a nested runtime)
        let response = tokio::task::block_in_place(|| {
            let handle = tokio::runtime::Handle::current();
            handle.block_on(async {
                self.http_client
                    .post(server_url)
                    .headers(headers)
                    .json(&init_request)
                    .send()
                    .await
                    .map_err(|e| {
                        RuntimeError::Generic(format!("MCP initialize request failed: {}", e))
                    })
            })
        })?;

        // Check status
        if !response.status().is_success() {
            let status = response.status();
            let body = tokio::task::block_in_place(|| {
                let handle = tokio::runtime::Handle::current();
                handle.block_on(async {
                    response
                        .text()
                        .await
                        .unwrap_or_else(|_| "could not read body".to_string())
                })
            });
            return Err(RuntimeError::Generic(format!(
                "MCP initialize failed ({} {}): {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("Unknown"),
                body
            )));
        }

        // Extract session ID from header
        let session_id = response
            .headers()
            .get("Mcp-Session-Id")
            .and_then(|h| h.to_str().ok())
            .ok_or_else(|| {
                RuntimeError::Generic("No Mcp-Session-Id in initialize response".to_string())
            })?
            .to_string();

        eprintln!("✅ MCP session initialized: {}", session_id);
        Ok(session_id)
    }

    /// Execute MCP tool call with session
    fn execute_mcp_call(
        &self,
        session: &MCPSession,
        tool_name: &str,
        args: &[Value],
    ) -> RuntimeResult<Value> {
        eprintln!(
            "🔧 Calling MCP tool: {} with session {}",
            tool_name, session.session_id
        );

        // Convert args to MCP arguments (expect a single map)
        let mcp_args = if args.is_empty() {
            serde_json::json!({})
        } else if args.len() == 1 {
            match &args[0] {
                Value::Map(m) => {
                    // Convert RTFS map to JSON
                    let mut json_map = serde_json::Map::new();
                    for (key, value) in m.iter() {
                        let key_str = match key {
                            MapKey::Keyword(k) => {
                                // Strip leading colon
                                let s = &k.0;
                                if s.starts_with(':') {
                                    s[1..].to_string()
                                } else {
                                    s.clone()
                                }
                            }
                            MapKey::String(s) => s.clone(),
                            MapKey::Integer(i) => i.to_string(),
                        };

                        // Convert value to JSON
                        let json_val = value_conversion::rtfs_value_to_json(value)
                            .unwrap_or_else(|_| serde_json::Value::Null);

                        // Filter out null values for MCP arguments
                        // This handles cases where optional parameters are passed as nil (null)
                        // but the MCP server expects them to be omitted if not present.
                        if !json_val.is_null() {
                            json_map.insert(key_str, json_val);
                        }
                    }
                    serde_json::Value::Object(json_map)
                }
                _ => value_conversion::rtfs_value_to_json(&args[0])
                    .unwrap_or_else(|_| serde_json::Value::Null),
            }
        } else {
            return Err(RuntimeError::Generic(
                "MCP tool call expects a single map argument".to_string(),
            ));
        };

        eprintln!(
            "📝 MCP Arguments: {}",
            serde_json::to_string_pretty(&mcp_args).unwrap_or_default()
        );

        // Build MCP JSON-RPC request
        let mcp_request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "tool_call",
            "method": "tools/call",
            "params": {
                "name": tool_name,
                "arguments": mcp_args
            }
        });

        // Build headers
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("Content-Type", "application/json".parse().unwrap());
        headers.insert("Mcp-Session-Id", session.session_id.parse().unwrap());
        if let Some(ref token) = session.auth_token {
            // Use token as-is; env should include scheme if required
            headers.insert("Authorization", token.parse().unwrap());
        }

        // Make request (wrap async send in a blocking section without creating a nested runtime)
        let response = tokio::task::block_in_place(|| {
            let handle = tokio::runtime::Handle::current();
            handle.block_on(async {
                self.http_client
                    .post(&session.server_url)
                    .headers(headers)
                    .json(&mcp_request)
                    .send()
                    .await
                    .map_err(|e| {
                        RuntimeError::Generic(format!("MCP tool call request failed: {}", e))
                    })
            })
        })?;

        // Check status
        if !response.status().is_success() {
            let status = response.status();
            let body = tokio::task::block_in_place(|| {
                let handle = tokio::runtime::Handle::current();
                handle.block_on(async {
                    response
                        .text()
                        .await
                        .unwrap_or_else(|_| "could not read body".to_string())
                })
            });
            return Err(RuntimeError::Generic(format!(
                "MCP tool call failed ({} {}): {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("Unknown"),
                body
            )));
        }

        // Parse response
        let body_text = tokio::task::block_in_place(|| {
            let handle = tokio::runtime::Handle::current();
            handle.block_on(async {
                response.text().await.map_err(|e| {
                    RuntimeError::Generic(format!("Failed to read response body: {}", e))
                })
            })
        })?;

        let json: serde_json::Value = serde_json::from_str(&body_text)
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse MCP response: {}", e)))?;

        // Extract result from JSON-RPC response
        if let Some(result) = json.get("result") {
            // Debug: Log the keys of the result for troubleshooting
            if let Some(content) = result.get("content") {
                if let Some(arr) = content.as_array() {
                    for (i, item) in arr.iter().enumerate() {
                        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                            let preview = if text.len() > 100 { &text[..100] } else { text };
                            eprintln!(
                                "📦 MCP result content[{}]: type={}, text (preview)='{}...'",
                                i,
                                item.get("type").and_then(|t| t.as_str()).unwrap_or("?"),
                                preview
                            );
                            // Try to parse JSON to see structure
                            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(text) {
                                if let Some(obj) = parsed.as_object() {
                                    eprintln!(
                                        "   ↳ Parsed JSON keys: {:?}",
                                        obj.keys().collect::<Vec<_>>()
                                    );
                                } else if parsed.is_array() {
                                    eprintln!(
                                        "   ↳ Parsed JSON is an Array of len {}",
                                        parsed.as_array().unwrap().len()
                                    );
                                }
                            }
                        }
                    }
                }
            }

            // MCP protocol returns results in a "content" array with text blocks.
            // We should return the raw result structure so downstream adapters can handle it
            // instead of trying to be too smart and unwrapping it automatically.
            // The adapter `adapters.mcp.parse-json-from-text-content` expects the raw structure
            // with a "content" array containing text blocks.

            // Fallback: Check for structuredContent field (if MCP server provides it)
            // Note: The MCP spec allows structuredContent, but most tools return content array.
            // We preserve the original structure unless structuredContent is explicitly present.

            // Default: Convert JSON to RTFS Value as-is, preserving 'content' or 'structuredContent' keys
            value_conversion::json_to_rtfs_value(result)
        } else if let Some(error) = json.get("error") {
            Err(RuntimeError::Generic(format!("MCP error: {}", error)))
        } else {
            Err(RuntimeError::Generic(
                "MCP response missing 'result' and 'error'".to_string(),
            ))
        }
    }

    /// Terminate MCP session (optional cleanup)
    fn terminate_mcp_session(&self, session: &MCPSession) -> RuntimeResult<()> {
        eprintln!("🔚 Terminating MCP session: {}", session.session_id);

        // MCP doesn't have a standard terminate endpoint in the spec,
        // but we can optionally send one if the server supports it.
        // For now, just remove from pool (graceful degradation).
        Ok(())
    }
}

impl SessionHandler for MCPSessionHandler {
    fn initialize_session(
        &self,
        capability_id: &str,
        metadata: &HashMap<String, String>,
    ) -> RuntimeResult<SessionId> {
        // eprintln!("[MCPSessionHandler] initialize_session for {}: metadata keys={:?}", capability_id, metadata.keys());

        // Extract server URL from metadata
        let server_url = metadata
            .get("server_url")
            .or_else(|| metadata.get("url"))
            .or_else(|| metadata.get("mcp_server_url")); // Added fallback to mcp_server_url

        let server_url = match server_url {
            Some(url) => {
                // eprintln!("[MCPSessionHandler] Found server_url: {}", url);
                url.clone()
            }
            None => {
                // eprintln!("[MCPSessionHandler] Missing server_url in metadata: {:?}", metadata);
                return Err(RuntimeError::Generic(
                    "Missing server_url in metadata".to_string(),
                ));
            }
        };

        // Get auth token from environment variable specified in metadata
        let auth_token = self.get_auth_token(metadata);

        // Initialize MCP session
        let session_id = self.initialize_mcp_session(&server_url, auth_token.as_deref())?;

        // Store session in pool
        let session = MCPSession {
            session_id: session_id.clone(),
            server_url,
            auth_token,
            created_at: std::time::Instant::now(),
        };

        let mut sessions = self.sessions.lock().unwrap();
        sessions.insert(capability_id.to_string(), session);

        Ok(session_id)
    }

    fn execute_with_session(
        &self,
        session_id: &SessionId,
        capability_id: &str,
        args: &[Value],
    ) -> RuntimeResult<Value> {
        // Get session from pool
        let sessions = self.sessions.lock().unwrap();
        let session = sessions
            .get(capability_id)
            .ok_or_else(|| {
                RuntimeError::Generic(format!("No MCP session found for {}", capability_id))
            })?
            .clone(); // Clone to release lock
        drop(sessions);

        // Verify session ID matches
        if session.session_id != *session_id {
            return Err(RuntimeError::Generic(format!(
                "Session ID mismatch: expected {}, got {}",
                session.session_id, session_id
            )));
        }

        // Extract tool name from capability_id (e.g., "mcp.github.list_issues" → "list_issues")
        let tool_name = capability_id.split('.').last().ok_or_else(|| {
            RuntimeError::Generic(format!("Invalid MCP capability ID: {}", capability_id))
        })?;

        // Execute MCP call
        self.execute_mcp_call(&session, tool_name, args)
    }

    fn terminate_session(&self, session_id: &SessionId) -> RuntimeResult<()> {
        // Find and remove session from pool
        let mut sessions = self.sessions.lock().unwrap();
        let capability_id = sessions
            .iter()
            .find(|(_, s)| s.session_id == *session_id)
            .map(|(k, _)| k.clone());

        if let Some(cap_id) = capability_id {
            if let Some(session) = sessions.remove(&cap_id) {
                drop(sessions); // Release lock before calling terminate
                return self.terminate_mcp_session(&session);
            }
        }

        Err(RuntimeError::Generic(format!(
            "Session not found: {}",
            session_id
        )))
    }

    fn get_or_create_session(
        &self,
        capability_id: &str,
        metadata: &HashMap<String, String>,
    ) -> RuntimeResult<SessionId> {
        // Check if session already exists
        let sessions = self.sessions.lock().unwrap();
        if let Some(session) = sessions.get(capability_id) {
            eprintln!("♻️  Reusing existing MCP session: {}", session.session_id);
            return Ok(session.session_id.clone());
        }
        drop(sessions);

        // Create new session
        self.initialize_session(capability_id, metadata)
    }
}

impl Default for MCPSessionHandler {
    fn default() -> Self {
        Self::new()
    }
}
