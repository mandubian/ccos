# CCOS Runtime Integration Guide

## Overview

This guide details how to integrate the CCOS Capability Architecture with the RTFS runtime, ensuring seamless execution while maintaining security boundaries.

## Analysis

### Current State Assessment

The RTFS runtime currently has a mixed standard library with both pure and dangerous functions in the same namespace. The goal is to:

1. **Separate concerns**: Pure functions stay in RTFS, dangerous operations move to CCOS
2. **Maintain compatibility**: Existing RTFS code should continue working
3. **Add extensibility**: New capabilities can be added without compiler changes
4. **Enforce security**: Clear boundaries between safe and unsafe operations

### Motivation

- **Security**: Prevent accidental execution of dangerous operations in pure contexts
- **Extensibility**: Enable plugin ecosystem without core modifications
- **Clarity**: Clear separation between language features and system capabilities
- **Performance**: Pure functions can be optimized more aggressively

## Implementation Steps

### Phase 1: Core Runtime Integration

#### 1.1 Update Runtime Initialization

```rust
// src/runtime/mod.rs
use crate::runtime::{
    secure_stdlib::SecureStandardLibrary,
    capability_registry::CapabilityRegistry,
    security::SecurityContext,
};

pub struct RTFSRuntime {
    /// Secure standard library (pure functions only)
    secure_stdlib: SecureStandardLibrary,
    /// CCOS capability registry
    capability_registry: Option<CapabilityRegistry>,
    /// Current security context
    security_context: SecurityContext,
    /// Execution environment
    environment: Environment,
}

impl RTFSRuntime {
    /// Create a new RTFS runtime with pure functions only
    pub fn new_pure() -> Self {
        Self {
            secure_stdlib: SecureStandardLibrary::new(),
            capability_registry: None,
            security_context: SecurityContext::pure(),
            environment: Environment::default(),
        }
    }
    
    /// Create a new RTFS runtime with CCOS integration
    pub fn new_with_ccos(capability_registry: CapabilityRegistry) -> Self {
        Self {
            secure_stdlib: SecureStandardLibrary::new(),
            capability_registry: Some(capability_registry),
            security_context: SecurityContext::controlled(),
            environment: Environment::default(),
        }
    }
    
    /// Create a new RTFS runtime with full system access
    pub fn new_full() -> Self {
        let mut registry = CapabilityRegistry::new();
        
        // Register built-in providers
        registry.register_system_provider().expect("Failed to register system provider");
        
        Self {
            secure_stdlib: SecureStandardLibrary::new(),
            capability_registry: Some(registry),
            security_context: SecurityContext::full(),
            environment: Environment::default(),
        }
    }
}
```

#### 1.2 Update Function Resolution

```rust
// src/runtime/environment.rs
impl Environment {
    pub fn resolve_function(&self, name: &str) -> Option<Value> {
        // First, check secure standard library
        if let Some(func) = self.runtime.secure_stdlib.get_function(name) {
            return Some(func);
        }
        
        // If CCOS is available, check for capability call
        if name == "call" {
            if self.runtime.capability_registry.is_some() {
                return Some(Value::NativeFunction(NativeFunction::CapabilityCall));
            } else {
                // In pure mode, capability calls are not allowed
                return None;
            }
        }
        
        // Check user-defined functions
        if let Some(func) = self.functions.get(name) {
            return Some(func.clone());
        }
        
        None
    }
}
```

#### 1.3 Enhanced Function Call Handler

