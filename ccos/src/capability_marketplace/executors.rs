use super::types::*;
use crate::secrets::SecretStore;
use crate::utils::fs::get_workspace_root;
use crate::utils::log_redaction::{redact_json_for_logs, redact_text_for_logs};
use crate::utils::value_conversion::{self, json_to_rtfs_value};
use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as Base64Engine;
use base64::Engine as _;
use futures::StreamExt;
use regex::Regex;
use reqwest;
use reqwest_eventsource::{Event, EventSource};
use rtfs::ast::MapKey;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use serde::de::DeserializeOwned;
use serde_json::json;
use std::any::TypeId;
use std::collections::HashMap;
use std::collections::HashSet;

lazy_static::lazy_static! {
    static ref SECRET_INJECTION: RwLock<HashMap<String, String>> = RwLock::new(HashMap::new());
}
use std::net::IpAddr;
use std::time::Duration;
use urlencoding::encode;

use crate::capabilities::native_provider::NativeCapabilityProvider;
use crate::capabilities::SessionPoolManager;
use crate::sandbox::{ResourceLimits, ResourceMetrics, VirtualFilesystem};
use rtfs::runtime::microvm::{Program, ScriptLanguage};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Execution context passed to all capability executors.
/// Provides access to metadata, session management, and capability identification.
pub struct ExecutionContext<'a> {
    /// Full capability ID (e.g., "mcp.github/github-mcp.list_issues")
    pub capability_id: &'a str,
    /// Metadata from the capability manifest
    pub metadata: &'a HashMap<String, String>,
    /// Optional session pool for session-managed execution
    pub session_pool: Option<Arc<SessionPoolManager>>,
}

impl<'a> ExecutionContext<'a> {
    pub fn new(
        capability_id: &'a str,
        metadata: &'a HashMap<String, String>,
        session_pool: Option<Arc<SessionPoolManager>>,
    ) -> Self {
        Self {
            capability_id,
            metadata,
            session_pool,
        }
    }
}

#[async_trait]
pub trait CapabilityExecutor: Send + Sync {
    fn provider_type_id(&self) -> TypeId;
    async fn execute(
        &self,
        provider: &ProviderType,
        inputs: &Value,
        context: &ExecutionContext<'_>,
    ) -> RuntimeResult<Value>;
}

pub struct MCPExecutor {
    pub session_pool: Arc<RwLock<Option<Arc<SessionPoolManager>>>>,
}

