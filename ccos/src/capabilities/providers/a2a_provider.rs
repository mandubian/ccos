use crate::capabilities::provider::{
    CapabilityDescriptor, CapabilityProvider, ExecutionContext, HealthStatus, Permission,
    ProviderMetadata, ResourceLimits, SecurityRequirements, NetworkAccess,
};
use rtfs::ast::{PrimitiveType, TypeExpr};
use rtfs::runtime::{RuntimeError, RuntimeResult, Value};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for Agent-to-Agent (A2A) communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2AConfig {
    /// Agent ID of the target agent
    pub agent_id: String,
    /// Agent endpoint URL
    pub endpoint: String,
    /// Communication protocol (http, grpc, websocket)
    pub protocol: String,
    /// Authentication token
    pub auth_token: Option<String>,
    /// Request timeout in milliseconds
    pub timeout_ms: u64,
}

/// A2A message request
#[derive(Debug, Clone, Serialize, Deserialize)]
struct A2ARequest {
    /// Source agent ID
    source_agent_id: String,
    /// Target agent ID
    target_agent_id: String,
    /// Message type (query, command, event, response)
    message_type: String,
    /// Message payload
    payload: serde_json::Value,
    /// Correlation ID for tracking
    correlation_id: Option<String>,
    /// Security context
    security_context: A2ASecurityContext,
}

/// Security context for A2A communication
#[derive(Debug, Clone, Serialize, Deserialize)]
struct A2ASecurityContext {
    /// Sender's identity/credentials
    sender_identity: String,
    /// Required capabilities for the receiver
    required_capabilities: Vec<String>,
    /// Trust level (low, medium, high, verified)
    trust_level: String,
    /// Encryption enabled
    encrypted: bool,
}

/// A2A message response
#[derive(Debug, Clone, Serialize, Deserialize)]
struct A2AResponse {
    /// Success status
    success: bool,
    /// Response payload
    payload: Option<serde_json::Value>,
    /// Error message if failed
    error: Option<String>,
    /// Response metadata
    metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug)]
pub struct A2AProvider {
    client: reqwest::Client,
    default_config: A2AConfig,
    source_agent_id: String,
}

