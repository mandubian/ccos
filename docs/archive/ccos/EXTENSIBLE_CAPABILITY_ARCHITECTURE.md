# CCOS Extensible Capability Architecture

## Overview

The CCOS Extensible Capability Architecture allows dynamic registration of capabilities without modifying the RTFS compiler. This enables:

- **Plugin System**: Load capabilities from external modules
- **MCP Integration**: Connect to Model Context Protocol servers
- **A2A Integration**: Connect to Agent-to-Agent communication servers
- **Dynamic Discovery**: Automatically discover and register capabilities
- **Hot Reloading**: Add/remove capabilities at runtime

## Architecture Components

### 1. Capability Provider Interface

```rust
/// Core trait that all capability providers must implement
pub trait CapabilityProvider: Send + Sync {
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
}
```

### 2. Capability Registry (Enhanced)

```rust
/// Enhanced registry supporting dynamic providers
pub struct CapabilityRegistry {
    providers: HashMap<String, Arc<dyn CapabilityProvider>>,
    capability_index: HashMap<String, String>, // capability_id -> provider_id
    discovery_agents: Vec<Box<dyn CapabilityDiscovery>>,
    execution_context: ExecutionContext,
}

impl CapabilityRegistry {
    /// Register a capability provider
    pub fn register_provider(&mut self, provider: Arc<dyn CapabilityProvider>) -> Result<(), String>;
    
    /// Discover and register capabilities from external sources
    pub fn discover_capabilities(&mut self) -> Result<usize, String>;
    
    /// Execute capability with automatic provider routing
    pub fn execute_capability(
        &self,
        capability_id: &str,
        inputs: &Value,
        options: Option<&Value>,
    ) -> RuntimeResult<Value>;
    
    /// List all available capabilities across all providers
    pub fn list_all_capabilities(&self) -> Vec<CapabilityDescriptor>;
}
```

### 3. Built-in Capability Providers

#### System Provider
```rust
/// Built-in system capabilities
pub struct SystemCapabilityProvider {
    security_context: SecurityContext,
}

impl CapabilityProvider for SystemCapabilityProvider {
    fn provider_id(&self) -> &str { "ccos.system" }
    
    fn list_capabilities(&self) -> Vec<CapabilityDescriptor> {
        vec![
            CapabilityDescriptor::new("ccos.system.get-env", "Get environment variable"),
            CapabilityDescriptor::new("ccos.system.current-time", "Get current time"),
            CapabilityDescriptor::new("ccos.io.log", "Log message"),
            CapabilityDescriptor::new("ccos.io.file-exists", "Check file existence"),
            // ... other system capabilities
        ]
    }
    
    fn execute_capability(&self, capability_id: &str, inputs: &Value, context: &ExecutionContext) -> RuntimeResult<Value> {
        // Route to appropriate system function
        match capability_id {
            "ccos.system.get-env" => self.get_env(inputs, context),
            "ccos.system.current-time" => self.current_time(inputs, context),
            "ccos.io.log" => self.log(inputs, context),
            "ccos.io.file-exists" => self.file_exists(inputs, context),
            _ => Err(RuntimeError::Generic(format!("Unknown system capability: {}", capability_id)))
        }
    }
}
```

#### MCP Provider
```rust
/// Model Context Protocol server integration
pub struct MCPCapabilityProvider {
    server_url: String,
    client: MCPClient,
    capabilities_cache: Arc<RwLock<HashMap<String, CapabilityDescriptor>>>,
}

impl CapabilityProvider for MCPCapabilityProvider {
    fn provider_id(&self) -> &str { "mcp" }
    
    fn list_capabilities(&self) -> Vec<CapabilityDescriptor> {
        // Query MCP server for available tools
        self.client.list_tools().map(|tools| {
            tools.into_iter().map(|tool| {
                CapabilityDescriptor::new(
                    &format!("mcp.{}", tool.name),
                    &tool.description,
                ).with_schema(tool.input_schema)
            }).collect()
        }).unwrap_or_default()
    }
    
    fn execute_capability(&self, capability_id: &str, inputs: &Value, context: &ExecutionContext) -> RuntimeResult<Value> {
        // Extract tool name from capability_id
        let tool_name = capability_id.strip_prefix("mcp.").unwrap_or(capability_id);
        
        // Convert RTFS Value to MCP arguments
        let mcp_args = self.rtfs_to_mcp_args(inputs)?;
        
        // Call MCP server
        let result = self.client.call_tool(tool_name, mcp_args)
            .map_err(|e| RuntimeError::Generic(format!("MCP call failed: {}", e)))?;
        
        // Convert MCP response back to RTFS Value
        self.mcp_to_rtfs_value(result)
    }
}
```