#[async_trait]
impl CapabilityExecutor for MCPExecutor {
    fn provider_type_id(&self) -> TypeId {
        TypeId::of::<MCPCapability>()
    }
    async fn execute(
        &self,
        provider: &ProviderType,
        inputs: &Value,
        context: &ExecutionContext<'_>,
    ) -> RuntimeResult<Value> {
        if let ProviderType::MCP(mcp) = provider {
            // Use session pool from context if available
            if let Some(pool) = &context.session_pool {
                // Use context metadata which includes mcp_server_endpoint, mcp_server_name, etc.
                let args = vec![inputs.clone()];
                return pool.execute_with_session(context.capability_id, context.metadata, &args);
            }

            // Fallback: Resolve auth token from inputs > provider > context metadata > env
            let auth_token_from_inputs = if let Value::Map(map) = inputs {
                map.get(&MapKey::Keyword(rtfs::ast::Keyword(
                    "auth-token".to_string(),
                )))
                .or_else(|| {
                    map.get(&MapKey::Keyword(rtfs::ast::Keyword(
                        "auth_token".to_string(),
                    )))
                })
                .and_then(|v| {
                    if let Value::String(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
            } else {
                None
            };

            let auth_token = auth_token_from_inputs
                .or_else(|| mcp.auth_token.clone())
                .or_else(|| context.metadata.get("mcp_auth_token").cloned())
                .or_else(|| std::env::var("MCP_AUTH_TOKEN").ok());

            // No session pool - do direct MCP execution with initialization
            // Convert RTFS Value to JSON, preserving string/keyword map keys
            let input_json = A2AExecutor::value_to_json(inputs)
                .map_err(|e| RuntimeError::Generic(format!("Failed to serialize inputs: {}", e)))?;

            let client = reqwest::Client::new();

            // Step 1: Initialize MCP session
            let init_request = json!({
                "jsonrpc": "2.0",
                "id": "init",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {
                        "name": "ccos-mcp-executor",
                        "version": "1.0.0"
                    }
                }
            });

            let mut init_builder = client
                .post(&mcp.server_url)
                .header("Accept", "application/json")
                .header("Accept", "text/event-stream");
            if let Some(token) = &auth_token {
                init_builder = init_builder.header("Authorization", format!("Bearer {}", token));
            }

            let init_response = init_builder
                .json(&init_request)
                .timeout(Duration::from_millis(mcp.timeout_ms))
                .send()
                .await
                .map_err(|e| RuntimeError::Generic(format!("MCP initialization failed: {}", e)))?;

            // Check status and handle SSE format
            let status = init_response.status();
            if !status.is_success() {
                let error_text = init_response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unknown error".to_string());
                return Err(RuntimeError::Generic(format!(
                    "MCP initialization failed ({}): {}",
                    status, error_text
                )));
            }

            // Extract session ID from response header first (MCP Streamable HTTP Transport spec)
            let session_id_from_header = init_response
                .headers()
                .get("mcp-session-id")
                .and_then(|v| v.to_str().ok())
                .map(String::from);

            log::debug!("MCP session ID from header: {:?}", session_id_from_header);

            let content_type = init_response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("application/json");

            let init_json: serde_json::Value = if content_type.contains("text/event-stream") {
                let body = init_response.text().await.map_err(|e| {
                    RuntimeError::Generic(format!("Failed to read SSE init response: {}", e))
                })?;
                let mut last_data = None;
                for line in body.lines() {
                    if let Some(data) = line.strip_prefix("data: ") {
                        if !data.trim().is_empty() {
                            last_data = Some(data.to_string());
                        }
                    }
                }
                if let Some(data) = last_data {
                    serde_json::from_str(&data).map_err(|e| {
                        RuntimeError::Generic(format!("Failed to parse SSE init data: {}", e))
                    })?
                } else {
                    return Err(RuntimeError::Generic(
                        "No data found in SSE init response".to_string(),
                    ));
                }
            } else {
                init_response.json().await.map_err(|e| {
                    RuntimeError::Generic(format!("Failed to parse MCP init response: {}", e))
                })?
            };

            // Check for initialization error
            if let Some(error) = init_json.get("error") {
                return Err(RuntimeError::Generic(format!(
                    "MCP initialization error: {:?}",
                    error
                )));
            }

            // Extract session ID from response header (MCP Streamable HTTP Transport spec)
            // The Mcp-Session-Id header is set in the response, not the body
            let session_id = session_id_from_header.or_else(|| {
                // Fallback: check if it's in the JSON response body (some servers)
                init_json
                    .get("result")
                    .and_then(|r| r.get("sessionId"))
                    .and_then(|s| s.as_str())
                    .map(String::from)
            });

            log::debug!("MCP session ID (final): {:?}", session_id);

            let tool_name = if mcp.tool_name.is_empty() || mcp.tool_name == "*" {
                let tools_request = json!({"jsonrpc":"2.0","id":"tools_discovery","method":"tools/list","params":{}});
                let mut request_builder = client
                    .post(&mcp.server_url)
                    .header("Accept", "application/json")
                    .header("Accept", "text/event-stream");
                if let Some(token) = &auth_token {
                    request_builder =
                        request_builder.header("Authorization", format!("Bearer {}", token));
                }
                if let Some(ref sid) = session_id {
                    request_builder = request_builder.header("Mcp-Session-Id", sid);
                }

                let response = request_builder
                    .json(&tools_request)
                    .timeout(std::time::Duration::from_millis(mcp.timeout_ms))
                    .send()
                    .await
                    .map_err(|e| {
                        RuntimeError::Generic(format!("Failed to connect to MCP server: {}", e))
                    })?;

                // Check status and handle SSE format for tools/list
                let status = response.status();
                if !status.is_success() {
                    let error_text = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "Unknown error".to_string());
                    return Err(RuntimeError::Generic(format!(
                        "MCP tools/list failed ({}): {}",
                        status, error_text
                    )));
                }

                let content_type = response
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("application/json");

                let tools_response: serde_json::Value = if content_type
                    .contains("text/event-stream")
                {
                    let body = response.text().await.map_err(|e| {
                        RuntimeError::Generic(format!("Failed to read SSE tools response: {}", e))
                    })?;
                    let mut last_data = None;
                    for line in body.lines() {
                        if let Some(data) = line.strip_prefix("data: ") {
                            if !data.trim().is_empty() {
                                last_data = Some(data.to_string());
                            }
                        }
                    }
                    if let Some(data) = last_data {
                        serde_json::from_str(&data).map_err(|e| {
                            RuntimeError::Generic(format!("Failed to parse SSE tools data: {}", e))
                        })?
                    } else {
                        return Err(RuntimeError::Generic(
                            "No data found in SSE tools response".to_string(),
                        ));
                    }
                } else {
                    let body_text = response.text().await.map_err(|e| {
                        RuntimeError::Generic(format!("Failed to read MCP response: {}", e))
                    })?;
                    serde_json::from_str(&body_text).map_err(|e| {
                        RuntimeError::Generic(format!(
                            "Failed to parse MCP response: {} (body: {})",
                            e,
                            &body_text[..body_text.len().min(200)]
                        ))
                    })?
                };
                if let Some(result) = tools_response.get("result") {
                    if let Some(tools) = result.get("tools").and_then(|t| t.as_array()) {
                        if let Some(first_tool) = tools
                            .first()
                            .and_then(|t| t.get("name"))
                            .and_then(|n| n.as_str())
                        {
                            first_tool.to_string()
                        } else {
                            return Err(RuntimeError::Generic(
                                "No MCP tools available".to_string(),
                            ));
                        }
                    } else {
                        return Err(RuntimeError::Generic(
                            "Invalid tools response format".to_string(),
                        ));
                    }
                } else {
                    return Err(RuntimeError::Generic(
                        "No result in MCP response".to_string(),
                    ));
                }
            } else {
                mcp.tool_name.clone()
            };
            let tool_request = json!({"jsonrpc":"2.0","id":"tool_call","method":"tools/call","params":{"name":tool_name,"arguments":input_json}});
            let mut request_builder = client
                .post(&mcp.server_url)
                .header("Accept", "application/json")
                .header("Accept", "text/event-stream");
            if let Some(token) = &auth_token {
                request_builder =
                    request_builder.header("Authorization", format!("Bearer {}", token));
            }
            if let Some(ref sid) = session_id {
                request_builder = request_builder.header("Mcp-Session-Id", sid);
            }

            let handle_tool_response = |tool_response: serde_json::Value| -> RuntimeResult<Value> {
                if let Some(error) = tool_response.get("error") {
                    return Err(RuntimeError::Generic(format!(
                        "MCP tool execution failed: {:?}",
                        error
                    )));
                }
                if let Some(result) = tool_response.get("result") {
                    if let Some(content) = result.get("content") {
                        // Prefer structured RTFS over raw JSON-in-string
                        if let Some(arr) = content.as_array() {
                            if let Some(first) = arr.first() {
                                if first
                                    .get("type")
                                    .and_then(|v| v.as_str())
                                    .map(|t| t.eq_ignore_ascii_case("text"))
                                    .unwrap_or(false)
                                {
                                    if let Some(text) = first.get("text").and_then(|v| v.as_str()) {
                                        if let Ok(parsed) =
                                            serde_json::from_str::<serde_json::Value>(text)
                                        {
                                            return json_to_rtfs_value(&parsed)
                                                .or_else(|_| json_to_rtfs_value(content));
                                        }
                                    }
                                }
                            }
                        }
                        json_to_rtfs_value(content)
                    } else {
                        json_to_rtfs_value(result)
                    }
                } else {
                    Err(RuntimeError::Generic(
                        "No result in MCP tool response".to_string(),
                    ))
                }
            };

            // Prefer streaming consumption when available to avoid partial SSE bodies.
            if let Some(es_builder) = request_builder.try_clone() {
                match EventSource::new(es_builder.json(&tool_request)) {
                    Ok(mut event_source) => {
                        while let Some(event) = event_source.next().await {
                            match event {
                                Ok(Event::Open) => continue,
                                Ok(Event::Message(msg)) => {
                                    if msg.data.trim().is_empty() {
                                        continue;
                                    }
                                    event_source.close();
                                    return match serde_json::from_str::<serde_json::Value>(
                                        &msg.data,
                                    ) {
                                        Ok(v) => handle_tool_response(v),
                                        Err(e) => Err(RuntimeError::Generic(format!(
                                            "Failed to parse SSE data: {} (data: {})",
                                            e,
                                            &msg.data[..msg.data.len().min(200)]
                                        ))),
                                    };
                                }
                                Err(_err) => {
                                    event_source.close();
                                    break;
                                }
                            }
                        }
                    }
                    Err(_) => {
                        // Fall back to HTTP body parsing below
                    }
                }
            }

            let response = request_builder
                .json(&tool_request)
                .timeout(std::time::Duration::from_millis(mcp.timeout_ms))
                .send()
                .await
                .map_err(|e| RuntimeError::Generic(format!("Failed to execute MCP tool: {}", e)))?;

            // Check status code first
            let status = response.status();

            // Read the body as text first for debugging
            let body_text = response.text().await.map_err(|e| {
                RuntimeError::Generic(format!("Failed to read response body: {}", e))
            })?;
            let _ = std::fs::write("mcp_last_body.txt", &body_text);
            if body_text.is_empty() {
                return Err(RuntimeError::Generic(format!(
                    "MCP tool execution returned empty response (status={}, content-type={})",
                    status, "unknown"
                )));
            }

            log::debug!(
                "MCP tool response body (first 500 chars): {}",
                &body_text[..body_text.len().min(500)]
            );

            if !status.is_success() {
                return Err(RuntimeError::Generic(format!(
                    "MCP tool execution failed ({}): {}",
                    status, body_text
                )));
            }

            // Helper to extract last `data: ...` JSON chunk from SSE-like bodies, including multi-line data
            fn extract_sse_data(body: &str) -> Option<String> {
                let mut candidates: Vec<String> = Vec::new();
                let mut current = String::new();
                let mut in_data_block = false;

                for raw_line in body.lines() {
                    let line = raw_line.trim_end_matches(['\r', '\n']);
                    let trimmed_line = line.trim_start();

                    if let Some(rest) = trimmed_line.strip_prefix("data:") {
                        // Allow multi-line data blocks; join them with newlines per SSE spec.
                        let data = rest.trim_start();
                        if !in_data_block {
                            current.clear();
                            in_data_block = true;
                        }
                        if !current.is_empty() {
                            current.push('\n');
                        }
                        current.push_str(data);
                        continue;
                    }

                    // Blank line or new event boundary flushes the current data block.
                    if trimmed_line.is_empty() || trimmed_line.starts_with("event:") {
                        if in_data_block && !current.is_empty() {
                            candidates.push(current.clone());
                            current.clear();
                        }
                        in_data_block = false;
                    }
                }

                if in_data_block && !current.is_empty() {
                    candidates.push(current);
                }

                if candidates.is_empty() {
                    if let Some(idx) = body.rfind("data:") {
                        let tail = body[idx + "data:".len()..].trim_start();
                        let tail_until_boundary = tail
                            .lines()
                            .take_while(|line| {
                                let trimmed = line.trim_start();
                                !trimmed.is_empty() && !trimmed.starts_with("event:")
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        if !tail_until_boundary.is_empty() {
                            candidates.push(tail_until_boundary);
                        }
                    }
                }

                if candidates.is_empty() {
                    if let Some(start_idx) = body.find(|c| c == '{' || c == '[') {
                        if let Some(end_idx) = body.rfind(|c| c == '}' || c == ']') {
                            if end_idx >= start_idx {
                                let slice = body[start_idx..=end_idx].trim();
                                if !slice.is_empty() {
                                    candidates.push(slice.to_string());
                                }
                            }
                        }
                    }
                }

                for candidate in candidates.into_iter().rev() {
                    let trimmed = candidate.trim_start();
                    if trimmed.starts_with('{') || trimmed.starts_with('[') {
                        if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
                            return Some(candidate);
                        }
                    }
                }
                None
            }

            let sse_candidate = {
                // Fast path: take the substring after the last "data:" marker if present.
                if let Some(idx) = body_text.rfind("data:") {
                    let tail = body_text[idx + "data:".len()..].trim();
                    if !tail.is_empty() {
                        Some(tail.to_string())
                    } else {
                        extract_sse_data(&body_text)
                    }
                } else {
                    extract_sse_data(&body_text)
                }
            };

            let tool_response: serde_json::Value = if let Some(data) = sse_candidate {
                match serde_json::from_str(&data) {
                    Ok(v) => v,
                    Err(e) => {
                        return Err(RuntimeError::Generic(format!(
                            "Failed to parse SSE data: {} (data: {})",
                            e,
                            &data[..data.len().min(200)]
                        )))
                    }
                }
            } else {
                // Parse as regular JSON as a last resort
                match serde_json::from_str(&body_text) {
                    Ok(v) => v,
                    Err(primary_err) => {
                        // Try trimming SSE prefixes
                        let cleaned = body_text
                            .replace("event: message", "")
                            .replace("data:", "")
                            .trim()
                            .to_string();
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&cleaned) {
                            v
                        } else if let Some(start_idx) = body_text.find(|c| c == '{' || c == '[') {
                            if let Some(end_idx) = body_text.rfind(|c| c == '}' || c == ']') {
                                let slice = body_text[start_idx..=end_idx].trim();
                                serde_json::from_str(slice).map_err(|secondary_err| {
                                    RuntimeError::Generic(format!(
                                        "Failed to parse tool response (primary: {}; slice: {}; body: {})",
                                        primary_err,
                                        secondary_err,
                                        &body_text[..body_text.len().min(200)]
                                    ))
                                })?
                            } else {
                                return Err(RuntimeError::Generic(format!(
                                    "Failed to parse tool response: {} (body: {})",
                                    primary_err,
                                    &body_text[..body_text.len().min(200)]
                                )));
                            }
                        } else {
                            return Err(RuntimeError::Generic(format!(
                                "Failed to parse tool response: {} (body: {})",
                                primary_err,
                                &body_text[..body_text.len().min(200)]
                            )));
                        }
                    }
                }
            };
            handle_tool_response(tool_response)
        } else {
            Err(RuntimeError::Generic(
                "Invalid provider type for MCP executor".to_string(),
            ))
        }
    }
}

pub struct A2AExecutor;

#[async_trait]
impl CapabilityExecutor for A2AExecutor {
    fn provider_type_id(&self) -> TypeId {
        TypeId::of::<A2ACapability>()
    }
    async fn execute(
        &self,
        provider: &ProviderType,
        inputs: &Value,
        _context: &ExecutionContext<'_>,
    ) -> RuntimeResult<Value> {
        if let ProviderType::A2A(a2a) = provider {
            match a2a.protocol.as_str() {
                "http" | "https" => self.execute_a2a_http(a2a, inputs).await,
                "websocket" | "ws" | "wss" => Err(RuntimeError::Generic(
                    "A2A WebSocket protocol not yet implemented".to_string(),
                )),
                "grpc" => Err(RuntimeError::Generic(
                    "A2A gRPC protocol not yet implemented".to_string(),
                )),
                _ => Err(RuntimeError::Generic(format!(
                    "Unsupported A2A protocol: {}",
                    a2a.protocol
                ))),
            }
        } else {
            Err(RuntimeError::Generic(
                "ProviderType mismatch for A2AExecutor".to_string(),
            ))
        }
    }
}

impl A2AExecutor {
    async fn execute_a2a_http(&self, a2a: &A2ACapability, inputs: &Value) -> RuntimeResult<Value> {
        let client = reqwest::Client::new();
        let input_json = Self::value_to_json(inputs)?;
        let payload = serde_json::json!({"agent_id":a2a.agent_id,"capability":"execute","inputs":input_json,"timestamp": chrono::Utc::now().to_rfc3339()});
        let response = client
            .post(&a2a.endpoint)
            .header("Content-Type", "application/json")
            .json(&payload)
            .timeout(std::time::Duration::from_millis(a2a.timeout_ms))
            .send()
            .await
            .map_err(|e| RuntimeError::Generic(format!("A2A HTTP request failed: {}", e)))?;
        if !response.status().is_success() {
            let status = response.status();
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(RuntimeError::Generic(format!(
                "A2A HTTP error {}: {}",
                status, error_body
            )));
        }
        let response_json: serde_json::Value = response.json().await.map_err(|e| {
            RuntimeError::Generic(format!("Failed to parse A2A HTTP response: {}", e))
        })?;
        if let Some(result) = response_json.get("result") {
            json_to_rtfs_value(result)
        } else if let Some(error) = response_json.get("error") {
            let error_msg = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown A2A error");
            Err(RuntimeError::Generic(format!("A2A error: {}", error_msg)))
        } else {
            Err(RuntimeError::Generic(
                "Invalid A2A response format".to_string(),
            ))
        }
    }
    // Use shared value conversion utilities
    fn value_to_json(value: &Value) -> Result<serde_json::Value, RuntimeError> {
        value_conversion::rtfs_value_to_json(value)
    }
}

pub struct LocalExecutor;

#[async_trait]
impl CapabilityExecutor for LocalExecutor {
    fn provider_type_id(&self) -> TypeId {
        TypeId::of::<LocalCapability>()
    }
    async fn execute(
        &self,
        provider: &ProviderType,
        inputs: &Value,
        _context: &ExecutionContext<'_>,
    ) -> RuntimeResult<Value> {
        if let ProviderType::Local(local) = provider {
            (local.handler)(inputs)
        } else {
            Err(RuntimeError::Generic(
                "ProviderType mismatch for LocalExecutor".to_string(),
            ))
        }
    }
}

pub struct RegistryExecutor;

#[async_trait]
impl CapabilityExecutor for RegistryExecutor {
    fn provider_type_id(&self) -> TypeId {
        TypeId::of::<RegistryCapability>()
    }
    async fn execute(
        &self,
        provider: &ProviderType,
        inputs: &Value,
        _context: &ExecutionContext<'_>,
    ) -> RuntimeResult<Value> {
        if let ProviderType::Registry(registry_cap) = provider {
            let registry = registry_cap.registry.read().await;

            // Re-wrap inputs in CallMetadata if needed, but for registry we usually
            // just pass the inputs directly as the registry handles its own envelope unwrapping
            // or expects a Vector of arguments depending on the provider.

            registry.execute_capability_with_microvm(
                &registry_cap.capability_id,
                vec![inputs.clone()],
                None,
            )
        } else {
            Err(RuntimeError::Generic(
                "ProviderType mismatch for RegistryExecutor".to_string(),
            ))
        }
    }
}

pub struct OpenApiExecutor;

#[async_trait]
impl CapabilityExecutor for OpenApiExecutor {
    fn provider_type_id(&self) -> TypeId {
        TypeId::of::<OpenApiCapability>()
    }

