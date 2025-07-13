# CCOS Capability Provider Implementation Guide

## Overview

This guide provides implementation details for the CCOS Extensible Capability Architecture, showing how to create providers for different types of capabilities.

## Core Interfaces

### CapabilityProvider Trait

```rust
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use crate::ast::{TypeExpr, Expression, PrimitiveType, ParamType, Literal, Keyword};

/// Core trait that all capability providers must implement
pub trait CapabilityProvider: Send + Sync + std::fmt::Debug {
    /// Unique identifier for this provider
    fn provider_id(&self) -> &str;
    
    /// List of capabilities this provider offers
    fn list_capabilities(&self) -> Vec<CapabilityDescriptor>;
    
    /// Execute a capability call
    fn execute_capability(
        &self,
        capability_id: &str,
        inputs: &Value,
        context: &ExecutionContext,
    ) -> RuntimeResult<Value>;
    
    /// Initialize the provider with configuration
    fn initialize(&mut self, config: &ProviderConfig) -> Result<(), String>;
    
    /// Check if provider is healthy/available
    fn health_check(&self) -> HealthStatus;
    
    /// Get provider metadata
    fn metadata(&self) -> ProviderMetadata;
}

/// Descriptor for a single capability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityDescriptor {
    /// Unique capability identifier (e.g., "ccos.io.log", "mcp.weather.get-forecast")
    pub id: String,
    /// Human-readable description
    pub description: String,
    /// The capability's function type signature using RTFS TypeExpr
    /// This should be a TypeExpr::Function variant: (param_types...) -> return_type
    /// Use TypeExpr::Intersection ([:and Type Predicate]) for complex constraints
    pub capability_type: TypeExpr,
    /// Security requirements
    pub security_requirements: SecurityRequirements,
    /// Provider-specific metadata
    pub metadata: HashMap<String, String>,
}
```

## RTFS-Native Type Constraints

The CCOS capability system leverages RTFS's native type system for expressing capability signatures and constraints. Instead of using external JSON schemas, all type information is expressed using RTFS `TypeExpr` structures.

### Basic Type Constraints

```rust
// Helper functions for creating constrained types
impl CapabilityDescriptor {
    /// Helper to create a positive integer constraint: [:and int [:> 0]]
    pub fn positive_int_type() -> TypeExpr {
        TypeExpr::Intersection(vec![
            TypeExpr::Primitive(PrimitiveType::Int),
            TypeExpr::Literal(Literal::Keyword(Keyword::new("> 0")))
        ])
    }
    
    /// Helper to create an email string constraint: [:and string [:string-contains "@"]]
    pub fn email_string_type() -> TypeExpr {
        TypeExpr::Intersection(vec![
            TypeExpr::Primitive(PrimitiveType::String),
            TypeExpr::Literal(Literal::Keyword(Keyword::new("string-contains @")))
        ])
    }
    
    /// Helper to create a range-constrained integer: [:and int [:>= min] [:<= max]]
    pub fn range_int_type(min: i64, max: i64) -> TypeExpr {
        TypeExpr::Intersection(vec![
            TypeExpr::Primitive(PrimitiveType::Int),
            TypeExpr::Literal(Literal::Keyword(Keyword::new(&format!(">= {}", min)))),
            TypeExpr::Literal(Literal::Keyword(Keyword::new(&format!("<= {}", max))))
        ])
    }
}
```

### RTFS Type Syntax Examples

```rtfs
;; Basic function type: (string, int) -> bool
[:=> [string int] bool]

;; Constrained function type: (positive_int, positive_int) -> positive_int
[:=> [[:and int [:> 0]] [:and int [:> 0]]] [:and int [:> 0]]]

;; Email validation: (email_string) -> validation_result
[:=> [[:and string [:string-contains "@"] [:string-min-length 5]]] 
     [:enum :valid :invalid]]

;; Complex map validation: (user_map) -> result_map
[:=> [[:map [:name [:and string [:string-min-length 1]]]
            [:age [:and int [:>= 0] [:<= 150]]]
            [:email [:and string [:string-contains "@"]]]]]
     [:map [:status [:enum :success :error]]
           [:message string]]]
```

### Capability Type Validation

The system provides automatic validation of inputs and outputs against the capability type:

```rust
/// Extension trait for CapabilityProvider that adds automatic type validation
pub trait ValidatedCapabilityProvider: CapabilityProvider {
    /// Execute capability with automatic input/output validation
    fn execute_capability_validated(
        &self,
        capability_id: &str,
        inputs: &[Value],
        context: &ExecutionContext,
    ) -> RuntimeResult<Value>;
}
```

### Benefits of RTFS-Native Constraints

1. **Type Safety**: All constraints are checked by RTFS's type system
2. **Composability**: Constraints can be combined using intersection and union types
3. **Reusability**: Type definitions can be shared across capabilities
4. **Static Analysis**: RTFS compiler can validate constraints at compile time
5. **Runtime Validation**: Automatic validation during capability execution
6. **Native Integration**: No external schema languages or JSON validation needed

### Execution Context for capability calls
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    /// Security context for the call
    pub security_context: SecurityContext,
    /// Trace ID for logging/debugging
    pub trace_id: String,
    /// Calling environment information
    pub environment: EnvironmentInfo,
    /// Timeout for the operation
    pub timeout: Duration,
}

/// Provider health status
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthStatus {
    Healthy,
    Degraded(String),
    Unhealthy(String),
}

