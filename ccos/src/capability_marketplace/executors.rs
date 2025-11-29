use super::types::*;
use crate::utils::value_conversion;
use async_trait::async_trait;
use regex::Regex;
use reqwest;
use rtfs::ast::MapKey;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use serde_json::json;
use std::any::TypeId;
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Duration;
use urlencoding::encode;

#[async_trait(?Send)]
pub trait CapabilityExecutor: Send + Sync {
    fn provider_type_id(&self) -> TypeId;
    async fn execute(&self, provider: &ProviderType, inputs: &Value) -> RuntimeResult<Value>;
}

use crate::capabilities::SessionPoolManager;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct MCPExecutor {
    pub session_pool: Arc<RwLock<Option<Arc<SessionPoolManager>>>>,
}

#[async_trait(?Send)]
impl CapabilityExecutor for MCPExecutor {
    fn provider_type_id(&self) -> TypeId {
        TypeId::of::<MCPCapability>()
    }
    async fn execute(&self, provider: &ProviderType, inputs: &Value) -> RuntimeResult<Value> {
        if let ProviderType::MCP(mcp) = provider {
            // eprintln!("[MCPExecutor] Executing tool '{}' on server '{}'", mcp.tool_name, mcp.server_url);
            // eprintln!("[MCPExecutor] Inputs type: {}", inputs.type_name());

            // Resolve auth token: inputs > provider > env
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
                .or_else(|| std::env::var("MCP_AUTH_TOKEN").ok());

            // Try to use SessionPoolManager if available
            let guard = self.session_pool.read().await;
            if let Some(pool) = guard.as_ref() {
                // eprintln!("[MCPExecutor] Using SessionPoolManager");
                let mut metadata = HashMap::new();
                metadata.insert("mcp_server_url".to_string(), mcp.server_url.clone());
                metadata.insert("mcp_tool_name".to_string(), mcp.tool_name.clone());
                metadata.insert("mcp_timeout_ms".to_string(), mcp.timeout_ms.to_string());

                if let Some(token) = &auth_token {
                    metadata.insert("mcp_auth_token".to_string(), token.clone());
                }

                let pool_clone = pool.clone();
                let tool_name = mcp.tool_name.clone();
                let inputs_clone = inputs.clone();

                // Execute via session pool
                return pool_clone.execute_with_session(&tool_name, &metadata, &[inputs_clone]);
            }

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

            let mut init_builder = client.post(&mcp.server_url);
            if let Some(token) = &auth_token {
                init_builder = init_builder.header("Authorization", format!("Bearer {}", token));
            }

            let init_response = init_builder
                .json(&init_request)
                .timeout(Duration::from_millis(mcp.timeout_ms))
                .send()
                .await
                .map_err(|e| RuntimeError::Generic(format!("MCP initialization failed: {}", e)))?;

            let init_json: serde_json::Value = init_response.json().await.map_err(|e| {
                RuntimeError::Generic(format!("Failed to parse MCP init response: {}", e))
            })?;

            // Check for initialization error
            if let Some(error) = init_json.get("error") {
                return Err(RuntimeError::Generic(format!(
                    "MCP initialization error: {:?}",
                    error
                )));
            }

            // Extract session ID if provided (some servers use it)
            let session_id = init_json
                .get("result")
                .and_then(|r| r.get("sessionId"))
                .and_then(|s| s.as_str())
                .map(String::from);

            let tool_name = if mcp.tool_name.is_empty() || mcp.tool_name == "*" {
                let tools_request = json!({"jsonrpc":"2.0","id":"tools_discovery","method":"tools/list","params":{}});
                let mut request_builder = client.post(&mcp.server_url);
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
                let tools_response: serde_json::Value = response.json().await.map_err(|e| {
                    RuntimeError::Generic(format!("Failed to parse MCP response: {}", e))
                })?;
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
            let mut request_builder = client.post(&mcp.server_url);
            if let Some(token) = &auth_token {
                request_builder =
                    request_builder.header("Authorization", format!("Bearer {}", token));
            }
            if let Some(ref sid) = session_id {
                request_builder = request_builder.header("Mcp-Session-Id", sid);
            }

            let response = request_builder
                .json(&tool_request)
                .timeout(std::time::Duration::from_millis(mcp.timeout_ms))
                .send()
                .await
                .map_err(|e| RuntimeError::Generic(format!("Failed to execute MCP tool: {}", e)))?;
            let tool_response: serde_json::Value = response.json().await.map_err(|e| {
                RuntimeError::Generic(format!("Failed to parse tool response: {}", e))
            })?;
            if let Some(error) = tool_response.get("error") {
                return Err(RuntimeError::Generic(format!(
                    "MCP tool execution failed: {:?}",
                    error
                )));
            }
            if let Some(result) = tool_response.get("result") {
                if let Some(content) = result.get("content") {
                    CapabilityMarketplace::json_to_rtfs_value(content)
                } else {
                    CapabilityMarketplace::json_to_rtfs_value(result)
                }
            } else {
                Err(RuntimeError::Generic(
                    "No result in MCP tool response".to_string(),
                ))
            }
        } else {
            Err(RuntimeError::Generic(
                "Invalid provider type for MCP executor".to_string(),
            ))
        }
    }
}

pub struct A2AExecutor;

#[async_trait(?Send)]
impl CapabilityExecutor for A2AExecutor {
    fn provider_type_id(&self) -> TypeId {
        TypeId::of::<A2ACapability>()
    }
    async fn execute(&self, provider: &ProviderType, inputs: &Value) -> RuntimeResult<Value> {
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
            Self::json_to_rtfs_value(result)
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
    
    fn json_to_rtfs_value(json: &serde_json::Value) -> RuntimeResult<Value> {
        value_conversion::json_to_rtfs_value(json)
    }
}

pub struct LocalExecutor;

#[async_trait(?Send)]
impl CapabilityExecutor for LocalExecutor {
    fn provider_type_id(&self) -> TypeId {
        TypeId::of::<LocalCapability>()
    }
    async fn execute(&self, provider: &ProviderType, inputs: &Value) -> RuntimeResult<Value> {
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

#[async_trait(?Send)]
impl CapabilityExecutor for RegistryExecutor {
    fn provider_type_id(&self) -> TypeId {
        TypeId::of::<RegistryCapability>()
    }
    async fn execute(&self, provider: &ProviderType, inputs: &Value) -> RuntimeResult<Value> {
        if let ProviderType::Registry(registry_cap) = provider {
            let args = match inputs {
                Value::List(list) => list.clone(),
                Value::Vector(vec) => vec.clone(),
                v => vec![v.clone()],
            };
            // Note: RTFS stub registry doesn't support execute_capability_with_microvm
            // Registry capabilities should be migrated to proper CCOS providers
            // For now, return an error
            Err(RuntimeError::Generic(format!(
                "Registry capability '{}' not supported - RTFS/CCOS separation in progress",
                registry_cap.capability_id
            )))
        } else {
            Err(RuntimeError::Generic(
                "ProviderType mismatch for RegistryExecutor".to_string(),
            ))
        }
    }
}

pub struct OpenApiExecutor;

#[async_trait(?Send)]
impl CapabilityExecutor for OpenApiExecutor {
    fn provider_type_id(&self) -> TypeId {
        TypeId::of::<OpenApiCapability>()
    }

    async fn execute(&self, provider: &ProviderType, inputs: &Value) -> RuntimeResult<Value> {
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
                if let Ok(rtfs_json) = CapabilityMarketplace::json_to_rtfs_value(&json_value) {
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
        auth.env_var_name
            .as_ref()
            .and_then(|env| std::env::var(env).ok())
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

#[async_trait(?Send)]
impl CapabilityExecutor for HttpExecutor {
    fn provider_type_id(&self) -> TypeId {
        TypeId::of::<HttpCapability>()
    }
    async fn execute(&self, provider: &ProviderType, inputs: &Value) -> RuntimeResult<Value> {
        if let ProviderType::Http(http) = provider {
            // Test-only shortcut pattern kept here if needed in tests
            // Normal path mirrors marketplace HTTP logic
            let args = match inputs {
                Value::List(list) => list.clone(),
                Value::Vector(vec) => vec.clone(),
                v => vec![v.clone()],
            };
            let url = args
                .get(0)
                .and_then(|v| v.as_string())
                .unwrap_or(&http.base_url);
            let method = args.get(1).and_then(|v| v.as_string()).unwrap_or("GET");
            let default_headers = std::collections::HashMap::new();
            let headers = args
                .get(2)
                .and_then(|v| match v {
                    Value::Map(m) => Some(m),
                    _ => None,
                })
                .unwrap_or(&default_headers);
            let body = args
                .get(3)
                .and_then(|v| v.as_string())
                .unwrap_or("")
                .to_string();
            let client = reqwest::Client::new();
            let method_enum =
                reqwest::Method::from_bytes(method.as_bytes()).unwrap_or(reqwest::Method::GET);
            let mut req = client.request(method_enum, url);
            if let Some(token) = &http.auth_token {
                req = req.bearer_auth(token);
            }
            for (k, v) in headers.iter() {
                if let MapKey::String(ref key) = k {
                    if let Value::String(ref val) = v {
                        req = req.header(key, val);
                    }
                }
            }
            if !body.is_empty() {
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
            let mut response_map = std::collections::HashMap::new();
            response_map.insert(MapKey::String("status".to_string()), Value::Integer(status));
            response_map.insert(MapKey::String("body".to_string()), Value::String(resp_body));
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

pub enum ExecutorVariant {
    MCP(MCPExecutor),
    A2A(A2AExecutor),
    Local(LocalExecutor),
    Http(HttpExecutor),
    OpenApi(OpenApiExecutor),
    Registry(RegistryExecutor),
}

impl ExecutorVariant {
    pub async fn execute(&self, provider: &ProviderType, inputs: &Value) -> RuntimeResult<Value> {
        match self {
            ExecutorVariant::MCP(e) => e.execute(provider, inputs).await,
            ExecutorVariant::A2A(e) => e.execute(provider, inputs).await,
            ExecutorVariant::Local(e) => e.execute(provider, inputs).await,
            ExecutorVariant::Http(e) => e.execute(provider, inputs).await,
            ExecutorVariant::OpenApi(e) => e.execute(provider, inputs).await,
            ExecutorVariant::Registry(e) => e.execute(provider, inputs).await,
        }
    }
}