    async fn execute(
        &self,
        provider: &ProviderType,
        inputs: &Value,
        _context: &ExecutionContext<'_>,
    ) -> RuntimeResult<Value> {
        if let ProviderType::OpenApi(openapi) = provider {
            self.execute_openapi(openapi, inputs).await
        } else {
            Err(RuntimeError::Generic(
                "ProviderType mismatch for OpenApiExecutor".to_string(),
            ))
        }
    }
}

impl OpenApiExecutor {
    async fn execute_openapi(
        &self,
        openapi: &OpenApiCapability,
        inputs: &Value,
    ) -> RuntimeResult<Value> {
        let mut input_map = Self::extract_input_map(inputs)?;
        let operation_hint = input_map
            .remove("operation")
            .or_else(|| input_map.remove("operation_id"))
            .map(|v| Self::value_to_string(&v))
            .transpose()?;
        let params_value = input_map.remove("params");
        let headers_value = input_map.remove("headers");
        let body_value = input_map.remove("body");

        let mut params = HashMap::new();
        if let Some(params_val) = params_value {
            params.extend(Self::value_to_value_map(&params_val)?);
        }
        for (key, value) in input_map.into_iter() {
            params.insert(key, value);
        }

        let operation = Self::resolve_operation(openapi, operation_hint.as_deref())?;
        let mut path = operation.path.clone();
        Self::apply_path_params(&mut path, &mut params)?;

        let mut url = Self::build_url(&openapi.base_url, &path)?;

        let mut headers = if let Some(h) = headers_value {
            Self::value_to_string_map(&h)?
        } else {
            HashMap::new()
        };

        let auth_override = params
            .remove("auth_token")
            .map(|v| Self::value_to_string(&v))
            .transpose()?;

        let mut query_pairs: Vec<(String, String)> = Vec::new();
        for (key, value) in params.iter() {
            let value_str = Self::value_to_string(value)?;
            if !value_str.is_empty() {
                query_pairs.push((key.clone(), value_str));
            }
        }

        if let Some(auth) = &openapi.auth {
            let mut token = auth_override.clone();
            if token.is_none() {
                token = Self::extract_auth_token(auth, &mut query_pairs, &mut headers)?;
            }
            if token.is_none() {
                token = Self::read_env_token(auth);
            }
            if auth.required && token.is_none() {
                return Err(RuntimeError::Generic(format!(
                    "Missing credentials for OpenAPI capability using parameter '{}'",
                    auth.parameter_name
                )));
            }
            if let Some(token) = token {
                Self::apply_auth(auth, token, &mut query_pairs, &mut headers);
            }
        }

        if !query_pairs.is_empty() {
            let mut pairs = url.query_pairs_mut();
            for (k, v) in query_pairs {
                pairs.append_pair(&k, &v);
            }
        }

        let method = reqwest::Method::from_bytes(operation.method.as_bytes())
            .unwrap_or(reqwest::Method::GET);
        let disable_proxy = url
            .host_str()
            .map(Self::should_disable_proxy_for_host)
            .unwrap_or(false);
        let client = if disable_proxy {
            reqwest::Client::builder().no_proxy().build().map_err(|e| {
                RuntimeError::Generic(format!("Failed to build OpenAPI client: {}", e))
            })?
        } else {
            reqwest::Client::new()
        };
        let mut request = client.request(method.clone(), url);

        for (key, value) in headers.iter() {
            request = request.header(key, value);
        }

        if let Some(body) = body_value {
            if method != reqwest::Method::GET {
                match &body {
                    Value::String(s) => {
                        if !headers
                            .keys()
                            .any(|h| h.eq_ignore_ascii_case("content-type"))
                        {
                            request = request.header("Content-Type", "application/json");
                        }
                        request = request.body(s.clone());
                    }
                    _ => {
                        let json_body = A2AExecutor::value_to_json(&body)?;
                        request = request.json(&json_body);
                    }
                }
            }
        }

        let response = request
            .timeout(Duration::from_millis(openapi.timeout_ms.max(1)))
            .send()
            .await
            .map_err(|e| RuntimeError::Generic(format!("OpenAPI request failed: {}", e)))?;

        let status = response.status().as_u16() as i64;
        let response_headers = response.headers().clone();
        let bytes = response.bytes().await.map_err(|e| {
            RuntimeError::Generic(format!("Failed to read OpenAPI response: {}", e))
        })?;
        let body_text = String::from_utf8_lossy(&bytes).to_string();

        let mut response_map = HashMap::new();
        response_map.insert(MapKey::String("status".to_string()), Value::Integer(status));
        response_map.insert(
            MapKey::String("body".to_string()),
            Value::String(body_text.clone()),
        );

        let mut headers_map = HashMap::new();
        for (key, value) in response_headers.iter() {
            headers_map.insert(
                MapKey::String(key.to_string()),
                Value::String(value.to_str().unwrap_or("").to_string()),
            );
        }
        response_map.insert(
            MapKey::String("headers".to_string()),
            Value::Map(headers_map),
        );

        if !bytes.is_empty() {
            if let Ok(json_value) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                if let Ok(rtfs_json) = json_to_rtfs_value(&json_value) {
                    response_map.insert(MapKey::String("json".to_string()), rtfs_json);
                }
            }
        }