```rust
// src/runtime/interpreter.rs
impl Interpreter {
    pub fn call_function(
        &mut self,
        function: &Value,
        args: &[Value],
        env: &mut Environment,
    ) -> RuntimeResult<Value> {
        match function {
            Value::NativeFunction(NativeFunction::CapabilityCall) => {
                self.handle_capability_call(args, env)
            }
            Value::NativeFunction(native_func) => {
                // Handle secure standard library functions
                self.call_secure_native_function(native_func, args, env)
            }
            Value::UserFunction(user_func) => {
                self.call_user_function(user_func, args, env)
            }
            _ => Err(RuntimeError::TypeError {
                expected: "function".to_string(),
                actual: function.type_name().to_string(),
                operation: "function call".to_string(),
            }),
        }
    }
    
    fn handle_capability_call(
        &mut self,
        args: &[Value],
        env: &mut Environment,
    ) -> RuntimeResult<Value> {
        // Validate arguments: (call :capability-id inputs options?)
        if args.len() < 2 {
            return Err(RuntimeError::ArityError {
                expected: "2 or 3".to_string(),
                actual: args.len(),
                function: "call".to_string(),
            });
        }
        
        // Extract capability ID
        let capability_id = match &args[0] {
            Value::Keyword(keyword) => keyword.0.clone(),
            Value::String(s) => s.clone(),
            _ => return Err(RuntimeError::TypeError {
                expected: "keyword or string".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "capability call".to_string(),
            }),
        };
        
        // Get inputs
        let inputs = &args[1];
        
        // Get options (if provided)
        let options = args.get(2);
        
        // Check if CCOS is available
        let registry = env.runtime.capability_registry.as_ref()
            .ok_or_else(|| RuntimeError::Generic(
                "Capability calls are not available in pure mode. Use RTFS with CCOS integration.".to_string()
            ))?;
        
        // Execute capability
        registry.execute_capability(&capability_id, inputs, options)
    }
    
    fn call_secure_native_function(
        &mut self,
        function: &NativeFunction,
        args: &[Value],
        env: &Environment,
    ) -> RuntimeResult<Value> {
        // All secure functions are guaranteed to be pure
        env.runtime.secure_stdlib.call_function(function, args)
    }
}
```

### Phase 2: Configuration System

#### 2.1 Runtime Configuration

```rust
// src/runtime/config.rs
use serde::{Deserialize, Serialize};

/// Configuration for RTFS runtime
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    /// Security mode
    pub security_mode: SecurityMode,
    /// Capability providers to enable
    pub capability_providers: Vec<ProviderConfig>,
    /// MCP servers to connect to
    pub mcp_servers: Vec<MCPServerConfig>,
    /// Plugin directories to scan
    pub plugin_directories: Vec<PathBuf>,
    /// Security policies
    pub security_policies: SecurityPolicies,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SecurityMode {
    /// Pure RTFS only, no dangerous operations
    Pure,
    /// Controlled access through CCOS with explicit permissions
    Controlled {
        /// Allowed capability patterns
        allowed_capabilities: Vec<String>,
        /// Resource limits
        resource_limits: ResourceLimits,
    },
    /// Full access to all capabilities
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub provider_type: ProviderType,
    pub config: serde_json::Value,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProviderType {
    System,
    MCP,
    Plugin { path: PathBuf },
    A2A { endpoint: String },
}

impl RuntimeConfig {
    /// Load configuration from file
    pub fn load_from_file(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read config file: {}", e))?;
        
        let config: RuntimeConfig = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse config file: {}", e))?;
        
        Ok(config)
    }
    
    /// Create runtime from this configuration
    pub fn create_runtime(&self) -> Result<RTFSRuntime, String> {
        match &self.security_mode {
            SecurityMode::Pure => Ok(RTFSRuntime::new_pure()),
            SecurityMode::Controlled { allowed_capabilities, resource_limits } => {
                let mut registry = CapabilityRegistry::new();
                
                // Configure security policies
                let security_context = SecurityContext::controlled_with_policies(
                    &self.security_policies,
                    allowed_capabilities,
                    resource_limits,
                );
                
                // Register enabled providers
                for provider_config in &self.capability_providers {
                    if provider_config.enabled {
                        let provider = self.create_provider(provider_config)?;
                        registry.register_provider(provider)?;
                    }
                }
                
                Ok(RTFSRuntime::new_with_ccos(registry))
            }
            SecurityMode::Full => {
                let mut runtime = RTFSRuntime::new_full();
                
                // Register all configured providers
                for provider_config in &self.capability_providers {
                    if provider_config.enabled {
                        let provider = self.create_provider(provider_config)?;
                        if let Some(registry) = runtime.capability_registry.as_mut() {
                            registry.register_provider(provider)?;
                        }
                    }
                }
                
                Ok(runtime)
            }
        }
    }
    
    fn create_provider(&self, config: &ProviderConfig) -> Result<Arc<dyn CapabilityProvider>, String> {
        match &config.provider_type {
            ProviderType::System => {
                let system_config: SystemConfig = serde_json::from_value(config.config.clone())
                    .map_err(|e| format!("Invalid system provider config: {}", e))?;
                Ok(Arc::new(SystemCapabilityProvider::new(system_config)))
            }
            ProviderType::MCP => {
                let mcp_config: MCPServerConfig = serde_json::from_value(config.config.clone())
                    .map_err(|e| format!("Invalid MCP provider config: {}", e))?;
                let provider = MCPCapabilityProvider::new(mcp_config)?;
                Ok(Arc::new(provider))
            }
            ProviderType::Plugin { path } => {
                let provider = WASMPluginProvider::load(path.clone())?;
                Ok(Arc::new(provider))
            }
            ProviderType::A2A { endpoint } => {
                let a2a_config: A2AConfig = serde_json::from_value(config.config.clone())
                    .map_err(|e| format!("Invalid A2A provider config: {}", e))?;
                let provider = A2ACapabilityProvider::new(endpoint.clone(), a2a_config)?;
                Ok(Arc::new(provider))
            }
        }
    }
}
```

