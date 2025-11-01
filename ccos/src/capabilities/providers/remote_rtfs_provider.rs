use crate::capabilities::provider::{
    CapabilityDescriptor, CapabilityProvider, ExecutionContext, HealthStatus, Permission,
    ProviderMetadata, ResourceLimits, SecurityRequirements, NetworkAccess,
};
use rtfs::ast::{PrimitiveType, TypeExpr};
use rtfs::runtime::{RuntimeError, RuntimeResult, Value};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for a remote RTFS endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteRTFSConfig {
    /// Remote RTFS endpoint URL
    pub endpoint: String,
    /// Authentication token (Bearer token)
    pub auth_token: Option<String>,
    /// Request timeout in milliseconds
    pub timeout_ms: u64,
    /// Whether to use TLS/SSL
    pub use_tls: bool,
}

/// Request payload for remote RTFS execution
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RemoteRTFSRequest {
    /// RTFS source code to execute
    code: String,
    /// Context data to pass to remote execution
    context: HashMap<String, serde_json::Value>,
    /// Security constraints
    security_context: SecurityContext,
}

/// Security context for remote execution
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SecurityContext {
    /// Allowed capabilities on remote side
    allowed_capabilities: Vec<String>,
    /// Maximum execution time in milliseconds
    max_execution_time_ms: Option<u64>,
    /// Maximum memory usage in bytes
    max_memory_bytes: Option<u64>,
    /// Isolation level
    isolation_level: String,
}

/// Response from remote RTFS execution
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RemoteRTFSResponse {
    /// Success status
    success: bool,
    /// Result value (serialized as JSON)
    result: Option<serde_json::Value>,
    /// Error message if failed
    error: Option<String>,
    /// Execution metadata
    metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug)]
pub struct RemoteRTFSProvider {
    client: reqwest::Client,
    default_config: RemoteRTFSConfig,
}

impl RemoteRTFSProvider {
    pub fn new(config: RemoteRTFSConfig) -> RuntimeResult<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(config.timeout_ms))
            .build()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self {
            client,
            default_config: config,
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
                permissions: vec![Permission::NetworkAccess("*".to_string())],
                requires_microvm: true,
                resource_limits: ResourceLimits {
                    max_memory: Some(512 * 1024 * 1024), // 512 MB
                    max_cpu_time: Some(std::time::Duration::from_millis(30000)),
                    max_disk_space: None,
                },
                network_access: NetworkAccess::AllowedHosts(vec![]),
            },
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Execute RTFS code on remote endpoint
    async fn execute_remote(
        &self,
        code: &str,
        context: HashMap<String, serde_json::Value>,
        security_context: SecurityContext,
    ) -> RuntimeResult<Value> {
        let request = RemoteRTFSRequest {
            code: code.to_string(),
            context,
            security_context,
        };

        let mut req = self
            .client
            .post(format!("{}/execute", self.default_config.endpoint))
            .json(&request);

        // Add authentication if configured
        if let Some(ref token) = self.default_config.auth_token {
            req = req.bearer_auth(token);
        }

        let response = req
            .send()
            .await
            .map_err(|e| RuntimeError::IoError(format!("Remote RTFS request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(RuntimeError::IoError(format!(
                "Remote RTFS execution failed with status: {}",
                response.status()
            )));
        }

        let remote_response: RemoteRTFSResponse = response.json().await.map_err(|e| {
            RuntimeError::IoError(format!("Failed to parse remote response: {}", e))
        })?;

        if !remote_response.success {
            return Err(RuntimeError::Generic(
                remote_response
                    .error
                    .unwrap_or_else(|| "Remote execution failed".to_string()),
            ));
        }

        // Convert JSON result back to RTFS Value
        if let Some(result_json) = remote_response.result {
            Self::json_to_value(&result_json)
        } else {
            Ok(Value::Nil)
        }
    }

    /// Convert JSON value to RTFS value
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
                "Cannot serialize {} to JSON for remote execution",
                value.type_name()
            ))),
        }
    }

    /// Execute capability: ccos.remote.execute
    fn execute_rtfs_remote(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() < 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "ccos.remote.execute".to_string(),
                expected: "2-4".to_string(),
                actual: args.len(),
            });
        }

        let endpoint = args[0]
            .as_string()
            .ok_or_else(|| RuntimeError::TypeError {
                expected: "string".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "ccos.remote.execute (endpoint)".to_string(),
            })?;

        let code = args[1]
            .as_string()
            .ok_or_else(|| RuntimeError::TypeError {
                expected: "string".to_string(),
                actual: args[1].type_name().to_string(),
                operation: "ccos.remote.execute (code)".to_string(),
            })?;

        // Optional context (arg 2)
        let context = if args.len() > 2 {
            match &args[2] {
                Value::Map(m) => {
                    let mut ctx = HashMap::new();
                    for (k, v) in m {
                        if let rtfs::ast::MapKey::String(key) = k {
                            ctx.insert(key.clone(), Self::value_to_json(v)?);
                        }
                    }
                    ctx
                }
                _ => HashMap::new(),
            }
        } else {
            HashMap::new()
        };

        // Optional auth token (arg 3)
        let auth_token = if args.len() > 3 {
            args[3].as_string().map(|s| s.to_string())
        } else {
            None
        };

        // Create temporary config with endpoint
        let config = RemoteRTFSConfig {
            endpoint: endpoint.to_string(),
            auth_token,
            timeout_ms: 30000,
            use_tls: endpoint.starts_with("https://"),
        };

        let provider = Self::new(config)?;
        let security_context = SecurityContext {
            allowed_capabilities: vec![],
            max_execution_time_ms: Some(30000),
            max_memory_bytes: Some(512 * 1024 * 1024),
            isolation_level: "sandboxed".to_string(),
        };

        // Execute remotely (need to use tokio runtime)
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create runtime: {}", e)))?;
        rt.block_on(provider.execute_remote(code, context, security_context))
    }
}

