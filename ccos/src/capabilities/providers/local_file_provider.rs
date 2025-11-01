use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use crate::capabilities::provider::{CapabilityDescriptor, CapabilityProvider, ExecutionContext, HealthStatus, Permission, ProviderMetadata, ResourceLimits, SecurityRequirements};
use rtfs::ast::{PrimitiveType, TypeExpr};
use rtfs::runtime::{RuntimeError, RuntimeResult, Value};

#[derive(Debug, Default)]
pub struct LocalFileProvider;

impl LocalFileProvider {
    fn descriptor(
        id: &str,
        description: &str,
        arity: usize,
        returns_string: bool,
        permissions: Vec<Permission>,
    ) -> CapabilityDescriptor {
        let param_types = std::iter::repeat(TypeExpr::Primitive(PrimitiveType::String))
            .take(arity)
            .collect();
        let return_type = if returns_string {
            TypeExpr::Primitive(PrimitiveType::String)
        } else {
            TypeExpr::Primitive(PrimitiveType::Bool)
        };
        CapabilityDescriptor {
            id: id.to_string(),
            description: description.to_string(),
            capability_type: CapabilityDescriptor::constrained_function_type(
                param_types,
                return_type,
                None,
            ),
            security_requirements: SecurityRequirements {
                permissions,
                requires_microvm: true,
                resource_limits: ResourceLimits {
                    max_memory: None,
                    max_cpu_time: None,
                    max_disk_space: None,
                },
                network_access: crate::capabilities::provider::NetworkAccess::None,
            },
            metadata: std::collections::HashMap::new(),
        }
    }

    fn read_file(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "ccos.io.read-file".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        let path = args[0]
            .as_string()
            .ok_or_else(|| RuntimeError::TypeError {
                expected: "string".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "ccos.io.read-file".to_string(),
            })?;
        if path.is_empty() {
            return Err(RuntimeError::InvalidArgument(
                "File path must not be empty".to_string(),
            ));
        }
        let mut file = fs::File::open(Path::new(path)).map_err(|e| RuntimeError::IoError(e.to_string()))?;
        let mut content = String::new();
        file.read_to_string(&mut content)
            .map_err(|e| RuntimeError::IoError(e.to_string()))?;
        Ok(Value::String(content))
    }

    fn write_file(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "ccos.io.write-file".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }
        let path = args[0]
            .as_string()
            .ok_or_else(|| RuntimeError::TypeError {
                expected: "string".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "ccos.io.write-file".to_string(),
            })?;
        if path.is_empty() {
            return Err(RuntimeError::InvalidArgument(
                "File path must not be empty".to_string(),
            ));
        }
        let content = args[1]
            .as_string()
            .ok_or_else(|| RuntimeError::TypeError {
                expected: "string".to_string(),
                actual: args[1].type_name().to_string(),
                operation: "ccos.io.write-file".to_string(),
            })?;
        let mut file = fs::File::create(Path::new(path)).map_err(|e| RuntimeError::IoError(e.to_string()))?;
        file.write_all(content.as_bytes())
            .map_err(|e| RuntimeError::IoError(e.to_string()))?;
        Ok(Value::Boolean(true))
    }

    fn delete_file(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "ccos.io.delete-file".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        let path = args[0]
            .as_string()
            .ok_or_else(|| RuntimeError::TypeError {
                expected: "string".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "ccos.io.delete-file".to_string(),
            })?;
        if path.is_empty() {
            return Err(RuntimeError::InvalidArgument(
                "File path must not be empty".to_string(),
            ));
        }
        let path_ref = Path::new(path);
        if !path_ref.exists() {
            return Ok(Value::Boolean(false));
        }
        fs::remove_file(path_ref).map_err(|e| RuntimeError::IoError(e.to_string()))?;
        Ok(Value::Boolean(true))
    }

    fn file_exists(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "ccos.io.file-exists".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        let path = args[0]
            .as_string()
            .ok_or_else(|| RuntimeError::TypeError {
                expected: "string".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "ccos.io.file-exists".to_string(),
            })?;
        if path.is_empty() {
            return Err(RuntimeError::InvalidArgument(
                "File path must not be empty".to_string(),
            ));
        }
        Ok(Value::Boolean(Path::new(path).exists()))
    }
}

impl CapabilityProvider for LocalFileProvider {
    fn provider_id(&self) -> &str {
        "local-file"
    }

    fn list_capabilities(&self) -> Vec<CapabilityDescriptor> {
        vec![
            Self::descriptor(
                "ccos.io.file-exists",
                "Checks if a file is present",
                1,
                false,
                vec![Permission::FileRead(std::path::PathBuf::from("/"))],
            ),
            Self::descriptor(
                "ccos.io.read-file",
                "Reads file contents as string",
                1,
                true,
                vec![Permission::FileRead(std::path::PathBuf::from("/"))],
            ),
            Self::descriptor(
                "ccos.io.write-file",
                "Writes string content to file",
                2,
                false,
                vec![
                    Permission::FileWrite(std::path::PathBuf::from("/")),
                    Permission::FileRead(std::path::PathBuf::from("/")),
                ],
            ),
            Self::descriptor(
                "ccos.io.delete-file",
                "Deletes the specified file",
                1,
                false,
                vec![Permission::FileWrite(std::path::PathBuf::from("/"))],
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
            "ccos.io.file-exists" => Self::file_exists(args),
            "ccos.io.read-file" => Self::read_file(args),
            "ccos.io.write-file" => Self::write_file(args),
            "ccos.io.delete-file" => Self::delete_file(args),
            other => Err(RuntimeError::Generic(format!(
                "LocalFileProvider does not support capability {}",
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
            name: "Local File Provider".to_string(),
            version: "0.1.0".to_string(),
            description: "Executes local file operations for development".to_string(),
            author: "CCOS".to_string(),
            license: Some("MIT".to_string()),
            dependencies: vec![],
        }
    }
}
