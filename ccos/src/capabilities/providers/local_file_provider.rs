use crate::capabilities::provider::{
    CapabilityDescriptor, CapabilityProvider, ExecutionContext, HealthStatus, Permission,
    ProviderMetadata, ResourceLimits, SecurityRequirements,
};
use crate::ops::fs;
use rtfs::ast::{MapKey, PrimitiveType, TypeExpr};
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

    fn extract_path(input: &Value) -> RuntimeResult<String> {
        // More robust extraction that handles:
        // 1. Value::String (direct path)
        // 2. Value::Map containing "path" or "key" (normalized)
        // 3. Value::Vector/List where the first element is a Map containing these (wrapped normalized)
        
        match input {
            Value::String(s) => Ok(s.clone()),
            Value::Map(map) => {
                // Try "path" or "key" (for KV-like semantics if used here)
                for key_name in &["path", "key", "key-path"] {
                    if let Some(val) = map.get(&MapKey::String(key_name.to_string()))
                        .or_else(|| map.get(&MapKey::Keyword(rtfs::ast::Keyword(key_name.to_string())))) 
                    {
                        if let Some(s) = val.as_string() {
                            return Ok(s.to_string());
                        }
                    }
                }
                
                // Fallback to "args" if it exists (legacy wrapping)
                if let Some(args_val) = map.get(&MapKey::String("args".to_string()))
                    .or_else(|| map.get(&MapKey::Keyword(rtfs::ast::Keyword("args".to_string()))))
                {
                    return Self::extract_path(args_val);
                }
                
                Err(RuntimeError::Generic(format!("Missing 'path' parameter in map. Keys: {:?}", map.keys().collect::<Vec<_>>())))
            }
            Value::List(args) | Value::Vector(args) if !args.is_empty() => {
                if args.len() == 1 {
                    // Try to extract from the first argument recursively
                    // This handles cases like [Value::Map(...)] or [Value::String(...)]
                    return Self::extract_path(&args[0]);
                }
                
                // For multi-arg, usually the first one is the path
                if let Some(s) = args[0].as_string() {
                    Ok(s.to_string())
                } else {
                    // But if it's a map, try extracting from it
                    if let Value::Map(_) = &args[0] {
                        return Self::extract_path(&args[0]);
                    }
                    
                    Err(RuntimeError::TypeError {
                        expected: "string".to_string(),
                        actual: args[0].type_name().to_string(),
                        operation: "extract_path".to_string(),
                    })
                }
            }
            _ => Err(RuntimeError::Generic(format!("Invalid input for filesystem operation: expected string or map with 'path', got {}", input.type_name()))),
        }
    }

    fn extract_bool(input: &Value, key: &str, default: bool) -> bool {
        match input {
            Value::Map(map) => {
                if let Some(val) = map.get(&MapKey::String(key.to_string()))
                    .or_else(|| map.get(&MapKey::Keyword(rtfs::ast::Keyword(key.to_string()))))
                {
                    if let Value::Boolean(b) = val {
                        return *b;
                    }
                }
                default
            }
            Value::List(args) | Value::Vector(args) if !args.is_empty() => {
                // Try extracting from the first arg if it's a map
                if let Value::Map(_) = &args[0] {
                    return Self::extract_bool(&args[0], key, default);
                }
                default
            }
            _ => default
        }
    }

    fn extract_content(input: &Value) -> RuntimeResult<String> {
        match input {
            Value::String(s) => Ok(s.clone()),
            Value::Map(map) => {
                // Try "content", "value", or "data"
                for key_name in &["content", "value", "data"] {
                    if let Some(val) = map.get(&MapKey::String(key_name.to_string()))
                        .or_else(|| map.get(&MapKey::Keyword(rtfs::ast::Keyword(key_name.to_string())))) 
                    {
                        if let Some(s) = val.as_string() {
                            return Ok(s.to_string());
                        }
                    }
                }
                
                // Fallback to "args"
                if let Some(args_val) = map.get(&MapKey::String("args".to_string()))
                    .or_else(|| map.get(&MapKey::Keyword(rtfs::ast::Keyword("args".to_string()))))
                {
                    return Self::extract_content(args_val);
                }
                
                Err(RuntimeError::Generic(format!("Missing 'content' parameter in map. Keys: {:?}", map.keys().collect::<Vec<_>>())))
            }
            Value::List(args) | Value::Vector(args) if !args.is_empty() => {
                if args.len() == 1 {
                    return Self::extract_content(&args[0]);
                }
                
                // Traditionally for write-file(path, content), content is args[1]
                if args.len() >= 2 {
                    if let Some(s) = args[1].as_string() {
                        return Ok(s.to_string());
                    }
                }
                
                // If it's a map at args[0], try extracting from it
                if let Value::Map(_) = &args[0] {
                    return Self::extract_content(&args[0]);
                }
                
                Err(RuntimeError::Generic("Missing 'content' parameter in arguments".to_string()))
            }
            _ => Err(RuntimeError::Generic(format!("Invalid input for write operation: expected string or map, got {}", input.type_name()))),
        }
    }

    fn list_dir(input: &Value) -> RuntimeResult<Value> {
        let path = Self::extract_path(input)?;
        let entries = fs::list_dir(&path)?;
        let mut result = Vec::new();
        for entry in entries {
            let mut map = std::collections::HashMap::new();
            map.insert(
                MapKey::Keyword(rtfs::ast::Keyword("name".to_string())),
                Value::String(entry.name),
            );
            map.insert(
                MapKey::Keyword(rtfs::ast::Keyword("path".to_string())),
                Value::String(entry.path),
            );
            map.insert(
                MapKey::Keyword(rtfs::ast::Keyword("is_dir".to_string())),
                Value::Boolean(entry.is_dir),
            );
            map.insert(
                MapKey::Keyword(rtfs::ast::Keyword("is_file".to_string())),
                Value::Boolean(entry.is_file),
            );
            map.insert(
                MapKey::Keyword(rtfs::ast::Keyword("size".to_string())),
                Value::Integer(entry.size as i64),
            );
            result.push(Value::Map(map));
        }
        Ok(Value::Vector(result))
    }

    fn read_file(input: &Value) -> RuntimeResult<Value> {
        let path = Self::extract_path(input)?;
        let content = fs::read_file(&path)?;
        Ok(Value::String(content))
    }

    fn write_file(input: &Value) -> RuntimeResult<Value> {
        let path = Self::extract_path(input)?;
        let content = Self::extract_content(input)?;
        fs::write_file(&path, &content)?;
        Ok(Value::Boolean(true))
    }

    fn delete(input: &Value) -> RuntimeResult<Value> {
        let path = Self::extract_path(input)?;
        let recursive = Self::extract_bool(input, "recursive", false);
        let deleted = fs::delete(&path, recursive)?;
        Ok(Value::Boolean(deleted))
    }

    fn exists(input: &Value) -> RuntimeResult<Value> {
        let path = Self::extract_path(input)?;
        Ok(Value::Boolean(fs::exists(&path)))
    }

    fn mkdir(input: &Value) -> RuntimeResult<Value> {
        let path = Self::extract_path(input)?;
        let recursive = Self::extract_bool(input, "recursive", false);
        fs::mkdir(&path, recursive)?;
        Ok(Value::Boolean(true))
    }

    fn read_file_base64(input: &Value) -> RuntimeResult<Value> {
        let path = Self::extract_path(input)?;
        let content = fs::read_file_bytes(&path)?;
        use base64::Engine;
        Ok(Value::String(
            base64::engine::general_purpose::STANDARD.encode(content),
        ))
    }

    fn write_file_base64(input: &Value) -> RuntimeResult<Value> {
        let path = Self::extract_path(input)?;
        let content_b64 = Self::extract_content(input)?;
        use base64::Engine;
        let content = base64::engine::general_purpose::STANDARD
            .decode(&content_b64)
            .map_err(|e| {
                RuntimeError::Generic(format!("Failed to decode base64 content: {}", e))
            })?;
        fs::write_file_bytes(&path, &content)?;
        Ok(Value::Boolean(true))
    }
}