/// Provider metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderMetadata {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub license: Option<String>,
    pub dependencies: Vec<String>,
}

/// Provider configuration using RTFS Expression
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub config: Expression,
}
```

### Security Requirements

```rust
/// Security requirements for a capability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityRequirements {
    /// Required permissions
    pub permissions: Vec<Permission>,
    /// Whether capability requires microVM execution
    pub requires_microvm: bool,
    /// Resource limits
    pub resource_limits: ResourceLimits,
    /// Network access requirements
    pub network_access: NetworkAccess,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Permission {
    FileRead(PathBuf),
    FileWrite(PathBuf),
    NetworkAccess(String), // URL pattern
    EnvironmentRead(String), // Environment variable pattern
    SystemCommand(String), // Command pattern
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub max_memory: Option<u64>,
    pub max_cpu_time: Option<Duration>,
    pub max_disk_space: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkAccess {
    None,
    Limited(Vec<String>), // Allowed domains/IPs
    Full,
}
```

## Implementation Examples

### 1. System Capability Provider

```rust
/// Built-in system capabilities
#[derive(Debug)]
pub struct SystemCapabilityProvider {
    security_context: SecurityContext,
    config: SystemConfig,
}

impl SystemCapabilityProvider {
    pub fn new(config: SystemConfig) -> Self {
        Self {
            security_context: SecurityContext::system(),
            config,
        }
    }
    
    fn get_env(&self, inputs: &Value, context: &ExecutionContext) -> RuntimeResult<Value> {
        // Check permissions
        context.security_context.check_permission(
            &Permission::EnvironmentRead("*".to_string())
        )?;
        
        let key = inputs.as_string()
            .ok_or_else(|| RuntimeError::TypeError {
                expected: "string".to_string(),
                actual: inputs.type_name().to_string(),
                operation: "get-env".to_string(),
            })?;
        
        match std::env::var(key) {
            Ok(value) => Ok(Value::String(value)),
            Err(_) => Ok(Value::Nil),
        }
    }
    
    fn log(&self, inputs: &Value, context: &ExecutionContext) -> RuntimeResult<Value> {
        let message = format!("{:?}", inputs);
        
        // Log with context information
        log::info!(
            "[{}] [{}] {}",
            context.trace_id,
            context.environment.caller_id,
            message
        );
        
        Ok(Value::Nil)
    }
}

impl CapabilityProvider for SystemCapabilityProvider {
    fn provider_id(&self) -> &str { "ccos.system" }
    
    fn list_capabilities(&self) -> Vec<CapabilityDescriptor> {
        vec![
            CapabilityDescriptor {
                id: "ccos.system.get-env".to_string(),
                description: "Get environment variable".to_string(),
                capability_type: TypeExpr::Function {
                    param_types: vec![ParamType::Simple(Box::new(
                        TypeExpr::Intersection(vec![
                            TypeExpr::Primitive(PrimitiveType::String),
                            TypeExpr::Literal(Literal::Keyword(Keyword::new("string-min-length 1")))
                        ])
                    ))],
                    variadic_param_type: None,
                    return_type: Box::new(TypeExpr::Union(vec![
                        TypeExpr::Primitive(PrimitiveType::String),
                        TypeExpr::Primitive(PrimitiveType::Nil)
                    ])),
                },
                security_requirements: SecurityRequirements {
                    permissions: vec![Permission::EnvironmentRead("*".to_string())],
                    requires_microvm: false,
                    resource_limits: ResourceLimits {
                        max_memory: Some(1024 * 1024), // 1MB
                        max_cpu_time: Some(Duration::from_millis(100)),
                        max_disk_space: None,
                    },
                    network_access: NetworkAccess::None,
                },
                metadata: HashMap::new(),
            },
            CapabilityDescriptor {
                id: "ccos.io.log".to_string(),
                description: "Log a message".to_string(),
                capability_type: TypeExpr::Function {
                    param_types: vec![ParamType::Simple(Box::new(TypeExpr::Any))],
                    variadic_param_type: None,
                    return_type: Box::new(TypeExpr::Primitive(PrimitiveType::Nil)),
                },
                security_requirements: SecurityRequirements {
                    permissions: vec![],
                    requires_microvm: false,
                    resource_limits: ResourceLimits {
                        max_memory: Some(1024 * 1024), // 1MB
                        max_cpu_time: Some(Duration::from_millis(50)),
                        max_disk_space: None,
                    },
                    network_access: NetworkAccess::None,
                },
                metadata: HashMap::new(),
            },
        ]
    }
    
    fn execute_capability(
        &self,
        capability_id: &str,
        inputs: &Value,
        context: &ExecutionContext,
    ) -> RuntimeResult<Value> {
        match capability_id {
            "ccos.system.get-env" => self.get_env(inputs, context),
            "ccos.io.log" => self.log(inputs, context),
            _ => Err(RuntimeError::Generic(format!(
                "Unknown system capability: {}", capability_id
            ))),
        }
    }
    
    fn initialize(&mut self, config: &ProviderConfig) -> Result<(), String> {
        // Initialize with configuration
        Ok(())
    }
    
    fn health_check(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
    
    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            name: "CCOS System Provider".to_string(),
            version: "1.0.0".to_string(),
            description: "Built-in system capabilities".to_string(),
            author: "CCOS Team".to_string(),
            license: Some("MIT".to_string()),
            dependencies: vec![],
        }
    }
}
```

### 2. MCP Capability Provider

```rust
/// Model Context Protocol server integration
#[derive(Debug)]
pub struct MCPCapabilityProvider {
    server_config: MCPServerConfig,
    client: Arc<MCPClient>,
    capabilities_cache: Arc<RwLock<HashMap<String, CapabilityDescriptor>>>,
    last_discovery: Arc<RwLock<Instant>>,
}