        Ok(Value::Map(response_map))
    }

    fn extract_input_map(inputs: &Value) -> RuntimeResult<HashMap<String, Value>> {
        match inputs {
            Value::Map(m) => m
                .iter()
                .map(|(k, v)| Ok((Self::map_key_to_string(k)?, v.clone())))
                .collect(),
            Value::List(list) | Value::Vector(list) => {
                if let Some(Value::Map(m)) = list.first() {
                    m.iter()
                        .map(|(k, v)| Ok((Self::map_key_to_string(k)?, v.clone())))
                        .collect()
                } else {
                    Err(RuntimeError::Generic(
                        "OpenAPI executor expects map input".to_string(),
                    ))
                }
            }
            _ => Err(RuntimeError::Generic(
                "OpenAPI executor expects map input".to_string(),
            )),
        }
    }

    // Use shared utility for map key conversion
    fn map_key_to_string(key: &MapKey) -> RuntimeResult<String> {
        Ok(value_conversion::map_key_to_string(key))
    }

    fn value_to_value_map(value: &Value) -> RuntimeResult<HashMap<String, Value>> {
        if let Value::Map(map) = value {
            map.iter()
                .map(|(k, v)| Ok((Self::map_key_to_string(k)?, v.clone())))
                .collect()
        } else {
            Err(RuntimeError::Generic(
                "Expected map value for OpenAPI parameters".to_string(),
            ))
        }
    }