#### A2A Provider
```rust
/// Agent-to-Agent communication provider
pub struct A2ACapabilityProvider {
    agent_registry: Arc<AgentRegistry>,
    communication_client: A2AClient,
}

impl CapabilityProvider for A2ACapabilityProvider {
    fn provider_id(&self) -> &str { "a2a" }
    
    fn list_capabilities(&self) -> Vec<CapabilityDescriptor> {
        // Query agent registry for available agent capabilities
        self.agent_registry.list_agents().into_iter().flat_map(|agent| {
            agent.capabilities.into_iter().map(|cap| {
                CapabilityDescriptor::new(
                    &format!("a2a.{}.{}", agent.id, cap.name),
                    &cap.description,
                ).with_agent_metadata(agent.id.clone(), agent.endpoint.clone())
            })
        }).collect()
    }
    
    fn execute_capability(&self, capability_id: &str, inputs: &Value, context: &ExecutionContext) -> RuntimeResult<Value> {
        // Parse capability_id: "a2a.{agent_id}.{capability_name}"
        let parts: Vec<&str> = capability_id.split('.').collect();
        if parts.len() != 3 || parts[0] != "a2a" {
            return Err(RuntimeError::Generic("Invalid A2A capability ID".to_string()));
        }
        
        let agent_id = parts[1];
        let capability_name = parts[2];
        
        // Find agent and send capability request
        let agent = self.agent_registry.get_agent(agent_id)
            .ok_or_else(|| RuntimeError::Generic(format!("Agent not found: {}", agent_id)))?;
        
        let request = A2ARequest {
            capability: capability_name.to_string(),
            inputs: inputs.clone(),
            context: context.clone(),
        };
        
        let response = self.communication_client.send_request(&agent.endpoint, request)
            .map_err(|e| RuntimeError::Generic(format!("A2A communication failed: {}", e)))?;
        
        Ok(response.result)
    }
}
```

#### Plugin Provider
```rust
/// Dynamic plugin system for external capabilities
pub struct PluginCapabilityProvider {
    plugin_dir: PathBuf,
    loaded_plugins: HashMap<String, Arc<dyn CapabilityProvider>>,
    plugin_loader: PluginLoader,
}

impl CapabilityProvider for PluginCapabilityProvider {
    fn provider_id(&self) -> &str { "plugins" }
    
    fn list_capabilities(&self) -> Vec<CapabilityDescriptor> {
        self.loaded_plugins.values().flat_map(|plugin| {
            plugin.list_capabilities()
        }).collect()
    }
    
    fn execute_capability(&self, capability_id: &str, inputs: &Value, context: &ExecutionContext) -> RuntimeResult<Value> {
        // Find which plugin provides this capability
        for plugin in self.loaded_plugins.values() {
            if plugin.list_capabilities().iter().any(|cap| cap.id == capability_id) {
                return plugin.execute_capability(capability_id, inputs, context);
            }
        }
        
        Err(RuntimeError::Generic(format!("Plugin capability not found: {}", capability_id)))
    }
}

impl PluginCapabilityProvider {
    pub fn load_plugins(&mut self) -> Result<usize, String> {
        let mut loaded_count = 0;
        
        for entry in fs::read_dir(&self.plugin_dir)? {
            let entry = entry?;
            let path = entry.path();
            
            if path.extension() == Some(OsStr::new("wasm")) {
                // Load WASM plugin
                let plugin = self.plugin_loader.load_wasm_plugin(&path)?;
                self.loaded_plugins.insert(plugin.provider_id().to_string(), plugin);
                loaded_count += 1;
            } else if path.extension() == Some(OsStr::new("so")) {
                // Load native plugin
                let plugin = self.plugin_loader.load_native_plugin(&path)?;
                self.loaded_plugins.insert(plugin.provider_id().to_string(), plugin);
                loaded_count += 1;
            }
        }
        
        Ok(loaded_count)
    }
}
```

### 4. Capability Discovery