impl MCPCapabilityProvider {
    pub fn new(server_config: MCPServerConfig) -> Result<Self, String> {
        let client = MCPClient::connect(&server_config.url)
            .map_err(|e| format!("Failed to connect to MCP server: {}", e))?;
        
        Ok(Self {
            server_config,
            client: Arc::new(client),
            capabilities_cache: Arc::new(RwLock::new(HashMap::new())),
            last_discovery: Arc::new(RwLock::new(Instant::now())),
        })
    }
    
    fn discover_capabilities(&self) -> Result<Vec<CapabilityDescriptor>, String> {
        let tools = self.client.list_tools()
            .map_err(|e| format!("Failed to list MCP tools: {}", e))?;
        
        let capabilities = tools.into_iter().map(|tool| {
            // Convert MCP tool schema to RTFS TypeExpr
            let capability_type = self.mcp_schema_to_rtfs_type(&tool);
            
            CapabilityDescriptor {
                id: format!("mcp.{}.{}", self.server_config.name, tool.name),
                description: tool.description.clone(),
                capability_type,
                security_requirements: SecurityRequirements {
                    permissions: vec![Permission::NetworkAccess(
                        self.server_config.url.clone()
                    )],
                    requires_microvm: self.server_config.requires_microvm,
                    resource_limits: ResourceLimits {
                        max_memory: Some(64 * 1024 * 1024), // 64MB
                        max_cpu_time: Some(Duration::from_secs(30)),
                        max_disk_space: None,
                    },
                    network_access: NetworkAccess::Limited(vec![
                        self.server_config.url.clone()
                    ]),
                },
                metadata: {
                    let mut metadata = HashMap::new();
                    metadata.insert("server".to_string(), self.server_config.name.clone());
                    metadata.insert("tool".to_string(), tool.name.clone());
                    metadata
                },
            }
        }).collect();
        
        // Update cache
        let mut cache = self.capabilities_cache.write().unwrap();
        cache.clear();
        for cap in &capabilities {
            cache.insert(cap.id.clone(), cap.clone());
        }
        *self.last_discovery.write().unwrap() = Instant::now();
        
        Ok(capabilities)
    }
    
    /// Convert MCP tool schema to RTFS TypeExpr
    fn mcp_schema_to_rtfs_type(&self, tool: &MCPTool) -> TypeExpr {
        // This is a simplified conversion - a full implementation would
        // convert JSON Schema to RTFS TypeExpr recursively
        let input_type = self.json_schema_to_rtfs_type(&tool.input_schema);
        let output_type = tool.output_schema.as_ref()
            .map(|s| self.json_schema_to_rtfs_type(s))
            .unwrap_or(TypeExpr::Any);
        
        TypeExpr::Function {
            param_types: vec![ParamType::Simple(Box::new(input_type))],
            variadic_param_type: None,
            return_type: Box::new(output_type),
        }
    }
    
    /// Convert JSON Schema to RTFS TypeExpr (simplified)
    fn json_schema_to_rtfs_type(&self, schema: &serde_json::Value) -> TypeExpr {
        match schema.get("type").and_then(|t| t.as_str()) {
            Some("string") => {
                let mut constraints = vec![TypeExpr::Primitive(PrimitiveType::String)];
                
                // Add string constraints
                if let Some(min_length) = schema.get("minLength").and_then(|v| v.as_u64()) {
                    constraints.push(TypeExpr::Literal(Literal::Keyword(
                        Keyword::new(&format!("string-min-length {}", min_length))
                    )));
                }
                if let Some(pattern) = schema.get("pattern").and_then(|v| v.as_str()) {
                    constraints.push(TypeExpr::Literal(Literal::Keyword(
                        Keyword::new(&format!("string-matches-regex {}", pattern))
                    )));
                }
                
                if constraints.len() > 1 {
                    TypeExpr::Intersection(constraints)
                } else {
                    TypeExpr::Primitive(PrimitiveType::String)
                }
            }
            Some("integer") => {
                let mut constraints = vec![TypeExpr::Primitive(PrimitiveType::Int)];
                
                // Add integer constraints
                if let Some(minimum) = schema.get("minimum").and_then(|v| v.as_i64()) {
                    constraints.push(TypeExpr::Literal(Literal::Keyword(
                        Keyword::new(&format!(">= {}", minimum))
                    )));
                }
                if let Some(maximum) = schema.get("maximum").and_then(|v| v.as_i64()) {
                    constraints.push(TypeExpr::Literal(Literal::Keyword(
                        Keyword::new(&format!("<= {}", maximum))
                    )));
                }
                
                if constraints.len() > 1 {
                    TypeExpr::Intersection(constraints)
                } else {
                    TypeExpr::Primitive(PrimitiveType::Int)
                }
            }
            Some("boolean") => TypeExpr::Primitive(PrimitiveType::Bool),
            Some("number") => TypeExpr::Primitive(PrimitiveType::Float),
            _ => TypeExpr::Any, // Default for unknown or complex types
        }
    }
    
