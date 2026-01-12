use crate::capabilities::provider::{
    CapabilityProvider, ExecutionContext, HealthStatus, NetworkAccess, ProviderMetadata,
    ResourceLimits, SecurityRequirements,
};
use rtfs::ast::{PrimitiveType, TypeExpr};
use rtfs::runtime::{RuntimeError, RuntimeResult, Value};

#[derive(Debug, Default)]
pub struct JsonProvider;

impl JsonProvider {
    fn descriptor(
        id: &str,
        description: &str,
        return_type: TypeExpr,
    ) -> crate::capabilities::provider::CapabilityDescriptor {
        crate::capabilities::provider::CapabilityDescriptor {
            id: id.to_string(),
            description: description.to_string(),
            capability_type:
                crate::capabilities::provider::CapabilityDescriptor::constrained_function_type(
                    vec![TypeExpr::Primitive(PrimitiveType::String)],
                    return_type,
                    None,
                ),
            security_requirements: SecurityRequirements {
                permissions: vec![],
                requires_microvm: false,
                resource_limits: ResourceLimits {
                    max_memory: None,
                    max_cpu_time: None,
                    max_disk_space: None,
                },
                network_access: NetworkAccess::None,
            },
            metadata: std::collections::HashMap::new(),
        }
    }

    fn parse(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "ccos.json.parse".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        let json_str = args[0].as_string().ok_or_else(|| RuntimeError::TypeError {
            expected: "string".to_string(),
            actual: args[0].type_name().to_string(),
            operation: "ccos.json.parse".to_string(),
        })?;
        let parsed: serde_json::Value = serde_json::from_str(json_str)
            .map_err(|e| RuntimeError::Generic(format!("JSON parsing error: {}", e)))?;
        Ok(Self::json_to_value(&parsed))
    }

    fn stringify(args: &[Value], pretty: bool) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: if pretty {
                    "ccos.json.stringify-pretty".to_string()
                } else {
                    "ccos.json.stringify".to_string()
                },
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        let json_value = Self::value_to_json(&args[0])?;
        let output = if pretty {
            serde_json::to_string_pretty(&json_value)
        } else {
            serde_json::to_string(&json_value)
        }
        .map_err(|e| RuntimeError::Generic(format!("JSON serialization error: {}", e)))?;
        Ok(Value::String(output))
    }

    fn json_to_value(value: &serde_json::Value) -> Value {
        match value {
            serde_json::Value::Null => Value::Nil,
            serde_json::Value::Bool(b) => Value::Boolean(*b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Value::Integer(i)
                } else if let Some(f) = n.as_f64() {
                    Value::Float(f)
                } else {
                    Value::Nil
                }
            }
            serde_json::Value::String(s) => Value::String(s.clone()),
            serde_json::Value::Array(arr) => {
                Value::Vector(arr.iter().map(Self::json_to_value).collect())
            }
            serde_json::Value::Object(map) => {
                let mut rtfs_map = std::collections::HashMap::new();
                for (k, v) in map.iter() {
                    rtfs_map.insert(
                        rtfs::ast::MapKey::Keyword(rtfs::ast::Keyword(k.clone())),
                        Self::json_to_value(v),
                    );
                }
                Value::Map(rtfs_map)
            }
        }
    }

    fn value_to_json(value: &Value) -> RuntimeResult<serde_json::Value> {
        match value {
            Value::Nil => Ok(serde_json::Value::Null),
            Value::Boolean(b) => Ok(serde_json::Value::Bool(*b)),
            Value::Integer(i) => Ok(serde_json::Value::Number((*i).into())),
            Value::Float(f) => serde_json::Number::from_f64(*f)
                .map(serde_json::Value::Number)
                .ok_or_else(|| RuntimeError::Generic("Invalid float value".to_string())),
            Value::String(s) => Ok(serde_json::Value::String(s.clone())),
            Value::Vector(vec) => {
                let mut out = Vec::with_capacity(vec.len());
                for item in vec {
                    out.push(Self::value_to_json(item)?);
                }
                Ok(serde_json::Value::Array(out))
            }
            Value::Map(map) => {
                let mut obj = serde_json::Map::new();
                for (key, val) in map.iter() {
                    let key_str = match key {
                        rtfs::ast::MapKey::String(s) => s.clone(),
                        rtfs::ast::MapKey::Keyword(k) => format!(":{}", k.0),
                        other => format!("{:?}", other),
                    };
                    obj.insert(key_str, Self::value_to_json(val)?);
                }
                Ok(serde_json::Value::Object(obj))
            }
            Value::Keyword(k) => Ok(serde_json::Value::String(format!(":{}", k.0))),
            Value::Symbol(s) => Ok(serde_json::Value::String(s.0.clone())),
            Value::Timestamp(ts) => Ok(serde_json::Value::String(format!("@{}", ts))),
            Value::Uuid(uuid) => Ok(serde_json::Value::String(format!("@{}", uuid))),
            Value::ResourceHandle(handle) => Ok(serde_json::Value::String(format!("@{}", handle))),
            Value::Function(_) | Value::FunctionPlaceholder(_) => Err(RuntimeError::Generic(
                "Cannot serialize functions to JSON".to_string(),
            )),
            Value::Error(err) => Err(RuntimeError::Generic(format!(
                "Cannot serialize runtime errors to JSON: {}",
                err.message
            ))),
            Value::List(list) => {
                let mut out = Vec::with_capacity(list.len());
                for item in list {
                    out.push(Self::value_to_json(item)?);
                }
                Ok(serde_json::Value::Array(out))
            }
        }
    }
}

impl CapabilityProvider for JsonProvider {
    fn provider_id(&self) -> &str {
        "local-json"
    }

    fn list_capabilities(&self) -> Vec<crate::capabilities::provider::CapabilityDescriptor> {
        vec![
            Self::descriptor(
                "ccos.json.parse",
                "Parse JSON string into RTFS value",
                TypeExpr::Any,
            ),
            Self::descriptor(
                "ccos.json.stringify",
                "Serialize RTFS value into JSON string",
                TypeExpr::Primitive(PrimitiveType::String),
            ),
            Self::descriptor(
                "ccos.json.stringify-pretty",
                "Serialize RTFS value into pretty-formatted JSON string",
                TypeExpr::Primitive(PrimitiveType::String),
            ),
            Self::descriptor(
                "ccos.data.parse-json",
                "Backwards-compatible alias for JSON parse",
                TypeExpr::Any,
            ),
            Self::descriptor(
                "ccos.data.serialize-json",
                "Backwards-compatible alias for JSON stringify",
                TypeExpr::Primitive(PrimitiveType::String),
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
            "ccos.json.parse" | "ccos.data.parse-json" => Self::parse(args),
            "ccos.json.stringify" | "ccos.data.serialize-json" => Self::stringify(args, false),
            "ccos.json.stringify-pretty" => Self::stringify(args, true),
            other => Err(RuntimeError::Generic(format!(
                "JsonProvider does not support capability {}",
                other
            ))),
        }
    }

    fn initialize(
        &mut self,
        _config: &crate::capabilities::provider::ProviderConfig,
    ) -> Result<(), String> {
        Ok(())
    }

    fn health_check(&self) -> HealthStatus {
        HealthStatus::Healthy
    }

    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            name: "JSON Provider".to_string(),
            version: "0.1.0".to_string(),
            description: "Local JSON parsing/stringifying capabilities".to_string(),
            author: "CCOS".to_string(),
            license: Some("MIT".to_string()),
            dependencies: vec![],
        }
    }
}