impl A2AProvider {
    pub fn new(config: A2AConfig, source_agent_id: String) -> RuntimeResult<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(config.timeout_ms))
            .build()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self {
            client,
            default_config: config,
            source_agent_id,
        })
    }

    fn descriptor(
        id: &str,
        description: &str,
        param_types: Vec<TypeExpr>,
        return_type: TypeExpr,
    ) -> CapabilityDescriptor {
        CapabilityDescriptor {
            id: id.to_string(),
            description: description.to_string(),
            capability_type: CapabilityDescriptor::constrained_function_type(
                param_types,
                return_type,
                None,
            ),
            security_requirements: SecurityRequirements {
                permissions: vec![Permission::NetworkAccess("*".to_string()), Permission::AgentCommunication],
                requires_microvm: true,
                resource_limits: ResourceLimits {
                    max_memory: Some(256 * 1024 * 1024), // 256 MB
                    max_cpu_time: Some(std::time::Duration::from_millis(10000)),
                    max_disk_space: None,
                },
                network_access: NetworkAccess::AllowedHosts(vec![]),
            },
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Send a message to another agent
    async fn send_message(
        &self,
        target_agent_id: &str,
        message_type: &str,
        payload: serde_json::Value,
        security_context: A2ASecurityContext,
    ) -> RuntimeResult<Value> {
        let request = A2ARequest {
            source_agent_id: self.source_agent_id.clone(),
            target_agent_id: target_agent_id.to_string(),
            message_type: message_type.to_string(),
            payload,
            correlation_id: Some(uuid::Uuid::new_v4().to_string()),
            security_context,
        };

        let endpoint = if self.default_config.protocol == "http" {
            format!("{}/a2a/message", self.default_config.endpoint)
        } else {
            return Err(RuntimeError::Generic(format!(
                "Unsupported A2A protocol: {}",
                self.default_config.protocol
            )));
        };

        let mut req = self.client.post(&endpoint).json(&request);

        // Add authentication if configured
        if let Some(ref token) = self.default_config.auth_token {
            req = req.bearer_auth(token);
        }

        let response = req
            .send()
            .await
            .map_err(|e| RuntimeError::IoError(format!("A2A request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(RuntimeError::IoError(format!(
                "A2A communication failed with status: {}",
                response.status()
            )));
        }

        let a2a_response: A2AResponse = response.json().await.map_err(|e| {
            RuntimeError::IoError(format!("Failed to parse A2A response: {}", e))
        })?;

        if !a2a_response.success {
            return Err(RuntimeError::Generic(
                a2a_response
                    .error
                    .unwrap_or_else(|| "A2A communication failed".to_string()),
            ));
        }

        // Convert JSON result back to RTFS Value
        if let Some(result_json) = a2a_response.payload {
            Self::json_to_value(&result_json)
        } else {
            Ok(Value::Nil)
        }
    }

    /// Convert JSON value to RTFS value (same as RemoteRTFS)
    fn json_to_value(value: &serde_json::Value) -> RuntimeResult<Value> {
        match value {
            serde_json::Value::Null => Ok(Value::Nil),
            serde_json::Value::Bool(b) => Ok(Value::Boolean(*b)),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Ok(Value::Integer(i))
                } else if let Some(f) = n.as_f64() {
                    Ok(Value::Float(f))
                } else {
                    Ok(Value::Nil)
                }
            }
            serde_json::Value::String(s) => Ok(Value::String(s.clone())),
            serde_json::Value::Array(arr) => {
                let mut result = Vec::new();
                for item in arr {
                    result.push(Self::json_to_value(item)?);
                }
                Ok(Value::Vector(result))
            }
            serde_json::Value::Object(map) => {
                let mut result = std::collections::HashMap::new();
                for (k, v) in map {
                    result.insert(rtfs::ast::MapKey::String(k.clone()), Self::json_to_value(v)?);
                }
                Ok(Value::Map(result))
            }
        }
    }

    /// Convert RTFS value to JSON value
    fn value_to_json(value: &Value) -> RuntimeResult<serde_json::Value> {
        match value {
            Value::Nil => Ok(serde_json::Value::Null),
            Value::Boolean(b) => Ok(serde_json::Value::Bool(*b)),
            Value::Integer(i) => Ok(serde_json::Value::Number((*i).into())),
            Value::Float(f) => serde_json::Number::from_f64(*f)
                .map(serde_json::Value::Number)
                .ok_or_else(|| RuntimeError::Generic("Invalid float value".to_string())),
            Value::String(s) => Ok(serde_json::Value::String(s.clone())),
            Value::Vector(vec) | Value::List(vec) => {
                let mut out = Vec::new();
                for item in vec {
                    out.push(Self::value_to_json(item)?);
                }
                Ok(serde_json::Value::Array(out))
            }
            Value::Map(map) => {
                let mut obj = serde_json::Map::new();
                for (key, val) in map {
                    let key_str = match key {
                        rtfs::ast::MapKey::String(s) => s.clone(),
                        rtfs::ast::MapKey::Keyword(k) => format!(":{}", k.0),
                        _ => format!("{:?}", key),
                    };
                    obj.insert(key_str, Self::value_to_json(val)?);
                }
                Ok(serde_json::Value::Object(obj))
            }
            Value::Keyword(k) => Ok(serde_json::Value::String(format!(":{}", k.0))),
            Value::Symbol(s) => Ok(serde_json::Value::String(s.0.clone())),
            _ => Err(RuntimeError::Generic(format!(
                "Cannot serialize {} to JSON for A2A communication",
                value.type_name()
            ))),
        }
    }

    /// Execute capability: ccos.a2a.send
    fn send_a2a_message(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() < 3 {
            return Err(RuntimeError::ArityMismatch {
                function: "ccos.a2a.send".to_string(),
                expected: "3-4".to_string(),
                actual: args.len(),
            });
        }

        let agent_id = args[0]
            .as_string()
            .ok_or_else(|| RuntimeError::TypeError {
                expected: "string".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "ccos.a2a.send (agent_id)".to_string(),
            })?;

        let message_type = args[1]
            .as_string()
            .ok_or_else(|| RuntimeError::TypeError {
                expected: "string".to_string(),
                actual: args[1].type_name().to_string(),
                operation: "ccos.a2a.send (message_type)".to_string(),
            })?;

        // Payload (convert to JSON)
        let payload = Self::value_to_json(&args[2])?;

        // Optional auth token (arg 3)
        let auth_token = if args.len() > 3 {
            args[3].as_string().map(|s| s.to_string())
        } else {
            None
        };

        // Create temporary config
        let config = A2AConfig {
            agent_id: agent_id.to_string(),
            endpoint: format!("http://localhost:8080/agent/{}", agent_id), // Default endpoint pattern
            protocol: "http".to_string(),
            auth_token,
            timeout_ms: 10000,
        };

        let provider = Self::new(config, "local-agent".to_string())?;
        let security_context = A2ASecurityContext {
            sender_identity: "local-agent".to_string(),
            required_capabilities: vec![],
            trust_level: "medium".to_string(),
            encrypted: false,
        };

        // Send message (need to use tokio runtime)
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create runtime: {}", e)))?;
        rt.block_on(provider.send_message(agent_id, message_type, payload, security_context))
    }

    /// Execute capability: ccos.a2a.query
    fn query_agent(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() < 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "ccos.a2a.query".to_string(),
                expected: "2-3".to_string(),
                actual: args.len(),
            });
        }

        let agent_id = args[0]
            .as_string()
            .ok_or_else(|| RuntimeError::TypeError {
                expected: "string".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "ccos.a2a.query (agent_id)".to_string(),
            })?;

        let query = args[1]
            .as_string()
            .ok_or_else(|| RuntimeError::TypeError {
                expected: "string".to_string(),
                actual: args[1].type_name().to_string(),
                operation: "ccos.a2a.query (query)".to_string(),
            })?;

        // Create query payload
        let payload = serde_json::json!({
            "query": query,
            "timestamp": chrono::Utc::now().to_rfc3339()
        });

        // Optional auth token (arg 2)
        let auth_token = if args.len() > 2 {
            args[2].as_string().map(|s| s.to_string())
        } else {
            None
        };

        let config = A2AConfig {
            agent_id: agent_id.to_string(),
            endpoint: format!("http://localhost:8080/agent/{}", agent_id),
            protocol: "http".to_string(),
            auth_token,
            timeout_ms: 10000,
        };

        let provider = Self::new(config, "local-agent".to_string())?;
        let security_context = A2ASecurityContext {
            sender_identity: "local-agent".to_string(),
            required_capabilities: vec![],
            trust_level: "medium".to_string(),
            encrypted: false,
        };

        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create runtime: {}", e)))?;
        rt.block_on(provider.send_message(agent_id, "query", payload, security_context))
    }
}