#### 2.2 Example Configuration Files

```json
// config/pure.json - Pure RTFS mode
{
  "security_mode": "Pure",
  "capability_providers": [],
  "mcp_servers": [],
  "plugin_directories": [],
  "security_policies": {
    "default_timeout": 30,
    "max_memory": 134217728,
    "allow_network": false
  }
}

// config/development.json - Development mode with common capabilities
{
  "security_mode": {
    "Controlled": {
      "allowed_capabilities": [
        "ccos.io.*",
        "ccos.system.get-env",
        "mcp.weather.*"
      ],
      "resource_limits": {
        "max_memory": 268435456,
        "max_cpu_time": 30,
        "max_disk_space": null
      }
    }
  },
  "capability_providers": [
    {
      "name": "system",
      "provider_type": "System",
      "config": {
        "log_level": "info",
        "allowed_env_vars": ["PATH", "HOME", "USER"]
      },
      "enabled": true
    }
  ],
  "mcp_servers": [
    {
      "name": "weather-server",
      "url": "http://localhost:8080/mcp",
      "auth": null,
      "requires_microvm": false
    }
  ],
  "plugin_directories": ["./plugins"],
  "security_policies": {
    "default_timeout": 30,
    "max_memory": 268435456,
    "allow_network": true,
    "network_whitelist": ["api.weather.gov", "localhost"]
  }
}

// config/production.json - Production mode with strict security
{
  "security_mode": {
    "Controlled": {
      "allowed_capabilities": [
        "ccos.io.log",
        "ccos.system.get-env"
      ],
      "resource_limits": {
        "max_memory": 67108864,
        "max_cpu_time": 10,
        "max_disk_space": 1048576
      }
    }
  },
  "capability_providers": [
    {
      "name": "system",
      "provider_type": "System",
      "config": {
        "log_level": "warn",
        "allowed_env_vars": ["NODE_ENV"]
      },
      "enabled": true
    }
  ],
  "mcp_servers": [],
  "plugin_directories": [],
  "security_policies": {
    "default_timeout": 5,
    "max_memory": 67108864,
    "allow_network": false,
    "require_explicit_permissions": true
  }
}
```

### Phase 3: Command Line Integration

#### 3.1 Enhanced CLI