    fn rtfs_to_mcp_args(&self, inputs: &Value) -> Result<serde_json::Value, String> {
        match inputs {
            Value::Map(map) => {
                let mut json_obj = serde_json::Map::new();
                for (key, value) in map {
                    let key_str = match key {
                        crate::ast::MapKey::String(s) => s.clone(),
                        crate::ast::MapKey::Keyword(k) => k.0.clone(),
                        _ => continue,
                    };
                    let json_value = self.rtfs_to_json_value(value)?;
                    json_obj.insert(key_str, json_value);
                }
                Ok(serde_json::Value::Object(json_obj))
            }
            _ => self.rtfs_to_json_value(inputs),
        }
    }
    
    fn rtfs_to_json_value(&self, value: &Value) -> Result<serde_json::Value, String> {
        match value {
            Value::Nil => Ok(serde_json::Value::Null),
            Value::Boolean(b) => Ok(serde_json::Value::Bool(*b)),
            Value::Integer(i) => Ok(serde_json::Value::Number((*i).into())),
            Value::Float(f) => Ok(serde_json::Value::Number(
                serde_json::Number::from_f64(*f).unwrap()
            )),
            Value::String(s) => Ok(serde_json::Value::String(s.clone())),
            Value::Vector(vec) => {
                let json_array: Result<Vec<serde_json::Value>, String> = vec
                    .iter()
                    .map(|v| self.rtfs_to_json_value(v))
                    .collect();
                Ok(serde_json::Value::Array(json_array?))
            }
            _ => Err(format!("Cannot convert {:?} to JSON", value)),
        }
    }
    
    fn mcp_to_rtfs_value(&self, value: serde_json::Value) -> RuntimeResult<Value> {
        match value {
            serde_json::Value::Null => Ok(Value::Nil),
            serde_json::Value::Bool(b) => Ok(Value::Boolean(b)),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Ok(Value::Integer(i))
                } else if let Some(f) = n.as_f64() {
                    Ok(Value::Float(f))
                } else {
                    Ok(Value::Integer(0))
                }
            }
            serde_json::Value::String(s) => Ok(Value::String(s)),
            serde_json::Value::Array(arr) => {
                let values: Result<Vec<Value>, RuntimeError> = arr
                    .into_iter()
                    .map(|v| self.mcp_to_rtfs_value(v))
                    .collect();
                Ok(Value::Vector(values?))
            }
            serde_json::Value::Object(obj) => {
                let mut map = HashMap::new();
                for (key, value) in obj {
                    let map_key = crate::ast::MapKey::String(key);
                    let rtfs_value = self.mcp_to_rtfs_value(value)?;
                    map.insert(map_key, rtfs_value);
                }
                Ok(Value::Map(map))
            }
        }
    }
}

impl CapabilityProvider for MCPCapabilityProvider {
    fn provider_id(&self) -> &str { "mcp" }
    
    fn list_capabilities(&self) -> Vec<CapabilityDescriptor> {
        // Check if cache is stale
        let last_discovery = *self.last_discovery.read().unwrap();
        if last_discovery.elapsed() > Duration::from_secs(300) { // 5 minutes
            if let Ok(capabilities) = self.discover_capabilities() {
                return capabilities;
            }
        }
        
        // Return cached capabilities
        self.capabilities_cache.read().unwrap().values().cloned().collect()
    }
    
    fn execute_capability(
        &self,
        capability_id: &str,
        inputs: &Value,
        context: &ExecutionContext,
    ) -> RuntimeResult<Value> {
        // Extract tool name from capability_id
        let tool_name = capability_id
            .strip_prefix(&format!("mcp.{}.", self.server_config.name))
            .ok_or_else(|| RuntimeError::Generic(format!(
                "Invalid MCP capability ID: {}", capability_id
            )))?;
        
        // Convert RTFS Value to MCP arguments
        let mcp_args = self.rtfs_to_mcp_args(inputs)
            .map_err(|e| RuntimeError::Generic(format!("Failed to convert arguments: {}", e)))?;
        
        // Call MCP server with timeout
        let result = tokio::time::timeout(
            context.timeout,
            self.client.call_tool(tool_name, mcp_args)
        ).await
        .map_err(|_| RuntimeError::Generic("MCP call timed out".to_string()))?
        .map_err(|e| RuntimeError::Generic(format!("MCP call failed: {}", e)))?;
        
        // Convert MCP response back to RTFS Value
        self.mcp_to_rtfs_value(result)
    }
    
    fn initialize(&mut self, config: &ProviderConfig) -> Result<(), String> {
        // Initialize MCP client with auth if configured
        if let Some(auth) = &self.server_config.auth {
            self.client.authenticate(auth)
                .map_err(|e| format!("MCP authentication failed: {}", e))?;
        }
        
        // Discover initial capabilities
        self.discover_capabilities()?;
        
        Ok(())
    }
    
    fn health_check(&self) -> HealthStatus {
        match self.client.ping() {
            Ok(_) => HealthStatus::Healthy,
            Err(e) => HealthStatus::Unhealthy(format!("MCP server unreachable: {}", e)),
        }
    }
    
    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            name: format!("MCP Provider: {}", self.server_config.name),
            version: "1.0.0".to_string(),
            description: format!("Model Context Protocol server at {}", self.server_config.url),
            author: "CCOS Team".to_string(),
            license: Some("MIT".to_string()),
            dependencies: vec!["mcp-client".to_string()],
        }
    }
}
```

### 3. Plugin Capability Provider

```rust
/// WASM plugin capability provider
#[derive(Debug)]
pub struct WASMPluginProvider {
    plugin_path: PathBuf,
    engine: wasmtime::Engine,
    store: wasmtime::Store<()>,
    instance: wasmtime::Instance,
    capabilities: Vec<CapabilityDescriptor>,
}