impl CapabilityProvider for RemoteRTFSProvider {
    fn provider_id(&self) -> &str {
        "remote-rtfs"
    }

    fn list_capabilities(&self) -> Vec<CapabilityDescriptor> {
        vec![
            Self::descriptor(
                "ccos.remote.execute",
                "Execute RTFS code on remote CCOS endpoint",
                vec![
                    TypeExpr::Primitive(PrimitiveType::String), // endpoint
                    TypeExpr::Primitive(PrimitiveType::String), // code
                    TypeExpr::Any,                              // optional context
                    TypeExpr::Primitive(PrimitiveType::String), // optional auth_token
                ],
                TypeExpr::Any,
            ),
            Self::descriptor(
                "ccos.remote.ping",
                "Check if remote RTFS endpoint is available",
                vec![TypeExpr::Primitive(PrimitiveType::String)], // endpoint
                TypeExpr::Primitive(PrimitiveType::Bool),
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
            "ccos.remote.execute" => Self::execute_rtfs_remote(args),
            "ccos.remote.ping" => {
                if args.len() != 1 {
                    return Err(RuntimeError::ArityMismatch {
                        function: "ccos.remote.ping".to_string(),
                        expected: "1".to_string(),
                        actual: args.len(),
                    });
                }
                let _endpoint = args[0].as_string().ok_or_else(|| RuntimeError::TypeError {
                    expected: "string".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "ccos.remote.ping".to_string(),
                })?;
                // Simple ping - return true for now
                Ok(Value::Boolean(true))
            }
            other => Err(RuntimeError::Generic(format!(
                "RemoteRTFSProvider does not support capability {}",
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
            name: "Remote RTFS Provider".to_string(),
            version: "0.1.0".to_string(),
            description: "Execute RTFS code on remote CCOS endpoints with security propagation".to_string(),
            author: "CCOS".to_string(),
            license: Some("MIT".to_string()),
            dependencies: vec!["reqwest".to_string(), "serde_json".to_string()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remote_rtfs_provider_creation() {
        let config = RemoteRTFSConfig {
            endpoint: "http://localhost:8080".to_string(),
            auth_token: None,
            timeout_ms: 5000,
            use_tls: false,
        };

        let provider = RemoteRTFSProvider::new(config);
        assert!(provider.is_ok());
    }

    #[test]
    fn test_json_to_value_conversion() {
        let json = serde_json::json!({
            "name": "test",
            "count": 42,
            "enabled": true
        });

        let value = RemoteRTFSProvider::json_to_value(&json).unwrap();
        match value {
            Value::Map(m) => {
                assert_eq!(m.len(), 3);
            }
            _ => panic!("Expected map"),
        }
    }

    #[test]
    fn test_value_to_json_conversion() {
        let mut map = HashMap::new();
        map.insert(
            rtfs::ast::MapKey::String("name".to_string()),
            Value::String("test".to_string()),
        );
        map.insert(
            rtfs::ast::MapKey::String("count".to_string()),
            Value::Integer(42),
        );

        let value = Value::Map(map);
        let json = RemoteRTFSProvider::value_to_json(&value).unwrap();

        assert!(json.is_object());
        assert_eq!(json["name"], "test");
        assert_eq!(json["count"], 42);
    }
}

