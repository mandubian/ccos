//! MCP Session Handler
//!
//! Provider-specific implementation of SessionHandler for MCP (Model Context Protocol).
//! Manages MCP session lifecycle: initialize, execute tools/call, terminate.

use super::session_pool::{SessionHandler, SessionId};
use crate::mcp::stdio_client::StdioClient;
use crate::utils::value_conversion;
use rtfs::ast::{Keyword, MapKey};
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
    stdio_client: Option<Arc<StdioClient>>,
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

    /// Get auth token from environment variable specified in metadata or inferred from capability ID
    fn get_auth_token(
        &self,
        capability_id: &str,
        metadata: &HashMap<String, String>,
    ) -> Option<String> {
        // Check for explicit token in metadata first (passed from executor)
        if let Some(token) = metadata.get("mcp_auth_token") {
            return Some(token.clone());
        }

        // Check for env var name specified in metadata
        if let Some(env_var) = metadata.get("mcp_auth_env_var") {
            if let Ok(token) = std::env::var(env_var) {
                return Some(token);
            }
        }

        // Try to get server name from metadata, or infer it from capability ID
        // Capability ID format: mcp.{server_name}.{tool_name} (e.g., mcp.github/github-mcp.list_issues)
        let server_name = metadata.get("mcp_server_name").cloned().or_else(|| {
            // Infer from capability_id: "mcp.github/github-mcp.list_issues" -> "github/github-mcp"
            if capability_id.starts_with("mcp.") {
                let rest = &capability_id[4..]; // Remove "mcp." prefix
                                                // Find the last dot which separates server_name from tool_name
                if let Some(last_dot) = rest.rfind('.') {
                    let server_name = &rest[..last_dot];
                    if !server_name.is_empty() {
                        return Some(server_name.to_string());
                    }
                }
            }
            None
        });

        // Fallback: try server-specific tokens (e.g., GITHUB_MCP_TOKEN, SLACK_MCP_TOKEN)
        // Pattern: {NAMESPACE}_MCP_TOKEN where namespace is extracted from server name
        if let Some(server_name) = server_name {
            let namespace = if let Some(slash_pos) = server_name.find('/') {
                &server_name[..slash_pos]
            } else {
                &server_name
            };
            let normalized = namespace.replace('-', "_").to_uppercase();
            let server_specific_var = format!("{}_MCP_TOKEN", normalized);

            if let Ok(token) = std::env::var(&server_specific_var) {
                log::debug!("Found auth token from {}", server_specific_var);
                return Some(token);
            }
        }

        // Final fallback: generic MCP_AUTH_TOKEN
        std::env::var("MCP_AUTH_TOKEN").ok()
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
    ) -> RuntimeResult<(String, Option<Arc<StdioClient>>)> {
        // Detect if this is a stdio transport
        if server_url.starts_with("npx ")
            || server_url.starts_with("node ")
            || server_url.starts_with("/")
            || server_url.starts_with("./")
        {
            let client = self.initialize_stdio_session(server_url)?;
            return Ok(("stdio".to_string(), Some(client)));
        }

        crate::ccos_println!("🔌 Initializing MCP session with {}", server_url);

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

        crate::ccos_println!("✅ MCP session initialized: {}", session_id);
        Ok((session_id, None))
    }

    /// Internal helper to initialize a stdio-based session
    fn initialize_stdio_session(&self, command: &str) -> RuntimeResult<Arc<StdioClient>> {
        crate::ccos_println!("🚀 Spawning stdio MCP server: {}", command);

        tokio::task::block_in_place(|| {
            let handle = tokio::runtime::Handle::current();
            handle.block_on(async {
                let client = StdioClient::spawn(command).await?;
                let client = Arc::new(client);

                let init_params = serde_json::json!({
                    "protocolVersion": "2024-11-05",
                    "clientInfo": {
                        "name": "ccos-rtfs",
                        "version": "0.1.0"
                    },
                    "capabilities": {}
                });

                let _ = client.make_request("initialize", init_params).await?;
                Ok(client)
            })
        })
    }

    /// Execute MCP tool call with session
    fn execute_mcp_call(
        &self,
        session: &MCPSession,
        tool_name: &str,
        args: &[Value],
    ) -> RuntimeResult<Value> {
        // Dispatch to stdio client if available
        if let Some(ref stdio) = session.stdio_client {
            return tokio::task::block_in_place(|| {
                let handle = tokio::runtime::Handle::current();
                handle.block_on(async {
                    let mcp_args = self.prepare_mcp_args(args)?;
                    let response = stdio
                        .make_request(
                            "tools/call",
                            serde_json::json!({
                                "name": tool_name,
                                "arguments": mcp_args
                            }),
                        )
                        .await?;

                    self.parse_mcp_response(response)
                })
            });
        }

        crate::ccos_println!(
            "🔧 Calling MCP tool: {} with session {}",
            tool_name, session.session_id
        );

        let mcp_args = self.prepare_mcp_args(args)?;

        crate::ccos_println!(
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

        let json = self.extract_json_from_sse(&body_text)?;
        self.parse_mcp_response(json)
    }

    /// Internal helper to prepare MCP arguments from RTFS values
    fn prepare_mcp_args(&self, args: &[Value]) -> RuntimeResult<serde_json::Value> {
        if args.is_empty() {
            Ok(serde_json::json!({}))
        } else if args.len() == 1 {
            match &args[0] {
                Value::Map(m) => {
                    let mut json_map = serde_json::Map::new();
                    for (key, value) in m.iter() {
                        let key_str = match key {
                            MapKey::Keyword(k) => {
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
                        let normalized_key = normalize_param_name(&key_str);
                        let json_val = value_conversion::rtfs_value_to_json(value)
                            .unwrap_or_else(|_| serde_json::Value::Null);
                        if !json_val.is_null() {
                            json_map.insert(normalized_key, json_val);
                        }
                    }
                    Ok(serde_json::Value::Object(json_map))
                }
                _ => Ok(value_conversion::rtfs_value_to_json(&args[0])
                    .unwrap_or_else(|_| serde_json::Value::Null)),
            }
        } else {
            Err(RuntimeError::Generic(
                "MCP tool call expects a single map argument".to_string(),
            ))
        }
    }

    /// Internal helper to extract JSON from SSE or raw response
    fn extract_json_from_sse(&self, body: &str) -> RuntimeResult<serde_json::Value> {
        fn internal_extract_sse(body: &str) -> Option<String> {
            let mut candidates: Vec<String> = Vec::new();
            let mut current = String::new();
            let mut in_data = false;
            for line in body.lines() {
                let trimmed = line.trim_end_matches(['\r', '\n']).trim_start();
                if let Some(rest) = trimmed.strip_prefix("data:") {
                    if !in_data {
                        current.clear();
                        in_data = true;
                    }
                    if !current.is_empty() {
                        current.push('\n');
                    }
                    current.push_str(rest.trim_start());
                    continue;
                }
                if trimmed.is_empty() || trimmed.starts_with("event:") {
                    if in_data && !current.is_empty() {
                        candidates.push(current.clone());
                        current.clear();
                    }
                    in_data = false;
                }
            }
            if in_data && !current.is_empty() {
                candidates.push(current);
            }
            for cand in candidates.into_iter().rev() {
                let trimmed = cand.trim_start();
                if (trimmed.starts_with('{') || trimmed.starts_with('['))
                    && serde_json::from_str::<serde_json::Value>(trimmed).is_ok()
                {
                    return Some(cand);
                }
            }
            None
        }

        match internal_extract_sse(body) {
            Some(data) => serde_json::from_str(&data).map_err(|e| {
                RuntimeError::Generic(format!(
                    "Failed to parse MCP response: {} (data: {})",
                    e,
                    &data[..data.len().min(200)]
                ))
            }),
            None => serde_json::from_str(body).map_err(|e| {
                RuntimeError::Generic(format!(
                    "Failed to parse MCP response: {} (body: {})",
                    e,
                    &body[..body.len().min(200)]
                ))
            }),
        }
    }

    /// Internal helper to parse MCP JSON-RPC response into RTFS Value
    fn parse_mcp_response(&self, json: serde_json::Value) -> RuntimeResult<Value> {
        if let Some(result) = json.get("result") {
            // Auto-extract JSON from content[0].text if available
            if let Some(content_array) = result.get("content").and_then(|c| c.as_array()) {
                if let Some(first_block) = content_array.first() {
                    if first_block.get("type").and_then(|t| t.as_str()) == Some("text") {
                        if let Some(text) = first_block.get("text").and_then(|t| t.as_str()) {
                            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(text) {
                                return value_conversion::json_to_rtfs_value(&parsed)
                                    .map(normalize_map_keys);
                            }
                        }
                    }
                }
            }
            value_conversion::json_to_rtfs_value(result).map(normalize_map_keys)
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
        crate::ccos_println!("🔚 Terminating MCP session: {}", session.session_id);

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
        // crate::ccos_println!("[MCPSessionHandler] initialize_session for {}: metadata keys={:?}", capability_id, metadata.keys());

        // Extract server URL from metadata (try multiple key variations)
        let server_url = metadata
            .get("server_url")
            .or_else(|| metadata.get("url"))
            .or_else(|| metadata.get("mcp_server_url"))
            .or_else(|| metadata.get("mcp_server_endpoint")); // RTFS exports use this key

        let server_url = match server_url {
            Some(url) => {
                // crate::ccos_println!("[MCPSessionHandler] Found server_url: {}", url);
                url.clone()
            }
            None => {
                // crate::ccos_println!("[MCPSessionHandler] Missing server_url in metadata: {:?}", metadata);
                return Err(RuntimeError::Generic(
                    "Missing server_url in metadata".to_string(),
                ));
            }
        };

        // Get auth token from environment variable specified in metadata or inferred from capability ID
        let auth_token = self.get_auth_token(capability_id, metadata);

        // Initialize MCP session
        let (session_id, stdio_client) =
            self.initialize_mcp_session(&server_url, auth_token.as_deref())?;

        // Store session in pool
        let session = MCPSession {
            session_id: session_id.clone(),
            server_url,
            auth_token,
            created_at: std::time::Instant::now(),
            stdio_client,
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
            crate::ccos_println!("♻️  Reusing existing MCP session: {}", session.session_id);
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

/// Normalize parameter names for known variations
///
/// This is a lightweight adapter that handles common naming mismatches between
/// LLM-generated parameter names and API-expected names. Related to issue #178
/// (Planner: lightweight adapters between capabilities).
///
/// Examples:
/// - "repository" → "repo" (GitHub API convention)
/// - "issue" → "issue_number" (when API expects number not object)
fn normalize_param_name(name: &str) -> String {
    match name {
        // GitHub API naming conventions
        "repository" | "name" => "repo".to_string(),
        "issue" => "issue_number".to_string(),
        // Skip internal params (handled separately)
        s if s.starts_with('_') => name.to_string(),
        // Pass through unchanged
        _ => name.to_string(),
    }
}

/// Normalize map keys in an RTFS value from string to keyword type.
///
/// This is an adapter that ensures MCP JSON responses (which have string keys)
/// can be accessed with RTFS keyword syntax like `(get result :number)`.
///
/// Recursively processes nested maps and lists.
fn normalize_map_keys(value: Value) -> Value {
    match value {
        Value::Map(map) => {
            let mut new_map = HashMap::new();
            for (key, val) in map.into_iter() {
                // Convert string keys to keyword keys
                let new_key = match key {
                    MapKey::String(s) => MapKey::Keyword(Keyword(s)),
                    other => other,
                };
                // Recursively normalize nested values
                new_map.insert(new_key, normalize_map_keys(val));
            }
            Value::Map(new_map)
        }
        Value::List(list) => Value::List(list.into_iter().map(normalize_map_keys).collect()),
        Value::Vector(vec) => Value::Vector(vec.into_iter().map(normalize_map_keys).collect()),
        // Other value types pass through unchanged
        other => other,
    }
}