impl WASMPluginProvider {
    pub fn load(plugin_path: PathBuf) -> Result<Self, String> {
        let engine = wasmtime::Engine::default();
        let mut store = wasmtime::Store::new(&engine, ());
        
        // Load WASM module
        let module = wasmtime::Module::from_file(&engine, &plugin_path)
            .map_err(|e| format!("Failed to load WASM module: {}", e))?;
        
        // Create instance
        let instance = wasmtime::Instance::new(&mut store, &module, &[])
            .map_err(|e| format!("Failed to create WASM instance: {}", e))?;
        
        // Get capabilities from plugin
        let get_capabilities = instance
            .get_typed_func::<(), i32>(&mut store, "get_capabilities")
            .map_err(|e| format!("Plugin missing get_capabilities function: {}", e))?;
        
        let capabilities_ptr = get_capabilities.call(&mut store, ())
            .map_err(|e| format!("Failed to call get_capabilities: {}", e))?;
        
        // Read capabilities from WASM memory
        let capabilities = Self::read_capabilities_from_memory(&mut store, &instance, capabilities_ptr)?;
        
        Ok(Self {
            plugin_path,
            engine,
            store,
            instance,
            capabilities,
        })
    }
    
    fn read_capabilities_from_memory(
        store: &mut wasmtime::Store<()>,
        instance: &wasmtime::Instance,
        ptr: i32,
    ) -> Result<Vec<CapabilityDescriptor>, String> {
        // Implementation would read RTFS type definitions from WASM memory
        // This is a simplified version
        Ok(vec![
            CapabilityDescriptor {
                id: "plugin.custom.example".to_string(),
                description: "Example plugin capability".to_string(),
                capability_type: TypeExpr::Function {
                    param_types: vec![ParamType::Simple(Box::new(TypeExpr::Any))],
                    variadic_param_type: None,
                    return_type: Box::new(TypeExpr::Any),
                },
                security_requirements: SecurityRequirements {
                    permissions: vec![],
                    requires_microvm: true, // WASM is already sandboxed
                    resource_limits: ResourceLimits {
                        max_memory: Some(32 * 1024 * 1024), // 32MB
                        max_cpu_time: Some(Duration::from_secs(10)),
                        max_disk_space: None,
                    },
                    network_access: NetworkAccess::None,
                },
                metadata: HashMap::new(),
            }
        ])
    }
}

impl CapabilityProvider for WASMPluginProvider {
    fn provider_id(&self) -> &str { "wasm-plugin" }
    
    fn list_capabilities(&self) -> Vec<CapabilityDescriptor> {
        self.capabilities.clone()
    }
    
    fn execute_capability(
        &self,
        capability_id: &str,
        inputs: &Value,
        context: &ExecutionContext,
    ) -> RuntimeResult<Value> {
        // Find the execute function in the WASM module
        let execute_func = self.instance
            .get_typed_func::<(i32, i32), i32>(&mut self.store, "execute_capability")
            .map_err(|e| RuntimeError::Generic(format!("Plugin missing execute function: {}", e)))?;
        
        // Serialize inputs to JSON and write to WASM memory
        let inputs_json = serde_json::to_string(inputs)
            .map_err(|e| RuntimeError::Generic(format!("Failed to serialize inputs: {}", e)))?;
        
        // Write to WASM memory (simplified)
        let inputs_ptr = self.write_to_memory(&inputs_json)?;
        let capability_id_ptr = self.write_to_memory(capability_id)?;
        
        // Call WASM function
        let result_ptr = execute_func.call(&mut self.store, (capability_id_ptr, inputs_ptr))
            .map_err(|e| RuntimeError::Generic(format!("WASM execution failed: {}", e)))?;
        
        // Read result from WASM memory
        let result_json = self.read_from_memory(result_ptr)?;
        
        // Deserialize result
        let result: Value = serde_json::from_str(&result_json)
            .map_err(|e| RuntimeError::Generic(format!("Failed to deserialize result: {}", e)))?;
        
        Ok(result)
    }
    
    fn initialize(&mut self, config: &ProviderConfig) -> Result<(), String> {
        // Initialize plugin if it has an init function
        if let Ok(init_func) = self.instance.get_typed_func::<(), ()>(&mut self.store, "init") {
            init_func.call(&mut self.store, ())
                .map_err(|e| format!("Plugin initialization failed: {}", e))?;
        }
        
        Ok(())
    }
    
