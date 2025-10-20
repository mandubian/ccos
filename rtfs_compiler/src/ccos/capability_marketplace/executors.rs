use super::types::*;
use crate::ast::MapKey;
use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::runtime::values::Value;
use async_trait::async_trait;
use reqwest;
use serde_json::json;
use std::any::TypeId;
use std::collections::HashMap;

#[async_trait(?Send)]
pub trait CapabilityExecutor: Send + Sync {
    fn provider_type_id(&self) -> TypeId;
    async fn execute(&self, provider: &ProviderType, inputs: &Value) -> RuntimeResult<Value>;
}

pub struct MCPExecutor;

#[async_trait(?Send)]
impl CapabilityExecutor for MCPExecutor {
    fn provider_type_id(&self) -> TypeId {
        TypeId::of::<MCPCapability>()
    }
    async fn execute(&self, provider: &ProviderType, inputs: &Value) -> RuntimeResult<Value> {
        if let ProviderType::MCP(mcp) = provider {
            // Convert RTFS Value to JSON, preserving string/keyword map keys
            let input_json = A2AExecutor::value_to_json(inputs)
                .map_err(|e| RuntimeError::Generic(format!("Failed to serialize inputs: {}", e)))?;
            let client = reqwest::Client::new();
            let tool_name = if mcp.tool_name.is_empty() || mcp.tool_name == "*" {
                let tools_request = json!({"jsonrpc":"2.0","id":"tools_discovery","method":"tools/list","params":{}});
                let response = client
                    .post(&mcp.server_url)
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
            let response = client
                .post(&mcp.server_url)
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
    fn value_to_json(value: &Value) -> Result<serde_json::Value, RuntimeError> {
        use serde_json::Value as JsonValue;
        match value {
            Value::Integer(i) => Ok(JsonValue::Number(serde_json::Number::from(*i))),
            Value::Float(f) => Ok(JsonValue::Number(
                serde_json::Number::from_f64(*f)
                    .ok_or_else(|| RuntimeError::Generic("Invalid float value".to_string()))?,
            )),
            Value::String(s) => Ok(JsonValue::String(s.clone())),
            Value::Boolean(b) => Ok(JsonValue::Bool(*b)),
            Value::Vector(vec) => Ok(JsonValue::Array(
                vec.iter()
                    .map(|v| Self::value_to_json(v))
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            Value::Map(map) => {
                let mut json_map = serde_json::Map::new();
                for (key, val) in map {
                    let key_str = match key {
                        MapKey::String(s) => s.clone(),
                        MapKey::Keyword(k) => k.0.clone(),
                        _ => {
                            return Err(RuntimeError::Generic(
                                "Map keys must be strings or keywords".to_string(),
                            ))
                        }
                    };
                    json_map.insert(key_str, Self::value_to_json(val)?);
                }
                Ok(JsonValue::Object(json_map))
            }
            Value::Nil => Ok(JsonValue::Null),
            _ => Err(RuntimeError::Generic(format!(
                "Cannot convert {} to JSON",
                value.type_name()
            ))),
        }
    }
    fn json_to_rtfs_value(json: &serde_json::Value) -> RuntimeResult<Value> {
        match json {
            serde_json::Value::String(s) => Ok(Value::String(s.clone())),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Ok(Value::Integer(i))
                } else if let Some(f) = n.as_f64() {
                    Ok(Value::Float(f))
                } else {
                    Err(RuntimeError::Generic("Invalid number format".to_string()))
                }
            }
            serde_json::Value::Bool(b) => Ok(Value::Boolean(*b)),
            serde_json::Value::Array(arr) => Ok(Value::Vector(
                arr.iter()
                    .map(|json| CapabilityMarketplace::json_to_rtfs_value(json))
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            serde_json::Value::Object(obj) => {
                let mut map = HashMap::new();
                for (k, v) in obj {
                    map.insert(
                        crate::ast::MapKey::String(k.clone()),
                        CapabilityMarketplace::json_to_rtfs_value(v)?,
                    );
                }
                Ok(Value::Map(map))
            }
            serde_json::Value::Null => Ok(Value::Nil),
        }
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
            let registry = registry_cap.registry.read().await;
            registry.execute_capability_with_microvm(&registry_cap.capability_id, args, None)
        } else {
            Err(RuntimeError::Generic(
                "ProviderType mismatch for RegistryExecutor".to_string(),
            ))
        }
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
    Registry(RegistryExecutor),
}

impl ExecutorVariant {
    pub async fn execute(&self, provider: &ProviderType, inputs: &Value) -> RuntimeResult<Value> {
        match self {
            ExecutorVariant::MCP(e) => e.execute(provider, inputs).await,
            ExecutorVariant::A2A(e) => e.execute(provider, inputs).await,
            ExecutorVariant::Local(e) => e.execute(provider, inputs).await,
            ExecutorVariant::Http(e) => e.execute(provider, inputs).await,
            ExecutorVariant::Registry(e) => e.execute(provider, inputs).await,
        }
    }
}