    fn value_to_string_map(value: &Value) -> RuntimeResult<HashMap<String, String>> {
        if let Value::Map(map) = value {
            map.iter()
                .map(|(k, v)| Ok((Self::map_key_to_string(k)?, Self::value_to_string(v)?)))
                .collect()
        } else {
            Err(RuntimeError::Generic(
                "Expected map value for OpenAPI headers".to_string(),
            ))
        }
    }

    fn value_to_string(value: &Value) -> RuntimeResult<String> {
        match value {
            Value::String(s) => Ok(s.clone()),
            Value::Integer(i) => Ok(i.to_string()),
            Value::Float(f) => Ok(f.to_string()),
            Value::Boolean(b) => Ok(b.to_string()),
            Value::Nil => Ok(String::new()),
            Value::Vector(_) | Value::List(_) | Value::Map(_) => {
                let json = A2AExecutor::value_to_json(value)?;
                Ok(json.to_string())
            }
            _ => Err(RuntimeError::Generic(format!(
                "Unsupported value type '{}' for OpenAPI parameter",
                value.type_name()
            ))),
        }
    }

    fn resolve_operation<'a>(
        openapi: &'a OpenApiCapability,
        hint: Option<&str>,
    ) -> RuntimeResult<&'a OpenApiOperation> {
        if let Some(hint) = hint {
            let hint_lower = hint.to_lowercase();
            if let Some(op) = openapi.operations.iter().find(|op| {
                op.operation_id
                    .as_deref()
                    .map(|id| id.eq_ignore_ascii_case(&hint_lower))
                    .unwrap_or(false)
            }) {
                return Ok(op);
            }
            if let Some(op) = openapi.operations.iter().find(|op| {
                let method_path = format!("{} {}", op.method.to_lowercase(), op.path);
                method_path == hint_lower
            }) {
                return Ok(op);
            }
            if let Some(op) = openapi.operations.iter().find(|op| {
                op.summary
                    .as_ref()
                    .map(|s| s.to_lowercase() == hint_lower)
                    .unwrap_or(false)
            }) {
                return Ok(op);
            }
            if let Some(op) = openapi.operations.iter().find(|op| op.path == hint) {
                return Ok(op);
            }
            return Err(RuntimeError::Generic(format!(
                "No OpenAPI operation matches '{}'; available operations: {}",
                hint,
                openapi
                    .operations
                    .iter()
                    .filter_map(|op| op.operation_id.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }

        match openapi.operations.len() {
            0 => Err(RuntimeError::Generic(
                "OpenAPI provider has no operations defined".to_string(),
            )),
            1 => Ok(&openapi.operations[0]),
            _ => Err(RuntimeError::Generic(
                "Multiple OpenAPI operations available; specify :operation".to_string(),
            )),
        }
    }

    fn apply_path_params(
        path: &mut String,
        params: &mut HashMap<String, Value>,
    ) -> RuntimeResult<()> {
        let regex = Regex::new(r"\{([^}/]+)\}").map_err(|e| {
            RuntimeError::Generic(format!("Failed to compile path parameter regex: {}", e))
        })?;

        for capture in regex.captures_iter(path.clone().as_str()) {
            let key = capture.get(1).unwrap().as_str();
            let value = params.remove(key).ok_or_else(|| {
                RuntimeError::Generic(format!(
                    "Missing required path parameter '{}' for OpenAPI call",
                    key
                ))
            })?;
            let value_str = Self::value_to_string(&value)?;
            let encoded = encode(&value_str).into_owned();
            *path = path.replace(&format!("{{{}}}", key), &encoded);
        }
        Ok(())
    }

    fn build_url(base_url: &str, path: &str) -> RuntimeResult<reqwest::Url> {
        let mut base = base_url.trim_end_matches('/').to_string();
        let mut final_path = path.to_string();
        if !final_path.starts_with('/') {
            final_path = format!("/{}", final_path);
        }
        base.push_str(&final_path);
        reqwest::Url::parse(&base)
            .map_err(|e| RuntimeError::Generic(format!("Invalid OpenAPI URL '{}': {}", base, e)))
    }

    fn extract_auth_token(
        auth: &OpenApiAuth,
        query_pairs: &mut Vec<(String, String)>,
        headers: &mut HashMap<String, String>,
    ) -> RuntimeResult<Option<String>> {
        if auth.location.eq_ignore_ascii_case("query") {
            if let Some((index, _)) = query_pairs
                .iter()
                .enumerate()
                .find(|(_, (k, _))| k == &auth.parameter_name)
            {
                let (_, value) = query_pairs.remove(index);
                return Ok(Some(value));
            }
        }
        if auth.location.eq_ignore_ascii_case("header") {
            if let Some(value) = headers.remove(&auth.parameter_name) {
                return Ok(Some(value));
            }
        }
        Ok(None)
    }

    fn read_env_token(auth: &OpenApiAuth) -> Option<String> {
        let env_var = auth.env_var_name.as_ref()?;

        // Try SecretStore lookup first (covers local secrets file AND env vars via fallback)
        if let Ok(store) = SecretStore::new(Some(get_workspace_root())) {
            if let Some(val) = store.get(env_var) {
                return Some(val);
            }
        }

        // Direct fallback if store failed to load (though store.get does this too)
        std::env::var(env_var).ok()
    }

    fn apply_auth(
        auth: &OpenApiAuth,
        token: String,
        query_pairs: &mut Vec<(String, String)>,
        headers: &mut HashMap<String, String>,
    ) {
        if auth.location.eq_ignore_ascii_case("query") {
            query_pairs.push((auth.parameter_name.clone(), token));
        } else if auth.location.eq_ignore_ascii_case("header") {
            let header_value = if auth.auth_type.eq_ignore_ascii_case("bearer")
                && !token.to_lowercase().starts_with("bearer ")
            {
                format!("Bearer {}", token)
            } else {
                token
            };
            headers.insert(auth.parameter_name.clone(), header_value);
        } else if auth.location.eq_ignore_ascii_case("cookie") {
            let cookie_value = format!("{}={}", auth.parameter_name, token);
            headers
                .entry("Cookie".to_string())
                .and_modify(|existing| {
                    if existing.is_empty() {
                        *existing = cookie_value.clone();
                    } else {
                        existing.push_str("; ");
                        existing.push_str(&cookie_value);
                    }
                })
                .or_insert(cookie_value);
        }
    }

    fn should_disable_proxy_for_host(host: &str) -> bool {
        if host.eq_ignore_ascii_case("localhost") {
            return true;
        }
        if let Ok(ip) = host.parse::<IpAddr>() {
            return ip.is_loopback();
        }
        false
    }
}

pub struct HttpExecutor;

#[async_trait]
impl CapabilityExecutor for HttpExecutor {
    fn provider_type_id(&self) -> TypeId {
        TypeId::of::<HttpCapability>()
    }
    async fn execute(
        &self,
        provider: &ProviderType,
        inputs: &Value,
        context: &ExecutionContext<'_>,
    ) -> RuntimeResult<Value> {
        if let ProviderType::Http(http) = provider {
            let (url, method, mut headers_owned, mut body) = match inputs {
                Value::Map(map) => {
                    let url = map
                        .get(&MapKey::String("url".to_string()))
                        .or_else(|| {
                            map.get(&MapKey::Keyword(rtfs::ast::Keyword("url".to_string())))
                        })
                        .and_then(|v| v.as_string())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| http.base_url.clone());

                    let method = map
                        .get(&MapKey::String("method".to_string()))
                        .or_else(|| {
                            map.get(&MapKey::Keyword(rtfs::ast::Keyword("method".to_string())))
                        })
                        .and_then(|v| v.as_string())
                        .map(|s| s.to_string())
                        .or_else(|| context.metadata.get("method").cloned())
                        .unwrap_or_else(|| "POST".to_string());

                    let mut headers = HashMap::new();
                    if let Some(h_val) =
                        map.get(&MapKey::String("headers".to_string())).or_else(|| {
                            map.get(&MapKey::Keyword(rtfs::ast::Keyword("headers".to_string())))
                        })
                    {
                        if let Value::Map(h_map) = h_val {
                            for (k, v) in h_map {
                                let ks = value_conversion::map_key_to_string(k);
                                if let Some(vs) = v.as_string() {
                                    headers.insert(ks, vs.to_string());
                                }
                            }
                        }
                    }

                    let body = if let Some(b_val) =
                        map.get(&MapKey::String("body".to_string())).or_else(|| {
                            map.get(&MapKey::Keyword(rtfs::ast::Keyword("body".to_string())))
                        }) {
                        match b_val {
                            Value::String(s) => s.clone(),
                            _ => A2AExecutor::value_to_json(b_val)
                                .map(|j| j.to_string())
                                .unwrap_or_default(),
                        }
                    } else {
                        // If no explicit body, treat the whole map as JSON body
                        // excluding control keys
                        let mut body_map = map.clone();
                        body_map.remove(&MapKey::String("url".to_string()));
                        body_map.remove(&MapKey::Keyword(rtfs::ast::Keyword("url".to_string())));
                        body_map.remove(&MapKey::String("method".to_string()));
                        body_map.remove(&MapKey::Keyword(rtfs::ast::Keyword("method".to_string())));
                        body_map.remove(&MapKey::String("headers".to_string()));
                        body_map
                            .remove(&MapKey::Keyword(rtfs::ast::Keyword("headers".to_string())));

                        if body_map.is_empty() {
                            String::new()
                        } else {
                            A2AExecutor::value_to_json(&Value::Map(body_map))
                                .map(|j| j.to_string())
                                .unwrap_or_default()
                        }
                    };
                    (url, method, headers, body)
                }
                _ => {
                    let args = match inputs {
                        Value::List(list) => list.clone(),
                        Value::Vector(vec) => vec.clone(),
                        v => vec![v.clone()],
                    };
                    let url = args
                        .get(0)
                        .and_then(|v| v.as_string())
                        .unwrap_or(&http.base_url)
                        .to_string();
                    let method = args
                        .get(1)
                        .and_then(|v| v.as_string())
                        .unwrap_or("GET")
                        .to_string();
                    let mut headers = HashMap::new();
                    if let Some(Value::Map(h_map)) = args.get(2) {
                        for (k, v) in h_map {
                            let ks = value_conversion::map_key_to_string(k);
                            if let Some(vs) = v.as_string() {
                                headers.insert(ks, vs.to_string());
                            }
                        }
                    }
                    let body = args
                        .get(3)
                        .and_then(|v| v.as_string())
                        .unwrap_or("")
                        .to_string();
                    (url, method, headers, body)
                }
            };

            let client = reqwest::Client::new();
            let method_enum =
                reqwest::Method::from_bytes(method.as_bytes()).unwrap_or(reqwest::Method::GET);

            let mut req = client.request(method_enum, &url);
            if let Some(token) = &http.auth_token {
                req = req.bearer_auth(token);
            }

            // Check if this is a skill capability and inject secret if available
            // SECURITY: Only inject secrets from SecretStore (which requires approval)
            // Secrets pending approval in SECRET_INJECTION are NOT injected until approved
            log::info!(
                "[HttpExecutor] Checking for skill_id in metadata: {:?}",
                context.metadata.get("skill_id")
            );
            if let Some(skill_id) = context.metadata.get("skill_id") {
                log::info!(
                    "[HttpExecutor] Found skill_id: {}, looking for approved secret",
                    skill_id
                );

                let secret_key = format!("{}.secret", skill_id);

                // Only use SecretStore - which requires approval
                // This ensures governance is enforced
                let cwd = std::env::current_dir().ok();
                let secret_store = crate::secrets::SecretStore::new(cwd).ok();

                if let Some(secret_value) = secret_store.and_then(|store| store.get(&secret_key)) {
                    log::info!(
                        "[HttpExecutor] Injecting APPROVED secret for skill '{}'",
                        skill_id
                    );
                    req = req.bearer_auth(secret_value);
                } else {
                    log::info!("[HttpExecutor] No approved secret found for skill '{}'. Secret may be pending approval.", skill_id);
                }
            } else {
                log::info!("[HttpExecutor] No skill_id in metadata, skipping secret injection");
            }

            let method_upper = method.to_ascii_uppercase();
            let is_write = matches!(method_upper.as_str(), "POST" | "PUT" | "PATCH");
            let has_content_type = headers_owned
                .keys()
                .any(|k| k.eq_ignore_ascii_case("content-type"));
            if is_write && body.is_empty() {
                body = "{}".to_string();
                if !has_content_type {
                    headers_owned
                        .insert("Content-Type".to_string(), "application/json".to_string());
                }
            }

            let redacted_url = redact_text_for_logs(&url);
            let has_content_type = headers_owned
                .keys()
                .any(|k| k.eq_ignore_ascii_case("content-type"));
            tracing::info!(
                "[HttpExecutor] Sending {} request to {} (body_len={}, content_type={})",
                method_upper,
                redacted_url,
                body.len(),
                has_content_type
            );

            for (key, val) in headers_owned.iter() {
                req = req.header(key, val);
            }

            if !body.is_empty() {
                // If it looks like JSON and no content-type, add it
                if (body.starts_with('{') || body.starts_with('['))
                    && !headers_owned
                        .keys()
                        .any(|k| k.eq_ignore_ascii_case("content-type"))
                {
                    req = req.header("Content-Type", "application/json");
                }
                req = req.body(body);
            }
            let response = req
                .timeout(std::time::Duration::from_millis(http.timeout_ms))
                .send()
                .await
                .map_err(|e| RuntimeError::Generic(format!("HTTP request failed: {}", e)))?;
            let status = response.status().as_u16() as i64;
            let response_headers = response.headers().clone();
            let resp_body = response.text().await.unwrap_or_default();

            if status >= 400 {
                return Err(RuntimeError::Generic(format!(
                    "HTTP {} request to '{}' failed with status {}: {}",
                    method, url, status, resp_body
                )));
            }

            let redacted_body = match serde_json::from_str::<serde_json::Value>(&resp_body) {
                Ok(json) => redact_json_for_logs(&json).to_string(),
                Err(_) => redact_text_for_logs(&resp_body),
            };
            tracing::info!(
                "[HttpExecutor] Received status {} with body: {}",
                status,
                redacted_body
            );

            // Auto-handle secrets: store, create approval, hide from agent
            let mut processed_body = resp_body.clone();
            if status < 400 {
                if let Some(returns_secret) = context.metadata.get("returns_secret") {
                    if returns_secret == "true" {
                        if let Some(skill_id) = context.metadata.get("skill_id") {
                            // Try to extract secret from response
                            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&resp_body)
                            {
                                if let Some(secret_value) =
                                    json.get("secret").and_then(|v| v.as_str())
                                {
                                    // Store secret in shared in-memory store for auto-injection
                                    let secret_key = format!("{}.secret", skill_id);
                                    SECRET_INJECTION
                                        .write()
                                        .await
                                        .insert(secret_key.clone(), secret_value.to_string());
                                    log::info!("[HttpExecutor] Auto-stored secret for skill '{}' (pending approval)", skill_id);

                                    // Replace secret value in response to hide from agent
                                    // Agent only sees that a secret exists, not its value
                                    if let Ok(mut json_obj) =
                                        serde_json::from_str::<serde_json::Value>(&processed_body)
                                    {
                                        if let Some(obj) = json_obj.as_object_mut() {
                                            obj.insert(
                                                "secret".to_string(),
                                                serde_json::Value::String(
                                                    "***PENDING_APPROVAL***".to_string(),
                                                ),
                                            );
                                            // Add helpful message
                                            obj.insert("message".to_string(), serde_json::Value::String(format!(
                                                "Secret received and stored pending approval. Use /approve to approve the '{}' secret, then subsequent skill operations will use it automatically.",
                                                secret_key
                                            )));
                                            processed_body = serde_json::to_string(&json_obj)
                                                .unwrap_or(processed_body);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            let mut response_map = std::collections::HashMap::new();
            response_map.insert(MapKey::String("status".to_string()), Value::Integer(status));
            response_map.insert(
                MapKey::String("body".to_string()),
                Value::String(processed_body),
            );
            let mut headers_map = std::collections::HashMap::new();
            for (k, v) in response_headers.iter() {
                headers_map.insert(
                    MapKey::String(k.to_string()),
                    Value::String(v.to_str().unwrap_or("").to_string()),
                );
            }
            response_map.insert(
                MapKey::String("headers".to_string()),
                Value::Map(headers_map),
            );
            Ok(Value::Map(response_map))
        } else {
            Err(RuntimeError::Generic(
                "ProviderType mismatch for HttpExecutor".to_string(),
            ))
        }
    }
}

pub struct SandboxedExecutor {
    manager: Arc<crate::sandbox::SandboxManager>,
}

impl SandboxedExecutor {
    pub fn new() -> Self {
        Self {
            manager: Arc::new(crate::sandbox::SandboxManager::new()),
        }
    }
}

#[async_trait]
impl CapabilityExecutor for SandboxedExecutor {
    fn provider_type_id(&self) -> TypeId {
        TypeId::of::<SandboxedCapability>()
    }

    async fn execute(
        &self,
        provider: &ProviderType,
        inputs: &Value,
        context: &ExecutionContext<'_>,
    ) -> RuntimeResult<Value> {
        if let ProviderType::Sandboxed(sandboxed) = provider {
            let program = if sandboxed.runtime == "wasm" {
                // Try to decode as base64, if fails assume it's a path and read it
                let wasm_bytes = if let Ok(bytes) = Base64Engine.decode(&sandboxed.source) {
                    bytes
                } else {
                    std::fs::read(&sandboxed.source).map_err(|e| {
                        RuntimeError::Generic(format!("Failed to read WASM file: {}", e))
                    })?
                };
                Program::Binary {
                    language: ScriptLanguage::Wasm,
                    source: wasm_bytes,
                }
            } else {
                let language = match sandboxed.runtime.as_str() {
                    "python" | "python3" => ScriptLanguage::Python,
                    "javascript" | "js" | "node" => ScriptLanguage::JavaScript,
                    "shell" | "sh" | "bash" => ScriptLanguage::Shell,
                    "ruby" => ScriptLanguage::Ruby,
                    "lua" => ScriptLanguage::Lua,
                    _ => ScriptLanguage::Custom {
                        interpreter: sandboxed.runtime.clone(),
                        file_ext: "tmp".to_string(),
                    },
                };

                Program::ScriptSource {
                    language,
                    source: sandboxed.source.clone(),
                }
            };

            let mut sandbox_config = crate::sandbox::SandboxConfig::from_sandboxed_capability(
                sandboxed,
                Some(context.capability_id.to_string()),
            );

            // Override with any explicit execution context metadata overrides
            let context_secrets = parse_csv_list(context.metadata, "sandbox_required_secrets");
            if !context_secrets.is_empty() {
                sandbox_config.required_secrets = context_secrets;
            }
            let context_hosts = parse_csv_set(context.metadata, "sandbox_allowed_hosts");
            if !context_hosts.is_empty() {
                sandbox_config.allowed_hosts = context_hosts;
            }
            let context_ports = parse_csv_ports(context.metadata, "sandbox_allowed_ports");
            if !context_ports.is_empty() {
                sandbox_config.allowed_ports = context_ports;
            }

            if let Some(raw) = context.metadata.get("sandbox_filesystem") {
                if let Ok(fs) = parse_json_metadata::<crate::sandbox::VirtualFilesystem>(
                    raw,
                    "sandbox_filesystem",
                ) {
                    sandbox_config.filesystem = Some(fs);
                }
            }

            if let Some(raw) = context.metadata.get("sandbox_resources") {
                if let Ok(res) =
                    parse_json_metadata::<crate::sandbox::ResourceLimits>(raw, "sandbox_resources")
                {
                    sandbox_config.resources = Some(res);
                }
            }

            let result = self
                .manager
                .execute(&sandbox_config, program, vec![inputs.clone()])
                .await?;
            let metrics = ResourceMetrics::from_microvm_metadata(&result.metadata);
            Ok(attach_usage(result.value, &metrics))
        } else {
            Err(RuntimeError::Generic("Invalid provider type".to_string()))
        }
    }
}

fn parse_json_metadata<T: DeserializeOwned>(raw: &str, key: &str) -> RuntimeResult<T> {
    serde_json::from_str(raw)
        .map_err(|e| RuntimeError::Generic(format!("Failed to parse {} metadata: {}", key, e)))
}

fn attach_usage(value: Value, metrics: &ResourceMetrics) -> Value {
    let mut usage_map: HashMap<MapKey, Value> = HashMap::new();

    if metrics.cpu_time_ms > 0 {
        usage_map.insert(
            MapKey::String("cpu_time_ms".to_string()),
            Value::Integer(metrics.cpu_time_ms as i64),
        );
    }
    if metrics.memory_peak_mb > 0 {
        usage_map.insert(
            MapKey::String("memory_peak_mb".to_string()),
            Value::Integer(metrics.memory_peak_mb as i64),
        );
    }
    if metrics.wall_clock_ms > 0 {
        usage_map.insert(
            MapKey::String("wall_clock_ms".to_string()),
            Value::Integer(metrics.wall_clock_ms as i64),
        );
    }
    if metrics.network_egress_bytes > 0 {
        usage_map.insert(
            MapKey::String("network_egress_bytes".to_string()),
            Value::Integer(metrics.network_egress_bytes as i64),
        );
    }
    if metrics.storage_write_bytes > 0 {
        usage_map.insert(
            MapKey::String("storage_write_bytes".to_string()),
            Value::Integer(metrics.storage_write_bytes as i64),
        );
    }

    if usage_map.is_empty() {
        return value;
    }

    let usage_key = MapKey::String("usage".to_string());

    match value {
        Value::Map(mut map) => {
            match map.get_mut(&usage_key) {
                Some(Value::Map(existing)) => {
                    for (k, v) in usage_map {
                        existing.entry(k).or_insert(v);
                    }
                }
                _ => {
                    map.insert(usage_key, Value::Map(usage_map));
                }
            }
            Value::Map(map)
        }
        other => {
            let mut map = HashMap::new();
            map.insert(MapKey::String("result".to_string()), other);
            map.insert(usage_key, Value::Map(usage_map));
            Value::Map(map)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::{FilesystemMode, MountMode, ResourceLimits, VirtualFilesystem};
    use std::path::PathBuf;

    #[test]
    fn test_parse_json_metadata_filesystem() {
        let json = r#"{
            "mounts": [
                {"host_path": "/tmp/data", "guest_path": "/data", "mode": "ReadOnly"}
            ],
            "quota_mb": 128,
            "mode": "Ephemeral"
        }"#;

        let fs: VirtualFilesystem =
            parse_json_metadata(json, "sandbox_filesystem").expect("filesystem metadata parses");

        assert_eq!(fs.quota_mb, 128);
        assert_eq!(fs.mode, FilesystemMode::Ephemeral);
        assert_eq!(fs.mounts.len(), 1);
        assert_eq!(fs.mounts[0].guest_path, "/data");
        assert_eq!(fs.mounts[0].mode, MountMode::ReadOnly);
        assert_eq!(fs.mounts[0].host_path, PathBuf::from("/tmp/data"));
    }

    #[test]
    fn test_parse_json_metadata_resources() {
        let json = r#"{
            "cpu_shares": 128,
            "memory_mb": 256,
            "timeout_ms": 5000,
            "network_egress_bytes": 4096
        }"#;

        let limits: ResourceLimits =
            parse_json_metadata(json, "sandbox_resources").expect("resource limits parse");

        assert_eq!(limits.cpu_shares, 128);
        assert_eq!(limits.memory_mb, 256);
        assert_eq!(limits.timeout_ms, 5000);
        assert_eq!(limits.network_egress_bytes, 4096);
    }

    #[test]
    fn test_attach_usage_merges_without_overwrite() {
        let metrics = ResourceMetrics {
            cpu_time_ms: 10,
            memory_peak_mb: 20,
            wall_clock_ms: 30,
            network_egress_bytes: 40,
            storage_write_bytes: 50,
        };

        let mut existing_usage = HashMap::new();
        existing_usage.insert(
            MapKey::String("network_egress_bytes".to_string()),
            Value::Integer(5),
        );
        let mut base_map = HashMap::new();
        base_map.insert(
            MapKey::String("usage".to_string()),
            Value::Map(existing_usage),
        );
        base_map.insert(
            MapKey::String("payload".to_string()),
            Value::String("ok".to_string()),
        );

        let updated = attach_usage(Value::Map(base_map), &metrics);
        let Value::Map(updated_map) = updated else {
            panic!("expected map result");
        };
        let Value::Map(usage_map) = updated_map
            .get(&MapKey::String("usage".to_string()))
            .expect("usage present")
        else {
            panic!("usage is map");
        };

        assert!(matches!(
            usage_map.get(&MapKey::String("network_egress_bytes".to_string())),
            Some(Value::Integer(5))
        ));
        assert!(matches!(
            usage_map.get(&MapKey::String("storage_write_bytes".to_string())),
            Some(Value::Integer(50))
        ));
    }

    #[test]
    fn test_attach_usage_wraps_non_map() {
        let metrics = ResourceMetrics {
            cpu_time_ms: 1,
            memory_peak_mb: 2,
            wall_clock_ms: 3,
            network_egress_bytes: 4,
            storage_write_bytes: 5,
        };

        let updated = attach_usage(Value::String("ok".to_string()), &metrics);
        let Value::Map(updated_map) = updated else {
            panic!("expected map result");
        };

        assert!(updated_map.contains_key(&MapKey::String("result".to_string())));
        assert!(updated_map.contains_key(&MapKey::String("usage".to_string())));
    }
}

fn parse_csv_list(metadata: &HashMap<String, String>, key: &str) -> Vec<String> {
    metadata
        .get(key)
        .map(|value| {
            value
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn parse_csv_set(metadata: &HashMap<String, String>, key: &str) -> HashSet<String> {
    parse_csv_list(metadata, key).into_iter().collect()
}

fn parse_csv_ports(metadata: &HashMap<String, String>, key: &str) -> HashSet<u16> {
    metadata
        .get(key)
        .map(|value| {
            value
                .split(',')
                .filter_map(|s| s.trim().parse::<u16>().ok())
                .collect()
        })
        .unwrap_or_default()
}

pub enum ExecutorVariant {
    MCP(MCPExecutor),
    A2A(A2AExecutor),
    Local(LocalExecutor),
    Http(HttpExecutor),
    OpenApi(OpenApiExecutor),
    Registry(RegistryExecutor),
    Native(NativeCapabilityProvider),
    Sandboxed(SandboxedExecutor),
}

impl ExecutorVariant {
    pub async fn execute(
        &self,
        provider: &ProviderType,
        inputs: &Value,
        context: &ExecutionContext<'_>,
    ) -> RuntimeResult<Value> {
        match self {
            ExecutorVariant::MCP(e) => e.execute(provider, inputs, context).await,
            ExecutorVariant::A2A(e) => e.execute(provider, inputs, context).await,
            ExecutorVariant::Local(e) => e.execute(provider, inputs, context).await,
            ExecutorVariant::Http(e) => e.execute(provider, inputs, context).await,
            ExecutorVariant::OpenApi(e) => e.execute(provider, inputs, context).await,
            ExecutorVariant::Registry(e) => e.execute(provider, inputs, context).await,
            ExecutorVariant::Sandboxed(e) => e.execute(provider, inputs, context).await,
            ExecutorVariant::Native(_native_provider) => {
                // For Native capabilities, the provider type contains the capability ID
                // and we dispatch through the NativeCapabilityProvider
                if let ProviderType::Native(native) = provider {
                    // Execute the native capability handler directly
                    (native.handler)(inputs).await
                } else {
                    Err(RuntimeError::Generic(
                        "ProviderType mismatch for Native executor".to_string(),
                    ))
                }
            }
        }
    }
}