```rust
/// Trait for discovering capabilities from various sources
pub trait CapabilityDiscovery: Send + Sync {
    fn discover(&self) -> Result<Vec<CapabilityDescriptor>, String>;
}

/// Discover MCP servers from configuration
pub struct MCPDiscovery {
    config: MCPConfig,
}

impl CapabilityDiscovery for MCPDiscovery {
    fn discover(&self) -> Result<Vec<CapabilityDescriptor>, String> {
        let mut capabilities = Vec::new();
        
        for server_config in &self.config.servers {
            match MCPClient::connect(&server_config.url) {
                Ok(client) => {
                    if let Ok(tools) = client.list_tools() {
                        for tool in tools {
                            capabilities.push(CapabilityDescriptor::new(
                                &format!("mcp.{}.{}", server_config.name, tool.name),
                                &tool.description,
                            ).with_server_info(server_config.clone()));
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to connect to MCP server {}: {}", server_config.name, e);
                }
            }
        }
        
        Ok(capabilities)
    }
}

/// Discover A2A agents from registry
pub struct A2ADiscovery {
    registry_client: AgentRegistryClient,
}

impl CapabilityDiscovery for A2ADiscovery {
    fn discover(&self) -> Result<Vec<CapabilityDescriptor>, String> {
        let agents = self.registry_client.list_agents()?;
        
        Ok(agents.into_iter().flat_map(|agent| {
            agent.capabilities.into_iter().map(|cap| {
                CapabilityDescriptor::new(
                    &format!("a2a.{}.{}", agent.id, cap.name),
                    &cap.description,
                ).with_agent_metadata(agent.id.clone(), agent.endpoint.clone())
            })
        }).collect())
    }
}
```

### 5. Configuration System

```rust
/// Configuration for capability providers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityConfig {
    pub system: SystemConfig,
    pub mcp: MCPConfig,
    pub a2a: A2AConfig,
    pub plugins: PluginConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPConfig {
    pub servers: Vec<MCPServerConfig>,
    pub timeout: Duration,
    pub retry_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPServerConfig {
    pub name: String,
    pub url: String,
    pub auth: Option<AuthConfig>,
    pub capabilities: Vec<String>, // Filter specific capabilities
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2AConfig {
    pub registry_url: String,
    pub discovery_interval: Duration,
    pub trust_policy: TrustPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfig {
    pub plugin_dir: PathBuf,
    pub auto_reload: bool,
    pub security_policy: PluginSecurityPolicy,
}
```

### 6. Enhanced Call Function

```rust
/// Enhanced call function with extensible capability routing
fn call_capability(
    args: Vec<Value>,
    evaluator: &Evaluator,
    env: &mut Environment,
) -> RuntimeResult<Value> {
    let args = args.as_slice();
    
    if args.len() < 2 || args.len() > 3 {
        return Err(RuntimeError::ArityMismatch {
            function: "call".to_string(),
            expected: "2 or 3".to_string(),
            actual: args.len(),
        });
    }

    // Extract capability-id (must be a keyword)
    let capability_id = match &args[0] {
        Value::Keyword(k) => k.0.clone(),
        _ => return Err(RuntimeError::TypeError {
            expected: "keyword".to_string(),
            actual: args[0].type_name().to_string(),
            operation: "call capability-id".to_string(),
        }),
    };

    // Extract inputs and options
    let inputs = args[1].clone();
    let options = if args.len() == 3 { Some(&args[2]) } else { None };

    // Get capability registry from evaluator context
    let registry = evaluator.get_capability_registry()
        .ok_or_else(|| RuntimeError::Generic("No capability registry available".to_string()))?;

    // Route through registry - this handles all provider types
    registry.execute_capability(&capability_id, &inputs, options)
}
```

## Benefits of This Architecture

### 1. **Zero-Touch Extensibility**
- Add new capabilities without modifying RTFS compiler
- Plugin system supports WASM and native plugins
- Dynamic discovery and registration

### 2. **Protocol Integration**
- MCP servers automatically become RTFS capabilities
- A2A agents seamlessly integrated
- Standard protocols for maximum compatibility

### 3. **Security & Isolation**
- Each provider has its own security context
- Plugins can run in WASM sandbox
- Fine-grained permission control

### 4. **Performance & Reliability**
- Lazy loading of capabilities
- Health checks and failover
- Caching and optimization

### 5. **Developer Experience**
- Simple plugin API
- Automatic capability discovery
- Rich metadata and documentation

## Usage Examples

### Basic System Capability
```rtfs
(call :ccos.io.log "Hello from RTFS!")
```

### MCP Server Tool
```rtfs
(call :mcp.weather-server.get-weather {:location "San Francisco"})
```

### A2A Agent Capability
```rtfs
(call :a2a.data-analyst.analyze-csv {:file "sales.csv" :type "quarterly"})
```

### Plugin Capability
```rtfs
(call :plugins.custom-ml.predict {:model "customer-churn" :features {...}})
```

## Migration Strategy

1. **Phase 1**: Implement core provider interfaces
2. **Phase 2**: Convert existing tool functions to SystemCapabilityProvider
3. **Phase 3**: Add MCP integration
4. **Phase 4**: Add A2A integration
5. **Phase 5**: Implement plugin system
6. **Phase 6**: Add discovery and hot-reloading