```rust
// src/main.rs
use clap::{Arg, Command};
use rtfs_compiler::runtime::{RuntimeConfig, RTFSRuntime};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let matches = Command::new("rtfs")
        .about("RTFS Compiler and Runtime")
        .arg(Arg::new("file")
            .help("RTFS source file to execute")
            .required(true)
            .index(1))
        .arg(Arg::new("config")
            .long("config")
            .short('c')
            .help("Runtime configuration file")
            .value_name("CONFIG_FILE"))
        .arg(Arg::new("mode")
            .long("mode")
            .short('m')
            .help("Security mode: pure, controlled, or full")
            .value_name("MODE")
            .possible_values(["pure", "controlled", "full"]))
        .arg(Arg::new("allow-capability")
            .long("allow-capability")
            .help("Allow specific capability (can be used multiple times)")
            .value_name("CAPABILITY")
            .action(clap::ArgAction::Append))
        .arg(Arg::new("enable-mcp")
            .long("enable-mcp")
            .help("Enable MCP server at URL")
            .value_name("MCP_URL")
            .action(clap::ArgAction::Append))
        .arg(Arg::new("plugin-dir")
            .long("plugin-dir")
            .help("Directory to scan for plugins")
            .value_name("PLUGIN_DIR")
            .action(clap::ArgAction::Append))
        .get_matches();

    let source_file = matches.get_one::<String>("file").unwrap();
    
    // Load runtime configuration
    let runtime = if let Some(config_file) = matches.get_one::<String>("config") {
        // Load from configuration file
        let config = RuntimeConfig::load_from_file(Path::new(config_file))?;
        config.create_runtime()?
    } else {
        // Create runtime from command line arguments
        create_runtime_from_args(&matches)?
    };
    
    // Compile and execute
    let source = std::fs::read_to_string(source_file)?;
    let ast = rtfs_compiler::parse(&source)?;
    let result = runtime.execute(&ast).await?;
    
    println!("{:?}", result);
    
    Ok(())
}

fn create_runtime_from_args(matches: &clap::ArgMatches) -> Result<RTFSRuntime, String> {
    let mode = matches.get_one::<String>("mode").map(|s| s.as_str()).unwrap_or("pure");
    
    match mode {
        "pure" => Ok(RTFSRuntime::new_pure()),
        "controlled" => {
            let mut registry = CapabilityRegistry::new();
            
            // Register system provider
            registry.register_system_provider()?;
            
            // Register MCP servers
            if let Some(mcp_urls) = matches.get_many::<String>("enable-mcp") {
                for url in mcp_urls {
                    let mcp_config = MCPServerConfig {
                        name: format!("cli-mcp-{}", url.split('/').last().unwrap_or("server")),
                        url: url.clone(),
                        auth: None,
                        requires_microvm: false,
                    };
                    let provider = MCPCapabilityProvider::new(mcp_config)?;
                    registry.register_provider(Arc::new(provider))?;
                }
            }
            
            // Scan plugin directories
            if let Some(plugin_dirs) = matches.get_many::<String>("plugin-dir") {
                for dir_str in plugin_dirs {
                    let dir = Path::new(dir_str);
                    if dir.exists() {
                        registry.scan_plugin_directory(dir)?;
                    }
                }
            }
            
            Ok(RTFSRuntime::new_with_ccos(registry))
        }
        "full" => {
            let mut runtime = RTFSRuntime::new_full();
            
            // Register MCP servers and plugins as in controlled mode
            // ...
            
            Ok(runtime)
        }
        _ => Err(format!("Invalid mode: {}", mode)),
    }
}
```

#### 3.2 Usage Examples

```bash
# Pure RTFS mode - only pure functions available
rtfs --mode pure my_script.rtfs

# Controlled mode with specific capabilities
rtfs --mode controlled \
     --allow-capability "ccos.io.log" \
     --allow-capability "ccos.system.get-env" \
     my_script.rtfs

# Enable MCP weather server
rtfs --mode controlled \
     --enable-mcp "http://localhost:8080/weather-mcp" \
     my_script.rtfs

# Load plugins from directory
rtfs --mode controlled \
     --plugin-dir "./plugins" \
     my_script.rtfs

# Use configuration file
rtfs --config config/development.json my_script.rtfs

# Full mode with all capabilities
rtfs --mode full my_script.rtfs
```

### Phase 4: REPL Integration

#### 4.1 Enhanced REPL