impl CapabilityProvider for LocalFileProvider {
    fn provider_id(&self) -> &str {
        "local-file"
    }

    fn list_capabilities(&self) -> Vec<CapabilityDescriptor> {
        vec![
            Self::descriptor(
                "ccos.fs.list",
                "List contents of a directory. Returns a list of objects with: \
                 :name, :path, :is_dir, :is_file, :size. \
                 Use for exploring the filesystem or finding files. \
                 NOT for listing registered MCP servers.",
                1,
                false,
                vec![Permission::FileRead(std::path::PathBuf::from("/"))],
            ),
            Self::descriptor(
                "ccos.fs.read",
                "Read the content of a file as a string. \
                 Use for viewing configuration, logs, or source code. \
                 NOT for reading MCP server registry data.",
                1,
                true,
                vec![Permission::FileRead(std::path::PathBuf::from("/"))],
            ),
            Self::descriptor(
                "ccos.fs.write",
                "Write string content to a file. \
                 Use for saving logs, creating configuration files, or updating code. \
                 NOT for writing to the MCP server registry.",
                2,
                false,
                vec![
                    Permission::FileWrite(std::path::PathBuf::from("/")),
                    Permission::FileRead(std::path::PathBuf::from("/")),
                ],
            ),
            Self::descriptor(
                "ccos.fs.delete",
                "Delete a file or directory. \
                 Params: :path (required), :recursive (bool, optional). \
                 Use to remove temporary files, builds, or unwanted directories. \
                 NOT for unregistering MCP servers.",
                1,
                false,
                vec![Permission::FileWrite(std::path::PathBuf::from("/"))],
            ),
            Self::descriptor(
                "ccos.fs.exists",
                "Check if a file or directory exists at the given path. \
                 Use for verifying path availability before other operations. \
                 NOT for checking if an MCP server is registered.",
                1,
                false,
                vec![Permission::FileRead(std::path::PathBuf::from("/"))],
            ),
            Self::descriptor(
                "ccos.fs.mkdir",
                "Create a new directory. \
                 Params: :path (required), :recursive (bool, optional). \
                 Use for setting up workspace structures.",
                1,
                false,
                vec![Permission::FileWrite(std::path::PathBuf::from("/"))],
            ),
            Self::descriptor(
                "ccos.fs.read-base64",
                "Read the content of a file as a base64 encoded string. \
                 Use for reading binary files like images, PDFs, etc.",
                1,
                true,
                vec![Permission::FileRead(std::path::PathBuf::from("/"))],
            ),
            Self::descriptor(
                "ccos.fs.write-base64",
                "Write base64 encoded content to a file. \
                 Use for writing binary files like images, PDFs, etc.",
                2,
                false,
                vec![
                    Permission::FileWrite(std::path::PathBuf::from("/")),
                    Permission::FileRead(std::path::PathBuf::from("/")),
                ],
            ),
        ]
    }