    fn health_check(&self) -> HealthStatus {
        // Check if WASM instance is still valid
        if let Ok(health_func) = self.instance.get_typed_func::<(), i32>(&mut self.store, "health_check") {
            match health_func.call(&mut self.store, ()) {
                Ok(0) => HealthStatus::Healthy,
                Ok(1) => HealthStatus::Degraded("Plugin reported degraded state".to_string()),
                _ => HealthStatus::Unhealthy("Plugin reported unhealthy state".to_string()),
            }
        } else {
            HealthStatus::Healthy // No health check function, assume healthy
        }
    }
    
    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            name: format!("WASM Plugin: {}", self.plugin_path.display()),
            version: "1.0.0".to_string(),
            description: "WASM-based plugin capability provider".to_string(),
            author: "Plugin Author".to_string(),
            license: None,
            dependencies: vec!["wasmtime".to_string()],
        }
    }
}
```

## Practical Examples of Constrained Capabilities

### Email Validation Service

```rust
/// Email validation capability with string constraints
pub fn email_validation_capability() -> CapabilityDescriptor {
    CapabilityDescriptor {
        id: "email.validate".to_string(),
        description: "Validates email address format".to_string(),
        capability_type: TypeExpr::Function {
            param_types: vec![ParamType::Simple(Box::new(
                TypeExpr::Intersection(vec![
                    TypeExpr::Primitive(PrimitiveType::String),
                    TypeExpr::Literal(Literal::Keyword(Keyword::new("string-contains @"))),
                    TypeExpr::Literal(Literal::Keyword(Keyword::new("string-min-length 5")))
                ])
            ))],
            variadic_param_type: None,
            return_type: Box::new(TypeExpr::Union(vec![
                TypeExpr::Literal(Literal::Keyword(Keyword::new("valid"))),
                TypeExpr::Literal(Literal::Keyword(Keyword::new("invalid")))
            ])),
        },
        security_requirements: SecurityRequirements {
            permissions: vec![],
            requires_microvm: false,
            resource_limits: ResourceLimits {
                max_memory: Some(1024 * 1024), // 1MB
                max_cpu_time: Some(Duration::from_millis(100)),
                max_disk_space: None,
            },
            network_access: NetworkAccess::None,
        },
        metadata: HashMap::new(),
    }
}
```

### Mathematical Operations with Range Constraints

```rust
/// Age calculation with constrained inputs and outputs
pub fn age_calculation_capability() -> CapabilityDescriptor {
    CapabilityDescriptor {
        id: "person.calculate-age".to_string(),
        description: "Calculate age from birth year (1900-2024)".to_string(),
        capability_type: TypeExpr::Function {
            param_types: vec![ParamType::Simple(Box::new(
                TypeExpr::Intersection(vec![
                    TypeExpr::Primitive(PrimitiveType::Int),
                    TypeExpr::Literal(Literal::Keyword(Keyword::new(">= 1900"))),
                    TypeExpr::Literal(Literal::Keyword(Keyword::new("<= 2024")))
                ])
            ))],
            variadic_param_type: None,
            return_type: Box::new(TypeExpr::Intersection(vec![
                TypeExpr::Primitive(PrimitiveType::Int),
                TypeExpr::Literal(Literal::Keyword(Keyword::new(">= 0"))),
                TypeExpr::Literal(Literal::Keyword(Keyword::new("<= 124")))
            ])),
        },
        security_requirements: SecurityRequirements {
            permissions: vec![],
            requires_microvm: false,
            resource_limits: ResourceLimits {
                max_memory: Some(512 * 1024), // 512KB
                max_cpu_time: Some(Duration::from_millis(50)),
                max_disk_space: None,
            },
            network_access: NetworkAccess::None,
        },
        metadata: HashMap::new(),
    }
}
```

### Complex Map Validation

```rust
/// User registration with complex map constraints
pub fn user_registration_capability() -> CapabilityDescriptor {
    CapabilityDescriptor {
        id: "user.register".to_string(),
        description: "Register new user with validation".to_string(),
        capability_type: TypeExpr::Function {
            param_types: vec![ParamType::Simple(Box::new(
                TypeExpr::Map {
                    entries: vec![
                        MapTypeEntry {
                            key: Keyword::new("name"),
                            value_type: Box::new(TypeExpr::Intersection(vec![
                                TypeExpr::Primitive(PrimitiveType::String),
                                TypeExpr::Literal(Literal::Keyword(Keyword::new("string-min-length 2"))),
                                TypeExpr::Literal(Literal::Keyword(Keyword::new("string-max-length 50")))
                            ])),
                            optional: false,
                        },
                        MapTypeEntry {
                            key: Keyword::new("email"),
                            value_type: Box::new(TypeExpr::Intersection(vec![
                                TypeExpr::Primitive(PrimitiveType::String),
                                TypeExpr::Literal(Literal::Keyword(Keyword::new("string-contains @"))),
                                TypeExpr::Literal(Literal::Keyword(Keyword::new("string-min-length 5")))
                            ])),
                            optional: false,
                        },
                        MapTypeEntry {
                            key: Keyword::new("age"),
                            value_type: Box::new(TypeExpr::Intersection(vec![
                                TypeExpr::Primitive(PrimitiveType::Int),
                                TypeExpr::Literal(Literal::Keyword(Keyword::new(">= 13"))),
                                TypeExpr::Literal(Literal::Keyword(Keyword::new("<= 120")))
                            ])),
                            optional: true,
                        },
                    ],
                    wildcard: None,
                }
            ))],
            variadic_param_type: None,
            return_type: Box::new(TypeExpr::Map {
                entries: vec![
                    MapTypeEntry {
                        key: Keyword::new("status"),
                        value_type: Box::new(TypeExpr::Union(vec![
                            TypeExpr::Literal(Literal::Keyword(Keyword::new("success"))),
                            TypeExpr::Literal(Literal::Keyword(Keyword::new("error")))
                        ])),
                        optional: false,
                    },
                    MapTypeEntry {
                        key: Keyword::new("user_id"),
                        value_type: Box::new(TypeExpr::Union(vec![
                            TypeExpr::Intersection(vec![
                                TypeExpr::Primitive(PrimitiveType::Int),
                                TypeExpr::Literal(Literal::Keyword(Keyword::new("> 0")))
                            ]),
                            TypeExpr::Primitive(PrimitiveType::Nil)
                        ])),
                        optional: true,
                    },
                    MapTypeEntry {
                        key: Keyword::new("errors"),
                        value_type: Box::new(TypeExpr::Vector(Box::new(TypeExpr::Primitive(PrimitiveType::String)))),
                        optional: true,
                    },
                ],
                wildcard: None,
            }),
        },
        security_requirements: SecurityRequirements {
            permissions: vec![Permission::FileWrite(PathBuf::from("/data/users"))],
            requires_microvm: false,
            resource_limits: ResourceLimits {
                max_memory: Some(2 * 1024 * 1024), // 2MB
                max_cpu_time: Some(Duration::from_millis(500)),
                max_disk_space: Some(1024), // 1KB for user record
            },
            network_access: NetworkAccess::None,
        },
        metadata: HashMap::new(),
    }
}
```

### Automatic Validation Usage

```rust
use crate::runtime::capability_provider::ValidatedCapabilityProvider;