```rust
// src/repl.rs
pub struct REPL {
    runtime: RTFSRuntime,
    environment: Environment,
    history: Vec<String>,
}

impl REPL {
    pub fn new(config: Option<RuntimeConfig>) -> Result<Self, String> {
        let runtime = if let Some(config) = config {
            config.create_runtime()?
        } else {
            RTFSRuntime::new_controlled_default()
        };
        
        let environment = Environment::new(&runtime);
        
        Ok(Self {
            runtime,
            environment,
            history: Vec::new(),
        })
    }
    
    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("RTFS REPL v1.0.0");
        println!("Security Mode: {:?}", self.runtime.security_context.mode());
        
        if let Some(registry) = &self.runtime.capability_registry {
            let capabilities = registry.list_all_capabilities();
            println!("Available Capabilities: {}", capabilities.len());
            
            // Show capability categories
            let mut categories = HashMap::new();
            for cap in capabilities {
                let category = cap.id.split('.').next().unwrap_or("unknown");
                *categories.entry(category).or_insert(0) += 1;
            }
            
            for (category, count) in categories {
                println!("  {}: {} capabilities", category, count);
            }
        }
        
        println!("Type :help for commands, :quit to exit\n");
        
        let mut rl = DefaultEditor::new()?;
        
        loop {
            let readline = rl.readline("rtfs> ");
            match readline {
                Ok(line) => {
                    let line = line.trim();
                    
                    if line.is_empty() {
                        continue;
                    }
                    
                    if line.starts_with(':') {
                        if let Err(e) = self.handle_command(line).await {
                            eprintln!("Command error: {}", e);
                        }
                        continue;
                    }
                    
                    rl.add_history_entry(line.clone());
                    self.history.push(line.to_string());
                    
                    match self.evaluate(line).await {
                        Ok(result) => println!("{:?}", result),
                        Err(e) => eprintln!("Error: {}", e),
                    }
                }
                Err(ReadlineError::Interrupted) => {
                    println!("Interrupted");
                    break;
                }
                Err(ReadlineError::Eof) => {
                    println!("EOF");
                    break;
                }
                Err(err) => {
                    eprintln!("Error: {:?}", err);
                    break;
                }
            }
        }
        
        Ok(())
    }
    
    async fn handle_command(&mut self, command: &str) -> Result<(), String> {
        match command {
            ":help" => self.show_help(),
            ":quit" | ":q" => std::process::exit(0),
            ":capabilities" | ":caps" => self.show_capabilities(),
            ":security" | ":sec" => self.show_security_info(),
            ":providers" => self.show_providers(),
            ":history" => self.show_history(),
            cmd if cmd.starts_with(":enable-mcp ") => {
                let url = cmd.strip_prefix(":enable-mcp ").unwrap();
                self.enable_mcp_server(url).await
            }
            cmd if cmd.starts_with(":load-plugin ") => {
                let path = cmd.strip_prefix(":load-plugin ").unwrap();
                self.load_plugin(path)
            }
            _ => Err(format!("Unknown command: {}", command)),
        }
    }
    
    fn show_capabilities(&self) {
        if let Some(registry) = &self.runtime.capability_registry {
            let capabilities = registry.list_all_capabilities();
            
            if capabilities.is_empty() {
                println!("No capabilities available");
                return;
            }
            
            // Group by provider
            let mut by_provider = HashMap::new();
            for cap in capabilities {
                let provider = cap.id.split('.').next().unwrap_or("unknown");
                by_provider.entry(provider).or_insert(Vec::new()).push(cap);
            }
            
            for (provider, caps) in by_provider {
                println!("\n{}:", provider);
                for cap in caps {
                    println!("  {} - {}", cap.id, cap.description);
                    if !cap.security_requirements.permissions.is_empty() {
                        println!("    Permissions: {:?}", cap.security_requirements.permissions);
                    }
                }
            }
        } else {
            println!("Pure mode - no capabilities available");
        }
    }
    
    async fn enable_mcp_server(&mut self, url: &str) -> Result<(), String> {
        if let Some(registry) = self.runtime.capability_registry.as_mut() {
            let config = MCPServerConfig {
                name: format!("repl-mcp-{}", url.split('/').last().unwrap_or("server")),
                url: url.to_string(),
                auth: None,
                requires_microvm: false,
            };
            
            let provider = MCPCapabilityProvider::new(config)?;
            registry.register_provider(Arc::new(provider))?;
            
            println!("MCP server enabled: {}", url);
            Ok(())
        } else {
            Err("CCOS not available in pure mode".to_string())
        }
    }
}
```

This comprehensive runtime integration provides:

1. **Seamless integration** between RTFS pure functions and CCOS capabilities
2. **Flexible configuration** supporting pure, controlled, and full security modes
3. **Command-line interface** with capability management
4. **Enhanced REPL** with live capability discovery and management
5. **Clear security boundaries** with explicit permission requirements
6. **Extensible architecture** supporting plugins, MCP servers, and A2A agents

The implementation maintains backward compatibility while providing a clear migration path to the secure, extensible architecture.