    fn execute_capability(
        &self,
        capability_id: &str,
        inputs: &Value,
        _context: &ExecutionContext,
    ) -> RuntimeResult<Value> {
        match capability_id {
            "ccos.fs.list" | "ccos.io.list-dir" => Self::list_dir(inputs),
            "ccos.fs.read" | "ccos.io.read-file" => Self::read_file(inputs),
            "ccos.fs.write" | "ccos.io.write-file" => Self::write_file(inputs),
            "ccos.fs.read-base64" | "ccos.io.read-file-base64" => Self::read_file_base64(inputs),
            "ccos.fs.write-base64" | "ccos.io.write-file-base64" => Self::write_file_base64(inputs),
            "ccos.fs.delete" | "ccos.io.delete-file" => Self::delete(inputs),
            "ccos.fs.exists" | "ccos.io.file-exists" => Self::exists(inputs),
            "ccos.fs.mkdir" | "ccos.io.mkdir" => Self::mkdir(inputs),
            other => Err(RuntimeError::Generic(format!(
                "LocalFileProvider does not support capability {}",
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
            name: "Local File Provider".to_string(),
            version: "0.2.0".to_string(),
            description: "Executes local filesystem operations".to_string(),
            author: "CCOS".to_string(),
            license: Some("MIT".to_string()),
            dependencies: vec![],
        }
    }
}