impl CapabilityProvider for A2AProvider {
    fn provider_id(&self) -> &str {
        "a2a"
    }

    fn list_capabilities(&self) -> Vec<CapabilityDescriptor> {
        vec![
            Self::descriptor(
                "ccos.a2a.send",
                "Send message to another agent",
                vec![
                    TypeExpr::Primitive(PrimitiveType::String), // agent_id
                    TypeExpr::Primitive(PrimitiveType::String), // message_type
                    TypeExpr::Any,                              // payload
                    TypeExpr::Primitive(PrimitiveType::String), // optional auth_token
                ],
                TypeExpr::Any,
            ),
            Self::descriptor(
                "ccos.a2a.query",
                "Query another agent",
                vec![
                    TypeExpr::Primitive(PrimitiveType::String), // agent_id
                    TypeExpr::Primitive(PrimitiveType::String), // query
                    TypeExpr::Primitive(PrimitiveType::String), // optional auth_token
                ],
                TypeExpr::Any,
            ),
            Self::descriptor(
                "ccos.a2a.discover",
                "Discover available agents",
                vec![TypeExpr::Primitive(PrimitiveType::String)], // optional filter
                TypeExpr::Any,
            ),
        ]
    }

    fn execute_capability(
        &self,
        capability_id: &str,
        inputs: &Value,
        _context: &ExecutionContext,
    ) -> RuntimeResult<Value> {
        let args = match inputs {
            Value::Vector(vec) => vec,
            Value::List(list) => list,
            single => std::slice::from_ref(single),
        };

        match capability_id {
            "ccos.a2a.send" => Self::send_a2a_message(args),
            "ccos.a2a.query" => Self::query_agent(args),
            "ccos.a2a.discover" => {
                // Return mock agent list for now
                let mut agents_map = std::collections::HashMap::new();
                agents_map.insert(
                    rtfs::ast::MapKey::String("agents".to_string()),
                    Value::Vector(vec![]),
                );
                Ok(Value::Map(agents_map))
            }
            other => Err(RuntimeError::Generic(format!(
                "A2AProvider does not support capability {}",
                other
            ))),
        }
    }