// Using the validated capability provider
pub fn execute_with_validation() -> Result<(), String> {
    let provider = MyCapabilityProvider::new();
    let context = ExecutionContext::new();
    
    // Input validation happens automatically
    let inputs = vec![
        Value::String("user@example.com".to_string()),
        Value::Integer(25),
    ];
    
    // This will validate inputs, execute, and validate outputs
    let result = provider.execute_capability_validated(
        "user.register",
        &inputs,
        &context,
    )?;
    
    println!("Validated result: {:?}", result);
    Ok(())
}
```

This constraint-based approach ensures that all capability interactions are type-safe and validated according to RTFS's native type system, providing better integration, performance, and developer experience than external schema validation approaches.

## RTFS Language Capability Definitions

Instead of the verbose Rust code, capabilities can be defined directly in RTFS language, which is much more concise and expressive:

### Email Validation Capability (RTFS)

```rtfs
;; Email validation capability definition
(capability email.validate
  :description "Validates email address format"
  :type [:=> [[:and string [:string-contains "@"] [:string-min-length 5]]] 
             [:enum :valid :invalid]]
  :security {:permissions []
             :requires-microvm false
             :resource-limits {:max-memory 1048576  ;; 1MB
                              :max-cpu-time 100     ;; 100ms
                              :max-disk-space nil}
             :network-access :none}
  :metadata {}
  
  ;; Implementation
  :implementation
  (fn [email :[:and string [:string-contains "@"] [:string-min-length 5]]]
    :[:enum :valid :invalid]
    (if (and (string-contains? email "@")
             (>= (string-length email) 5)
             (re-matches? #"^[^@]+@[^@]+\.[^@]+$" email))
      :valid
      :invalid)))
```

### Age Calculation Capability (RTFS)

```rtfs
;; Age calculation with range constraints
(capability person.calculate-age
  :description "Calculate age from birth year (1900-2024)"
  :type [:=> [[:and int [:>= 1900] [:<= 2024]]]
             [:and int [:>= 0] [:<= 124]]]
  :security {:permissions []
             :requires-microvm false
             :resource-limits {:max-memory 524288   ;; 512KB
                              :max-cpu-time 50      ;; 50ms
                              :max-disk-space nil}
             :network-access :none}
  :metadata {}
  
  ;; Implementation
  :implementation
  (fn [birth-year :[:and int [:>= 1900] [:<= 2024]]]
    :[:and int [:>= 0] [:<= 124]]
    (let [current-year 2024]
      (- current-year birth-year))))
```

### Mathematical Operations (RTFS)

```rtfs
;; Positive integer multiplication
(capability math.multiply-positive
  :description "Multiplies two positive integers"
  :type [:=> [[:and int [:> 0]] [:and int [:> 0]]]
             [:and int [:> 0]]]
  :security {:permissions []
             :requires-microvm false
             :resource-limits {:max-memory 524288   ;; 512KB
                              :max-cpu-time 50      ;; 50ms
                              :max-disk-space nil}
             :network-access :none}
  :metadata {}
  
  ;; Implementation
  :implementation
  (fn [a :[:and int [:> 0]] b :[:and int [:> 0]]]
    :[:and int [:> 0]]
    (* a b)))
```

### Complex Map Validation (RTFS)

```rtfs
;; User registration with complex validation
(capability user.register
  :description "Register new user with validation"
  :type [:=> [[:map 
               [:name [:and string [:string-min-length 2] [:string-max-length 50]]]
               [:email [:and string [:string-contains "@"] [:string-min-length 5]]]
               [:age [:and int [:>= 13] [:<= 120]]?]]]
             [:map 
               [:status [:enum :success :error]]
               [:user-id [:union [:and int [:> 0]] nil]?]
               [:errors [:vector string]?]]]
  :security {:permissions [[:file-write "/data/users"]]
             :requires-microvm false
             :resource-limits {:max-memory 2097152  ;; 2MB
                              :max-cpu-time 500     ;; 500ms
                              :max-disk-space 1024} ;; 1KB
             :network-access :none}
  :metadata {}
  
  ;; Implementation
  :implementation
  (fn [user-data :[:map 
                   [:name [:and string [:string-min-length 2] [:string-max-length 50]]]
                   [:email [:and string [:string-contains "@"] [:string-min-length 5]]]
                   [:age [:and int [:>= 13] [:<= 120]]?]]]
    :[:map 
      [:status [:enum :success :error]]
      [:user-id [:union [:and int [:> 0]] nil]?]
      [:errors [:vector string]?]]
    
    (let [errors (validate-user-data user-data)]
      (if (empty? errors)
        ;; Success case
        (let [user-id (save-user user-data)]
          {:status :success
           :user-id user-id})
        ;; Error case
        {:status :error
         :errors errors}))))

;; Helper function for validation
(defn validate-user-data [user-data]
  (let [errors []]
    ;; Name validation
    (when (or (< (string-length (:name user-data)) 2)
              (> (string-length (:name user-data)) 50))
      (conj errors "Name must be between 2 and 50 characters"))
    
    ;; Email validation
    (when (not (and (string-contains? (:email user-data) "@")
                   (>= (string-length (:email user-data)) 5)))
      (conj errors "Email must contain @ and be at least 5 characters"))
    
    ;; Age validation (if provided)
    (when (and (:age user-data)
               (or (< (:age user-data) 13)
                   (> (:age user-data) 120)))
      (conj errors "Age must be between 13 and 120"))
    
    errors))
```

### System Capabilities (RTFS)

```rtfs
;; System environment variable access
(capability ccos.system.get-env
  :description "Get environment variable"
  :type [:=> [[:and string [:string-min-length 1]]]
             [:union string nil]]
  :security {:permissions [[:environment-read "*"]]
             :requires-microvm false
             :resource-limits {:max-memory 1048576  ;; 1MB
                              :max-cpu-time 100     ;; 100ms
                              :max-disk-space nil}
             :network-access :none}
  :metadata {}
  
  ;; Implementation uses built-in system function
  :implementation
  (fn [var-name :[:and string [:string-min-length 1]]]
    :[:union string nil]
    (system:get-env var-name)))

;; System logging capability
(capability ccos.io.log
  :description "Log a message"
  :type [:=> [any] nil]
  :security {:permissions []
             :requires-microvm false
             :resource-limits {:max-memory 1048576  ;; 1MB
                              :max-cpu-time 50      ;; 50ms
                              :max-disk-space nil}
             :network-access :none}
  :metadata {}
  
  ;; Implementation
  :implementation
  (fn [message :any] :nil
    (system:log message)))
```

### Variadic Capabilities (RTFS)

```rtfs
;; Sum of positive numbers with variadic parameters
(capability math.sum-positive
  :description "Sum multiple positive numbers"
  :type [:=> [] [:* [:and int [:> 0]]] [:and int [:> 0]]]
  :security {:permissions []
             :requires-microvm false
             :resource-limits {:max-memory 1048576  ;; 1MB
                              :max-cpu-time 100     ;; 100ms
                              :max-disk-space nil}
             :network-access :none}
  :metadata {}
  
  ;; Implementation with variadic parameters
  :implementation
  (fn [& numbers :[:* [:and int [:> 0]]]]
    :[:and int [:> 0]]
    (reduce + 0 numbers)))
```

### Capability Provider Definition (RTFS)

```rtfs
;; Define a capability provider in RTFS
(provider math-provider
  :description "Mathematical operations provider"
  :version "1.0.0"
  :author "CCOS Team"
  :license "MIT"
  :dependencies []
  
  ;; List of capabilities this provider offers
  :capabilities [
    math.multiply-positive
    math.sum-positive
    person.calculate-age
  ]
  
  ;; Provider initialization
  :initialize
  (fn [config]
    ;; Initialize provider with configuration
    (log-step :info "Math provider initialized"))
  
  ;; Health check
  :health-check
  (fn []
    :healthy)
  
  ;; Metadata
  :metadata
  {:name "Math Provider"
   :description "Provides mathematical operations"
   :version "1.0.0"})
```

### Capability Registry Usage (RTFS)

```rtfs
;; Register and use capabilities
(let [registry (capability-registry:new)]
  ;; Register providers
  (capability-registry:register-provider registry math-provider)
  (capability-registry:register-provider registry email-provider)
  
  ;; Execute capabilities with automatic validation
  (let [result (capability-registry:execute registry 
                                           "math.multiply-positive" 
                                           [5 3])]
    (log-step :info "Result:" result))
  
  ;; Execute with validation
  (try
    (let [email-result (capability-registry:execute registry
                                                   "email.validate"
                                                   ["invalid-email"])]
      (log-step :info "Email validation:" email-result))
    (catch :validation-error e
      (log-step :error "Validation failed:" (:message e)))))
```

### Benefits of RTFS Capability Definitions

1. **Conciseness**: Much shorter than Rust equivalents
2. **Type Safety**: Full RTFS type system integration
3. **Expressiveness**: Natural constraint expression
4. **Composability**: Easy to combine and reuse
5. **Readability**: Clear intent and structure
6. **Native Integration**: No marshalling between languages
7. **Dynamic**: Can be loaded and modified at runtime
8. **Validation**: Automatic type checking and constraint validation

### Migration from Rust to RTFS

Existing Rust capability providers can be gradually migrated to RTFS:

```rtfs
;; Wrapper capability that delegates to Rust implementation
(capability legacy.rust-capability
  :description "Legacy Rust capability wrapper"
  :type [:=> [string] string]
  :security {:permissions []
             :requires-microvm false
             :resource-limits {:max-memory 1048576
                              :max-cpu-time 100
                              :max-disk-space nil}
             :network-access :none}
  :metadata {:legacy true}
  
  ;; Delegate to Rust implementation
  :implementation
  (fn [input :string] :string
    (rust:call-legacy-capability "legacy.rust-capability" input)))
```

This approach allows for a smooth transition from Rust-based capabilities to pure RTFS implementations while maintaining compatibility and type safety throughout the migration process.