    fn initialize(&mut self, _config: &crate::capabilities::provider::ProviderConfig) -> Result<(), String> {
        Ok(())
    }

    fn health_check(&self) -> HealthStatus {
        HealthStatus::Healthy
    }

    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            name: "A2A Provider".to_string(),
            version: "0.1.0".to_string(),
            description: "Agent-to-Agent communication with security validation".to_string(),
            author: "CCOS".to_string(),
            license: Some("MIT".to_string()),
            dependencies: vec!["reqwest".to_string(), "serde_json".to_string(), "uuid".to_string()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_a2a_provider_creation() {
        let config = A2AConfig {
            agent_id: "agent-123".to_string(),
            endpoint: "http://localhost:8080/agent/agent-123".to_string(),
            protocol: "http".to_string(),
            auth_token: None,
            timeout_ms: 5000,
        };

        let provider = A2AProvider::new(config, "source-agent".to_string());
        assert!(provider.is_ok());
    }

    #[test]
    fn test_a2a_list_capabilities() {
        let config = A2AConfig {
            agent_id: "agent-123".to_string(),
            endpoint: "http://localhost:8080/agent/agent-123".to_string(),
            protocol: "http".to_string(),
            auth_token: None,
            timeout_ms: 5000,
        };

        let provider = A2AProvider::new(config, "source-agent".to_string()).unwrap();
        let capabilities = provider.list_capabilities();
        
        assert_eq!(capabilities.len(), 3);
        assert!(capabilities.iter().any(|c| c.id == "ccos.a2a.send"));
        assert!(capabilities.iter().any(|c| c.id == "ccos.a2a.query"));
        assert!(capabilities.iter().any(|c| c.id == "ccos.a2a.discover"));
    }

    #[test]
    fn test_json_value_conversions() {
        let json = serde_json::json!({
            "agent_id": "test-agent",
            "status": "active",
            "capabilities": ["cap1", "cap2"]
        });

        let value = A2AProvider::json_to_value(&json).unwrap();
        match &value {
            Value::Map(m) => {
                assert_eq!(m.len(), 3);
            }
            _ => panic!("Expected map"),
        }

        // Test reverse conversion
        let json_back = A2AProvider::value_to_json(&value).unwrap();
        assert!(json_back.is_object());
        assert_eq!(json_back["agent_id"], "test-agent");
    }

    #[test]
    fn test_security_context_creation() {
        let security_ctx = A2ASecurityContext {
            sender_identity: "agent-001".to_string(),
            required_capabilities: vec!["read".to_string(), "write".to_string()],
            trust_level: "high".to_string(),
            encrypted: true,
        };

        assert_eq!(security_ctx.sender_identity, "agent-001");
        assert_eq!(security_ctx.required_capabilities.len(), 2);
        assert_eq!(security_ctx.trust_level, "high");
        assert!(security_ctx.encrypted);
    }
}
