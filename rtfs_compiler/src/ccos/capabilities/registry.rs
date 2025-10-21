// CCOS Capability Registry
// This module manages dangerous operations that require special permissions,
// sandboxing, or delegation to secure execution environments.

use crate::ast::{Keyword, MapKey};
use crate::ccos::capabilities::capability::Capability;
use crate::ccos::capabilities::provider::CapabilityProvider;
use crate::ccos::synthesis::missing_capability_resolver::MissingCapabilityResolver;
use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::runtime::microvm::{ExecutionContext, MicroVMConfig, MicroVMFactory};
use crate::runtime::security::{RuntimeContext, SecurityAuthorizer};
use crate::runtime::values::{Arity, Value};
use reqwest::blocking::Client as BlockingHttpClient;
use reqwest::{Method as HttpMethod, Url};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

/// Execution policy for capabilities - determines how effects are handled
#[derive(Debug, Clone, PartialEq)]
pub enum CapabilityExecutionPolicy {
    /// All capabilities must go through marketplace/providers (production mode)
    Marketplace,
    /// Hybrid mode: safe capabilities can use LocalProvider, risky ones use marketplace
    Hybrid,
    /// Development mode: allows inline execution for basic capabilities
    InlineDev,
}

impl Default for CapabilityExecutionPolicy {
    fn default() -> Self {
        Self::Hybrid
    }
}

/// Local provider that wraps basic host operations for development/bootstrap
/// In production, these should be replaced by proper marketplace providers
#[derive(Debug)]
pub struct LocalProvider {
    http_mocking_enabled: bool,
    http_allow_hosts: Option<HashSet<String>>,
}

impl LocalProvider {
    pub fn new(http_mocking_enabled: bool, http_allow_hosts: Option<HashSet<String>>) -> Self {
        Self {
            http_mocking_enabled,
            http_allow_hosts,
        }
    }

    /// Execute HTTP fetch with local implementation
    fn execute_http_fetch_local(&self, args: &[Value]) -> RuntimeResult<Value> {
        eprintln!("LocalProvider::execute_http_fetch_local called (http_mocking_enabled={}, allow_hosts={:?}) args={:?}", self.http_mocking_enabled, self.http_allow_hosts, args);
        if self.http_mocking_enabled {
            let mut response_map = std::collections::HashMap::new();
            response_map.insert(MapKey::String("status".to_string()), Value::Integer(200));

            let url = args
                .get(0)
                .and_then(|v| v.as_string())
                .unwrap_or("http://localhost:9999/mock");
            response_map.insert(
                MapKey::String("body".to_string()),
                Value::String(format!(
                    "{{\"args\": {{}}, \"headers\": {{}}, \"origin\": \"127.0.0.1\", \"url\": \"{}\"}}",
                    url
                )),
            );

            let mut headers_map = std::collections::HashMap::new();
            headers_map.insert(
                MapKey::String("content-type".to_string()),
                Value::String("application/json".to_string()),
            );
            response_map.insert(
                MapKey::String("headers".to_string()),
                Value::Map(headers_map),
            );

            return Ok(Value::Map(response_map));
        }

        // For real HTTP requests, delegate to the registry's implementation
        // This would need to be refactored to avoid circular dependency
        Err(RuntimeError::Generic(
            "Real HTTP requests not supported in LocalProvider - use marketplace provider"
                .to_string(),
        ))
    }

    // System capability implementations
    fn get_env_capability(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "ccos.system.get-env".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        match &args[0] {
            Value::String(key) => match std::env::var(key) {
                Ok(value) => Ok(Value::String(value)),
                Err(_) => Ok(Value::Nil),
            },
            _ => Err(RuntimeError::TypeError {
                expected: "string".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "ccos.system.get-env".to_string(),
            }),
        }
    }

    fn current_time_capability(args: Vec<Value>) -> RuntimeResult<Value> {
        if !args.is_empty() {
            return Err(RuntimeError::ArityMismatch {
                function: "ccos.system.current-time".to_string(),
                expected: "0".to_string(),
                actual: args.len(),
            });
        }

        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Ok(Value::Integer(timestamp as i64))
    }

    fn current_timestamp_ms_capability(args: Vec<Value>) -> RuntimeResult<Value> {
        if !args.is_empty() {
            return Err(RuntimeError::ArityMismatch {
                function: "ccos.system.current-timestamp-ms".to_string(),
                expected: "0".to_string(),
                actual: args.len(),
            });
        }

        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        Ok(Value::Integer(timestamp as i64))
    }

    // I/O capability implementations
    fn file_exists_capability(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "ccos.io.file-exists".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        match &args[0] {
            Value::String(path) => Ok(Value::Boolean(std::path::Path::new(path).exists())),
            _ => Err(RuntimeError::TypeError {
                expected: "string".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "ccos.io.file-exists".to_string(),
            }),
        }
    }

    fn parse_json_capability(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "ccos.data.parse-json".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        match &args[0] {
            Value::String(json_str) => match serde_json::from_str::<serde_json::Value>(json_str) {
                Ok(json_value) => Ok(Self::json_value_to_rtfs_value(&json_value)),
                Err(e) => Err(RuntimeError::Generic(format!("JSON parsing error: {}", e))),
            },
            _ => Err(RuntimeError::TypeError {
                expected: "string".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "ccos.data.parse-json".to_string(),
            }),
        }
    }

    fn serialize_json_capability(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "ccos.data.serialize-json".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        let json_value = Self::rtfs_value_to_json_value(&args[0])?;
        match serde_json::to_string_pretty(&json_value) {
            Ok(json_str) => Ok(Value::String(json_str)),
            Err(e) => Err(RuntimeError::Generic(format!(
                "JSON serialization error: {}",
                e
            ))),
        }
    }

    fn log_capability(args: Vec<Value>) -> RuntimeResult<Value> {
        let message = args
            .iter()
            .map(|v| format!("{:?}", v))
            .collect::<Vec<_>>()
            .join(" ");
        println!("[CCOS-LOG] {}", message);
        Ok(Value::Nil)
    }

    fn print_capability(args: Vec<Value>) -> RuntimeResult<Value> {
        let message = args
            .iter()
            .map(|v| format!("{:?}", v))
            .collect::<Vec<_>>()
            .join(" ");
        print!("{}", message);
        Ok(Value::Nil)
    }

    fn println_capability(args: Vec<Value>) -> RuntimeResult<Value> {
        let message = args
            .iter()
            .map(|v| format!("{:?}", v))
            .collect::<Vec<_>>()
            .join(" ");
        println!("{}", message);
        Ok(Value::Nil)
    }

    fn ask_human_capability(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.is_empty() {
            return Err(RuntimeError::ArityMismatch {
                function: "ccos.agent.ask-human".to_string(),
                expected: ">=1".to_string(),
                actual: 0,
            });
        }

        match &args[0] {
            Value::String(prompt) => {
                print!("{}: ", prompt);
                std::io::Write::flush(&mut std::io::stdout())
                    .map_err(|e| RuntimeError::Generic(format!("Failed to flush stdout: {}", e)))?;

                let mut input = String::new();
                std::io::stdin().read_line(&mut input).map_err(|e| {
                    RuntimeError::Generic(format!("Failed to read user input: {}", e))
                })?;

                let user_response = input.trim().to_string();
                Ok(Value::String(user_response))
            }
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "string".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "ccos.agent.ask-human".to_string(),
                });
            }
        }
    }

    // State capability implementations
    fn kv_get_capability(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "ccos.state.kv.get".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        let key = match &args[0] {
            Value::String(s) => s.clone(),
            Value::Keyword(k) => k.0.clone(),
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "string or keyword".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "kv.get".to_string(),
                })
            }
        };

        eprintln!("HOST_CALL: kv.get({}) - mock", key);
        Ok(Value::String(format!("mock-value-for-{}", key)))
    }

    fn kv_put_capability(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "ccos.state.kv.put".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        let key = match &args[0] {
            Value::String(s) => s.clone(),
            Value::Keyword(k) => k.0.clone(),
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "string or keyword".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "kv.put".to_string(),
                })
            }
        };

        eprintln!("HOST_CALL: kv.put({}, <value>) - mock", key);
        Ok(Value::Boolean(true))
    }

    fn kv_cas_put_capability(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 3 {
            return Err(RuntimeError::ArityMismatch {
                function: "ccos.state.kv.cas-put".to_string(),
                expected: "3".to_string(),
                actual: args.len(),
            });
        }

        let key = match &args[0] {
            Value::String(s) => s.clone(),
            Value::Keyword(k) => k.0.clone(),
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "string or keyword".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "kv.cas-put".to_string(),
                })
            }
        };

        eprintln!("HOST_CALL: kv.cas-put({}, <expected>, <new>) - mock", key);
        Ok(Value::Boolean(true))
    }

    fn counter_inc_capability(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.is_empty() {
            return Err(RuntimeError::ArityMismatch {
                function: "ccos.state.counter.inc".to_string(),
                expected: "at least 1".to_string(),
                actual: args.len(),
            });
        }

        let key = match &args[0] {
            Value::String(s) => s.clone(),
            Value::Keyword(k) => k.0.clone(),
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "string or keyword".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "counter.inc".to_string(),
                })
            }
        };

        let increment = if args.len() > 1 {
            match &args[1] {
                Value::Integer(i) => *i as i64,
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "integer".to_string(),
                        actual: args[1].type_name().to_string(),
                        operation: "counter.inc".to_string(),
                    })
                }
            }
        } else {
            1i64
        };

        eprintln!("HOST_CALL: counter.inc({}, {}) - mock", key, increment);
        Ok(Value::Integer(42i64))
    }

    fn event_append_capability(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.is_empty() {
            return Err(RuntimeError::ArityMismatch {
                function: "ccos.state.event.append".to_string(),
                expected: "at least 1".to_string(),
                actual: args.len(),
            });
        }

        let key = match &args[0] {
            Value::String(s) => s.clone(),
            Value::Keyword(k) => k.0.clone(),
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "string or keyword".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "event.append".to_string(),
                })
            }
        };

        eprintln!("HOST_CALL: event.append({}, <event-data>) - mock", key);
        Ok(Value::Boolean(true))
    }

    // Helper functions for JSON conversion
    fn json_value_to_rtfs_value(json_value: &serde_json::Value) -> Value {
        match json_value {
            serde_json::Value::Null => Value::Nil,
            serde_json::Value::Bool(b) => Value::Boolean(*b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Value::Integer(i)
                } else if let Some(f) = n.as_f64() {
                    Value::Float(f)
                } else {
                    Value::Integer(0)
                }
            }
            serde_json::Value::String(s) => {
                if s.starts_with(':') {
                    Value::Keyword(Keyword(s[1..].to_string()))
                } else {
                    Value::String(s.clone())
                }
            }
            serde_json::Value::Array(arr) => {
                let values: Vec<Value> = arr.iter().map(Self::json_value_to_rtfs_value).collect();
                Value::Vector(values)
            }
            serde_json::Value::Object(obj) => {
                let mut map = std::collections::HashMap::new();
                for (key, value) in obj {
                    let map_key = if key.starts_with(':') {
                        crate::ast::MapKey::Keyword(Keyword(key[1..].to_string()))
                    } else {
                        crate::ast::MapKey::String(key.clone())
                    };
                    map.insert(map_key, Self::json_value_to_rtfs_value(value));
                }
                Value::Map(map)
            }
        }
    }

    fn rtfs_value_to_json_value(rtfs_value: &Value) -> RuntimeResult<serde_json::Value> {
        match rtfs_value {
            Value::Nil => Ok(serde_json::Value::Null),
            Value::Boolean(b) => Ok(serde_json::Value::Bool(*b)),
            Value::Integer(i) => Ok(serde_json::Value::Number(serde_json::Number::from(*i))),
            Value::Float(f) => serde_json::Number::from_f64(*f)
                .map(serde_json::Value::Number)
                .ok_or_else(|| RuntimeError::Generic("Invalid float value".to_string())),
            Value::String(s) => Ok(serde_json::Value::String(s.clone())),
            Value::Vector(vec) => {
                let json_array: Result<Vec<serde_json::Value>, RuntimeError> =
                    vec.iter().map(Self::rtfs_value_to_json_value).collect();
                Ok(serde_json::Value::Array(json_array?))
            }
            Value::Map(map) => {
                let mut json_obj = serde_json::Map::new();
                for (key, value) in map {
                    let key_str = match key {
                        crate::ast::MapKey::String(s) => s.clone(),
                        crate::ast::MapKey::Keyword(k) => format!(":{}", k.0),
                        _ => continue,
                    };
                    json_obj.insert(key_str, Self::rtfs_value_to_json_value(value)?);
                }
                Ok(serde_json::Value::Object(json_obj))
            }
            Value::Keyword(k) => Ok(serde_json::Value::String(format!(":{}", k.0))),
            Value::Symbol(s) => Ok(serde_json::Value::String(format!("{}", s.0))),
            Value::Timestamp(ts) => Ok(serde_json::Value::String(format!("@{}", ts))),
            Value::Uuid(uuid) => Ok(serde_json::Value::String(format!("@{}", uuid))),
            Value::ResourceHandle(handle) => Ok(serde_json::Value::String(format!("@{}", handle))),
            Value::Function(_) => Err(RuntimeError::Generic(
                "Cannot serialize functions to JSON".to_string(),
            )),
            Value::FunctionPlaceholder(_) => Err(RuntimeError::Generic(
                "Cannot serialize function placeholders to JSON".to_string(),
            )),
            Value::Error(e) => Err(RuntimeError::Generic(format!(
                "Cannot serialize errors to JSON: {}",
                e.message
            ))),
            Value::List(_) => Err(RuntimeError::Generic(
                "Cannot serialize lists to JSON (use vectors instead)".to_string(),
            )),
        }
    }
}

impl CapabilityProvider for LocalProvider {
    fn provider_id(&self) -> &str {
        "local"
    }

    fn list_capabilities(&self) -> Vec<crate::ccos::capabilities::provider::CapabilityDescriptor> {
        // Return empty list - capabilities are registered directly in the registry
        vec![]
    }

    fn execute_capability(
        &self,
        capability_id: &str,
        inputs: &Value,
        _context: &crate::ccos::capabilities::provider::ExecutionContext,
    ) -> RuntimeResult<Value> {
        // Extract args from inputs
        let args = match inputs {
            Value::Vector(vec) => vec.clone(),
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "vector".to_string(),
                    actual: inputs.type_name().to_string(),
                    operation: "local provider".to_string(),
                })
            }
        };

        // Route to appropriate capability implementation
        match capability_id {
            "ccos.system.get-env" => Self::get_env_capability(args),
            "ccos.system.current-time" => Self::current_time_capability(args),
            "ccos.system.current-timestamp-ms" => Self::current_timestamp_ms_capability(args),
            "ccos.io.file-exists" => Self::file_exists_capability(args),
            "ccos.data.parse-json" => Self::parse_json_capability(args),
            "ccos.data.serialize-json" => Self::serialize_json_capability(args),
            "ccos.io.log" => Self::log_capability(args),
            "ccos.io.print" => Self::print_capability(args),
            "ccos.io.println" => Self::println_capability(args),
            "ccos.agent.ask-human" => Self::ask_human_capability(args),
            "ccos.state.kv.get" => Self::kv_get_capability(args),
            "ccos.state.kv.put" => Self::kv_put_capability(args),
            "ccos.state.kv.cas-put" => Self::kv_cas_put_capability(args),
            "ccos.state.counter.inc" => Self::counter_inc_capability(args),
            "ccos.state.event.append" => Self::event_append_capability(args),
            "ccos.network.http-fetch" => {
                eprintln!("LocalProvider::execute_capability handling ccos.network.http-fetch (http_mocking_enabled={})", self.http_mocking_enabled);
                self.execute_http_fetch_local(&args)
            }
            _ => Err(RuntimeError::Generic(format!(
                "Capability '{}' not supported by LocalProvider",
                capability_id
            ))),
        }
    }

    fn initialize(
        &mut self,
        _config: &crate::ccos::capabilities::provider::ProviderConfig,
    ) -> Result<(), String> {
        Ok(())
    }

    fn health_check(&self) -> crate::ccos::capabilities::provider::HealthStatus {
        crate::ccos::capabilities::provider::HealthStatus::Healthy
    }

    fn metadata(&self) -> crate::ccos::capabilities::provider::ProviderMetadata {
        crate::ccos::capabilities::provider::ProviderMetadata {
            name: "LocalProvider".to_string(),
            version: "1.0.0".to_string(),
            description: "Local provider for development and bootstrap capabilities".to_string(),
            author: "CCOS Team".to_string(),
            license: Some("MIT".to_string()),
            dependencies: vec![],
        }
    }
}

/// Registry of CCOS capabilities that require special execution
pub struct CapabilityRegistry {
    capabilities: HashMap<String, Capability>,
    providers: HashMap<String, Box<dyn CapabilityProvider>>, // Pluggable providers
    microvm_factory: MicroVMFactory,
    microvm_provider: Option<String>,
    /// Optional missing capability resolver for runtime trap
    missing_capability_resolver: Option<Arc<MissingCapabilityResolver>>,
    http_mocking_enabled: bool,
    http_allow_hosts: Option<HashSet<String>>,
    /// Execution policy for capabilities
    execution_policy: CapabilityExecutionPolicy,
}

impl CapabilityRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            capabilities: HashMap::new(),
            providers: HashMap::new(),
            microvm_factory: MicroVMFactory::new(),
            microvm_provider: None,
            missing_capability_resolver: None,
            http_mocking_enabled: true,
            http_allow_hosts: None,
            execution_policy: CapabilityExecutionPolicy::default(),
        };

        // Register system capabilities
        registry.register_system_capabilities();
        registry.register_io_capabilities();
        registry.register_network_capabilities();
        registry.register_agent_capabilities();
        registry.register_state_capabilities();

        // Register LocalProvider for development/bootstrap
        registry.register_local_provider();

        registry
    }

    /// Set the missing capability resolver for runtime trap functionality
    pub fn set_missing_capability_resolver(&mut self, resolver: Arc<MissingCapabilityResolver>) {
        self.missing_capability_resolver = Some(resolver);
    }

    /// Set the execution policy for capabilities
    pub fn set_execution_policy(&mut self, policy: CapabilityExecutionPolicy) {
        self.execution_policy = policy;
    }

    /// Get the current execution policy
    pub fn get_execution_policy(&self) -> &CapabilityExecutionPolicy {
        &self.execution_policy
    }

    /// Register the LocalProvider for development/bootstrap capabilities
    fn register_local_provider(&mut self) {
        let local_provider =
            LocalProvider::new(self.http_mocking_enabled, self.http_allow_hosts.clone());
        self.providers
            .insert("local".to_string(), Box::new(local_provider));
    }

    /// Enqueue a missing capability for resolution without attempting execution.
    /// This is used by orchestrator/marketplace to mark a capability as pending
    /// and trigger the Phase 2 resolution pipeline.
    pub fn enqueue_missing_capability(
        &self,
        capability_id: String,
        args: Vec<Value>,
        runtime_context: Option<&RuntimeContext>,
    ) -> RuntimeResult<()> {
        if let Some(resolver) = &self.missing_capability_resolver {
            let mut context = std::collections::HashMap::new();
            if runtime_context.is_some() {
                context.insert("context_available".to_string(), "true".to_string());
            }
            resolver.handle_missing_capability(capability_id, args, context)
        } else {
            Err(RuntimeError::Generic(
                "MissingCapabilityResolver not configured".to_string(),
            ))
        }
    }

    /// Register an additional capability with the registry.
    ///
    /// This is primarily intended for dynamic capabilities discovered at runtime (e.g. MCP).
    /// The caller is responsible for ensuring the capability's implementation enforces any
    /// required security policies.
    pub fn register_custom_capability(&mut self, capability: Capability) {
        self.capabilities.insert(capability.id.clone(), capability);
    }
    /// Register a capability provider (e.g., MCP, plugin, etc)
    pub fn register_provider(&mut self, provider_id: &str, provider: Box<dyn CapabilityProvider>) {
        self.providers.insert(provider_id.to_string(), provider);
    }

    pub fn set_http_mocking_enabled(&mut self, enabled: bool) {
        self.http_mocking_enabled = enabled;
        // If a local provider was already registered, replace it with a
        // new instance so the provider receives the updated mocking flag.
        if self.providers.contains_key("local") {
            let new_local =
                LocalProvider::new(self.http_mocking_enabled, self.http_allow_hosts.clone());
            self.providers
                .insert("local".to_string(), Box::new(new_local));
        }
    }

    pub fn set_http_allow_hosts(&mut self, hosts: Vec<String>) -> RuntimeResult<()> {
        if hosts.is_empty() {
            self.http_allow_hosts = None;
            return Ok(());
        }

        let mut normalized = HashSet::with_capacity(hosts.len());
        for host in hosts {
            let trimmed = host.trim();
            if trimmed.is_empty() {
                return Err(RuntimeError::Generic(
                    "HTTP allowlist entries must not be empty".to_string(),
                ));
            }
            normalized.insert(trimmed.to_lowercase());
        }

        self.http_allow_hosts = Some(normalized);
        // If a local provider exists, replace it so it receives the updated allowlist
        if self.providers.contains_key("local") {
            let new_local =
                LocalProvider::new(self.http_mocking_enabled, self.http_allow_hosts.clone());
            self.providers
                .insert("local".to_string(), Box::new(new_local));
        }
        Ok(())
    }

    /// Get a provider by ID
    pub fn get_provider(&self, provider_id: &str) -> Option<&Box<dyn CapabilityProvider>> {
        self.providers.get(provider_id)
    }

    pub fn list_providers(&self) -> Vec<&str> {
        self.providers.keys().map(|k| k.as_str()).collect()
    }

    pub fn register_system_capabilities(&mut self) {
        // Environment access capability - delegates to provider
        self.capabilities.insert(
            "ccos.system.get-env".to_string(),
            Capability {
                id: "ccos.system.get-env".to_string(),
                arity: Arity::Fixed(1),
                func: Arc::new(|_args| {
                    Err(RuntimeError::Generic(
                        "System capabilities must be executed through providers".to_string(),
                    ))
                }),
            },
        );

        // Time access capability - delegates to provider
        self.capabilities.insert(
            "ccos.system.current-time".to_string(),
            Capability {
                id: "ccos.system.current-time".to_string(),
                arity: Arity::Fixed(0),
                func: Arc::new(|_args| {
                    Err(RuntimeError::Generic(
                        "System capabilities must be executed through providers".to_string(),
                    ))
                }),
            },
        );

        // Timestamp capability - delegates to provider
        self.capabilities.insert(
            "ccos.system.current-timestamp-ms".to_string(),
            Capability {
                id: "ccos.system.current-timestamp-ms".to_string(),
                arity: Arity::Fixed(0),
                func: Arc::new(|_args| {
                    Err(RuntimeError::Generic(
                        "System capabilities must be executed through providers".to_string(),
                    ))
                }),
            },
        );
    }

    fn register_io_capabilities(&mut self) {
        // File operations - delegate to providers
        self.capabilities.insert(
            "ccos.io.file-exists".to_string(),
            Capability {
                id: "ccos.io.file-exists".to_string(),
                arity: Arity::Fixed(1),
                func: Arc::new(|_args| {
                    Err(RuntimeError::Generic(
                        "I/O capabilities must be executed through providers".to_string(),
                    ))
                }),
            },
        );

        self.capabilities.insert(
            "ccos.io.open-file".to_string(),
            Capability {
                id: "ccos.io.open-file".to_string(),
                arity: Arity::Variadic(1),
                func: Arc::new(|_args| {
                    Err(RuntimeError::Generic(
                        "File operations must be executed through providers".to_string(),
                    ))
                }),
            },
        );

        self.capabilities.insert(
            "ccos.io.read-line".to_string(),
            Capability {
                id: "ccos.io.read-line".to_string(),
                arity: Arity::Fixed(1),
                func: Arc::new(|_args| {
                    Err(RuntimeError::Generic(
                        "File operations must be executed through providers".to_string(),
                    ))
                }),
            },
        );

        self.capabilities.insert(
            "ccos.io.write-line".to_string(),
            Capability {
                id: "ccos.io.write-line".to_string(),
                arity: Arity::Fixed(2),
                func: Arc::new(|_args| {
                    Err(RuntimeError::Generic(
                        "File operations must be executed through providers".to_string(),
                    ))
                }),
            },
        );

        self.capabilities.insert(
            "ccos.io.close-file".to_string(),
            Capability {
                id: "ccos.io.close-file".to_string(),
                arity: Arity::Fixed(1),
                func: Arc::new(|_args| {
                    Err(RuntimeError::Generic(
                        "File operations must be executed through providers".to_string(),
                    ))
                }),
            },
        );

        // JSON operations - delegate to providers
        self.capabilities.insert(
            "ccos.data.parse-json".to_string(),
            Capability {
                id: "ccos.data.parse-json".to_string(),
                arity: Arity::Fixed(1),
                func: Arc::new(|_args| {
                    Err(RuntimeError::Generic(
                        "Data capabilities must be executed through providers".to_string(),
                    ))
                }),
            },
        );

        self.capabilities.insert(
            "ccos.data.serialize-json".to_string(),
            Capability {
                id: "ccos.data.serialize-json".to_string(),
                arity: Arity::Fixed(1),
                func: Arc::new(|_args| {
                    Err(RuntimeError::Generic(
                        "Data capabilities must be executed through providers".to_string(),
                    ))
                }),
            },
        );

        // Logging capabilities - delegate to providers
        self.capabilities.insert(
            "ccos.io.log".to_string(),
            Capability {
                id: "ccos.io.log".to_string(),
                arity: Arity::Variadic(1),
                func: Arc::new(|_args| {
                    Err(RuntimeError::Generic(
                        "Logging capabilities must be executed through providers".to_string(),
                    ))
                }),
            },
        );

        self.capabilities.insert(
            "ccos.io.print".to_string(),
            Capability {
                id: "ccos.io.print".to_string(),
                arity: Arity::Variadic(1),
                func: Arc::new(|_args| {
                    Err(RuntimeError::Generic(
                        "Output capabilities must be executed through providers".to_string(),
                    ))
                }),
            },
        );

        self.capabilities.insert(
            "ccos.io.println".to_string(),
            Capability {
                id: "ccos.io.println".to_string(),
                arity: Arity::Variadic(1),
                func: Arc::new(|_args| {
                    Err(RuntimeError::Generic(
                        "Output capabilities must be executed through providers".to_string(),
                    ))
                }),
            },
        );
    }

    fn register_network_capabilities(&mut self) {
        // HTTP operations
        self.capabilities.insert(
			"ccos.network.http-fetch".to_string(),
			Capability {
				id: "ccos.network.http-fetch".to_string(),
				arity: Arity::Variadic(1),
				func: Arc::new(|_args| {
					// HTTP operations must be executed through MicroVM isolation
					Err(RuntimeError::Generic(
						"Network operations must be executed through MicroVM isolation. Use CapabilityRegistry::execute_capability_with_microvm()".to_string(),
					))
				}),
			},
		);
    }

    fn register_agent_capabilities(&mut self) {
        // Agent operations - delegate to providers
        self.capabilities.insert(
            "ccos.agent.discover-agents".to_string(),
            Capability {
                id: "ccos.agent.discover-agents".to_string(),
                arity: Arity::Variadic(0),
                func: Arc::new(|_args| {
                    Err(RuntimeError::Generic(
                        "Agent capabilities must be executed through providers".to_string(),
                    ))
                }),
            },
        );

        self.capabilities.insert(
            "ccos.agent.task-coordination".to_string(),
            Capability {
                id: "ccos.agent.task-coordination".to_string(),
                arity: Arity::Variadic(0),
                func: Arc::new(|_args| {
                    Err(RuntimeError::Generic(
                        "Agent capabilities must be executed through providers".to_string(),
                    ))
                }),
            },
        );

        self.capabilities.insert(
            "ccos.agent.ask-human".to_string(),
            Capability {
                id: "ccos.agent.ask-human".to_string(),
                arity: Arity::Variadic(1),
                func: Arc::new(|_args| {
                    Err(RuntimeError::Generic(
                        "Agent capabilities must be executed through providers".to_string(),
                    ))
                }),
            },
        );

        self.capabilities.insert(
            "ccos.agent.discover-and-assess-agents".to_string(),
            Capability {
                id: "ccos.agent.discover-and-assess-agents".to_string(),
                arity: Arity::Variadic(0),
                func: Arc::new(|_args| {
                    Err(RuntimeError::Generic(
                        "Agent capabilities must be executed through providers".to_string(),
                    ))
                }),
            },
        );

        self.capabilities.insert(
            "ccos.agent.establish-system-baseline".to_string(),
            Capability {
                id: "ccos.agent.establish-system-baseline".to_string(),
                arity: Arity::Variadic(0),
                func: Arc::new(|_args| {
                    Err(RuntimeError::Generic(
                        "Agent capabilities must be executed through providers".to_string(),
                    ))
                }),
            },
        );
    }

    /// Register host-backed state capabilities that replace atoms
    fn register_state_capabilities(&mut self) {
        // Key-value store operations - delegate to providers
        self.capabilities.insert(
            "ccos.state.kv.get".to_string(),
            Capability {
                id: "ccos.state.kv.get".to_string(),
                arity: Arity::Fixed(1),
                func: Arc::new(|_args| {
                    Err(RuntimeError::Generic(
                        "State capabilities must be executed through providers".to_string(),
                    ))
                }),
            },
        );

        self.capabilities.insert(
            "ccos.state.kv.put".to_string(),
            Capability {
                id: "ccos.state.kv.put".to_string(),
                arity: Arity::Fixed(2),
                func: Arc::new(|_args| {
                    Err(RuntimeError::Generic(
                        "State capabilities must be executed through providers".to_string(),
                    ))
                }),
            },
        );

        self.capabilities.insert(
            "ccos.state.kv.cas-put".to_string(),
            Capability {
                id: "ccos.state.kv.cas-put".to_string(),
                arity: Arity::Fixed(3), // key, expected_value, new_value
                func: Arc::new(|_args| {
                    Err(RuntimeError::Generic(
                        "State capabilities must be executed through providers".to_string(),
                    ))
                }),
            },
        );

        // Counter operations - delegate to providers
        self.capabilities.insert(
            "ccos.state.counter.inc".to_string(),
            Capability {
                id: "ccos.state.counter.inc".to_string(),
                arity: Arity::Variadic(1), // key, increment (default 1)
                func: Arc::new(|_args| {
                    Err(RuntimeError::Generic(
                        "State capabilities must be executed through providers".to_string(),
                    ))
                }),
            },
        );

        // Event log operations - delegate to providers
        self.capabilities.insert(
            "ccos.state.event.append".to_string(),
            Capability {
                id: "ccos.state.event.append".to_string(),
                arity: Arity::Variadic(1), // key, event_data...
                func: Arc::new(|_args| {
                    Err(RuntimeError::Generic(
                        "State capabilities must be executed through providers".to_string(),
                    ))
                }),
            },
        );
    }

    pub fn get_capability(&self, id: &str) -> Option<&Capability> {
        self.capabilities.get(id)
    }

    pub fn list_capabilities(&self) -> Vec<&str> {
        self.capabilities.keys().map(|k| k.as_str()).collect()
    }

    /// Configure the MicroVM provider to use
    pub fn set_microvm_provider(&mut self, provider_name: &str) -> RuntimeResult<()> {
        let available_providers = self.microvm_factory.get_available_providers();
        if !available_providers.contains(&provider_name) {
            return Err(RuntimeError::Generic(format!(
                "MicroVM provider '{}' not available. Available providers: {:?}",
                provider_name, available_providers
            )));
        }

        // Initialize the provider when setting it
        self.microvm_factory.initialize_provider(provider_name)?;

        self.microvm_provider = Some(provider_name.to_string());
        Ok(())
    }

    /// Get the current MicroVM provider
    pub fn get_microvm_provider(&self) -> Option<&str> {
        self.microvm_provider.as_deref()
    }

    /// List available MicroVM providers
    pub fn list_microvm_providers(&self) -> Vec<&str> {
        self.microvm_factory.get_available_providers()
    }

    /// Execute a capability that requires MicroVM isolation
    fn execute_in_microvm(
        &self,
        capability_id: &str,
        args: Vec<Value>,
        runtime_context: Option<&RuntimeContext>,
    ) -> RuntimeResult<Value> {
        eprintln!("CapabilityRegistry::execute_in_microvm called for {} with args={:?} http_mocking_enabled={}", capability_id, args, self.http_mocking_enabled);
        // For HTTP operations, return a mock response for testing
        if capability_id == "ccos.network.http-fetch" {
            if self.http_mocking_enabled {
                let mut response_map = std::collections::HashMap::new();
                response_map.insert(MapKey::String("status".to_string()), Value::Integer(200));

                let url = args
                    .get(0)
                    .and_then(|v| v.as_string())
                    .unwrap_or("http://localhost:9999/mock");
                response_map.insert(
                    MapKey::String("body".to_string()),
                    Value::String(format!(
                        "{{\"args\": {{}}, \"headers\": {{}}, \"origin\": \"127.0.0.1\", \"url\": \"{}\"}}",
                        url
                    )),
                );

                let mut headers_map = std::collections::HashMap::new();
                headers_map.insert(
                    MapKey::String("content-type".to_string()),
                    Value::String("application/json".to_string()),
                );
                response_map.insert(
                    MapKey::String("headers".to_string()),
                    Value::Map(headers_map),
                );

                return Ok(Value::Map(response_map));
            }

            return self.execute_http_fetch(&args);
        }

        // For other capabilities, use the MicroVM provider
        let default_provider = "mock".to_string();
        let provider_name = self.microvm_provider.as_ref().unwrap_or(&default_provider);

        // Get the provider (should already be initialized from set_microvm_provider)
        let provider = self
            .microvm_factory
            .get_provider(provider_name)
            .ok_or_else(|| {
                RuntimeError::Generic(format!("MicroVM provider '{}' not found", provider_name))
            })?;

        // Central authorization: determine required permissions
        let required_permissions = if let Some(rt_ctx) = runtime_context {
            SecurityAuthorizer::authorize_capability(rt_ctx, capability_id, &args)?
        } else {
            // If no runtime context provided, use minimal permissions
            vec![capability_id.to_string()]
        };

        // Create execution context with authorized permissions
        let execution_context = ExecutionContext {
            execution_id: format!("exec_{}", uuid::Uuid::new_v4()),
            program: None,
            capability_id: Some(capability_id.to_string()),
            capability_permissions: required_permissions.clone(),
            args,
            config: runtime_context
                .and_then(|rc| rc.microvm_config_override.clone())
                .unwrap_or_else(MicroVMConfig::default),
            runtime_context: runtime_context.cloned(),
        };

        // Final validation: ensure execution context has all required permissions
        SecurityAuthorizer::validate_execution_context(&required_permissions, &execution_context)?;

        // Execute in the MicroVM
        let result = provider.execute_capability(execution_context)?;
        Ok(result.value)
    }

    /// Execute a capability through the appropriate provider based on execution policy
    pub fn execute_capability_with_microvm(
        &self,
        capability_id: &str,
        args: Vec<Value>,
        runtime_context: Option<&RuntimeContext>,
    ) -> RuntimeResult<Value> {
        eprintln!(
            "CapabilityRegistry::execute_capability_with_microvm called for {} args={:?}",
            capability_id, args
        );
        // Perform security validation if runtime context is provided
        if let Some(context) = runtime_context {
            use crate::runtime::security::SecurityAuthorizer;
            SecurityAuthorizer::authorize_capability(context, capability_id, &args)?;
        }

        // Special-case: route HTTP fetch through the microvm execution helper so
        // that the registry's http_mocking_enabled and allowlist settings are
        // honored. This ensures the REPL flag --http-real controls real network
        // calls for synthetic capabilities.
        if capability_id == "ccos.network.http-fetch" {
            return self.execute_in_microvm(capability_id, args, runtime_context);
        }

        // Determine which provider to use based on execution policy
        let provider_id = match &self.execution_policy {
            CapabilityExecutionPolicy::Marketplace => {
                // All capabilities must go through marketplace providers
                // For now, use local provider as fallback, but in production this should
                // be replaced with proper marketplace provider selection
                "local"
            }
            CapabilityExecutionPolicy::Hybrid => {
                // Safe capabilities can use LocalProvider, risky ones use marketplace
                if self.is_safe_capability(capability_id) {
                    "local"
                } else {
                    // For risky capabilities, require marketplace provider
                    return Err(RuntimeError::Generic(format!(
                        "Capability '{}' requires marketplace provider in hybrid mode",
                        capability_id
                    )));
                }
            }
            CapabilityExecutionPolicy::InlineDev => {
                // Development mode: allow local provider for all capabilities
                "local"
            }
        };

        // Execute through the selected provider
        if let Some(provider) = self.providers.get(provider_id) {
            let context = crate::ccos::capabilities::provider::ExecutionContext {
                trace_id: uuid::Uuid::new_v4().to_string(),
                timeout: std::time::Duration::from_secs(10),
            };
            provider.execute_capability(capability_id, &Value::Vector(args), &context)
        } else {
            // Runtime trap: Handle missing capability through resolver if available
            if let Some(ref resolver) = self.missing_capability_resolver {
                let mut context = std::collections::HashMap::new();
                if let Some(_runtime_context) = runtime_context {
                    context.insert("context_available".to_string(), "true".to_string());
                }

                if let Err(e) = resolver.handle_missing_capability(
                    capability_id.to_string(),
                    args.clone(),
                    context,
                ) {
                    eprintln!(
                        "Warning: Failed to queue missing capability for resolution: {}",
                        e
                    );
                }
            }

            Err(RuntimeError::Generic(format!(
                "No provider available for capability '{}' with policy {:?}",
                capability_id, self.execution_policy
            )))
        }
    }

    /// Determine if a capability is considered "safe" for local execution
    fn is_safe_capability(&self, capability_id: &str) -> bool {
        matches!(
            capability_id,
            "ccos.system.get-env"
                | "ccos.system.current-time"
                | "ccos.system.current-timestamp-ms"
                | "ccos.data.parse-json"
                | "ccos.data.serialize-json"
                | "ccos.io.log"
                | "ccos.io.print"
                | "ccos.io.println"
                | "ccos.network.http-fetch"  // Safe for testing with mock endpoints
                // Local state operations are treated as safe in Hybrid mode for tests and
                // development environments. In production these should be provided by
                // marketplace-backed providers.
                | "ccos.state.counter.inc"
                | "ccos.state.kv.get"
                | "ccos.state.kv.put"
                | "ccos.state.kv.cas-put"
                | "ccos.state.event.append"
        )
    }

    // Agent capability implementations (stubs - these should be implemented by proper providers)
    fn discover_agents_capability(_args: Vec<Value>) -> RuntimeResult<Value> {
        // TODO: Implement with proper capability marketplace integration
        Ok(Value::Vector(vec![]))
    }

    fn task_coordination_capability(_args: Vec<Value>) -> RuntimeResult<Value> {
        // TODO: Implement with proper CCOS task coordination
        Ok(Value::Map(std::collections::HashMap::new()))
    }

    fn discover_and_assess_agents_capability(_args: Vec<Value>) -> RuntimeResult<Value> {
        // TODO: Implement with proper agent discovery system
        Ok(Value::Vector(vec![]))
    }

    fn establish_system_baseline_capability(_args: Vec<Value>) -> RuntimeResult<Value> {
        // TODO: Implement with proper system baseline establishment
        Ok(Value::Map(std::collections::HashMap::new()))
    }
}

impl CapabilityRegistry {
    fn execute_http_fetch(&self, args: &[Value]) -> RuntimeResult<Value> {
        let request = self.parse_http_request(args)?;

        if let Some(allow_hosts) = &self.http_allow_hosts {
            let host = request
                .url
                .host_str()
                .ok_or_else(|| RuntimeError::NetworkError("URL missing host".to_string()))?
                .to_lowercase();

            if !allow_hosts.contains(&host) {
                return Err(RuntimeError::SecurityViolation {
                    operation: "ccos.network.http-fetch".to_string(),
                    capability: "ccos.network.http-fetch".to_string(),
                    context: format!("Host '{}' not in HTTP allowlist", host),
                });
            }
        }

        let mut client_builder = BlockingHttpClient::builder();
        if let Some(timeout) = request.timeout {
            client_builder = client_builder.timeout(timeout);
        }

        let client = client_builder
            .build()
            .map_err(|e| RuntimeError::NetworkError(e.to_string()))?;

        let mut req_builder = client.request(request.method.clone(), request.url.clone());
        for (key, value) in request.headers.iter() {
            req_builder = req_builder.header(key, value);
        }

        if let Some(body) = request.body.clone() {
            req_builder = req_builder.body(body);
        }

        if let Some(timeout) = request.timeout {
            req_builder = req_builder.timeout(timeout);
        }

        let response = req_builder
            .send()
            .map_err(|e| RuntimeError::NetworkError(e.to_string()))?;

        let status = response.status().as_u16() as i64;
        let response_headers = response.headers().clone();
        let resp_body = response
            .text()
            .map_err(|e| RuntimeError::NetworkError(e.to_string()))?;

        let mut response_map = HashMap::new();
        response_map.insert(MapKey::String("status".to_string()), Value::Integer(status));
        response_map.insert(MapKey::String("body".to_string()), Value::String(resp_body));

        let mut headers_map = HashMap::new();
        for (key, value) in response_headers.iter() {
            if let Ok(value_str) = value.to_str() {
                headers_map.insert(
                    MapKey::String(key.as_str().to_string()),
                    Value::String(value_str.to_string()),
                );
            }
        }

        response_map.insert(
            MapKey::String("headers".to_string()),
            Value::Map(headers_map),
        );

        Ok(Value::Map(response_map))
    }

    fn parse_http_request(&self, args: &[Value]) -> RuntimeResult<HttpRequestConfig> {
        if args.is_empty() {
            return Err(RuntimeError::ArityMismatch {
                function: "ccos.network.http-fetch".to_string(),
                expected: "at least 1".to_string(),
                actual: 0,
            });
        }

        let mut method = "GET".to_string();
        let mut url: Option<String> = None;
        let mut headers: Vec<(String, String)> = Vec::new();
        let mut body: Option<String> = None;
        let mut timeout: Option<Duration> = None;

        if args.len() == 1 {
            match &args[0] {
                Value::String(s) => url = Some(s.clone()),
                Value::Map(map) => {
                    for (key, value) in map.iter() {
                        self.assign_http_option(
                            map_key_to_string(key),
                            value,
                            &mut url,
                            &mut method,
                            &mut headers,
                            &mut body,
                            &mut timeout,
                        )?;
                    }
                }
                other => {
                    return Err(RuntimeError::TypeError {
                        expected: "string or map".to_string(),
                        actual: other.type_name().to_string(),
                        operation: "ccos.network.http-fetch".to_string(),
                    })
                }
            }
        } else {
            let pairs = self.collect_keyword_pairs(args)?;
            for (key, value) in pairs {
                self.assign_http_option(
                    key,
                    &value,
                    &mut url,
                    &mut method,
                    &mut headers,
                    &mut body,
                    &mut timeout,
                )?;
            }
        }

        let url_string =
            url.ok_or_else(|| RuntimeError::Generic("Missing :url for HTTP fetch".to_string()))?;
        let parsed_url = Url::parse(&url_string)
            .map_err(|e| RuntimeError::NetworkError(format!("Invalid URL: {}", e)))?;

        let method_enum = HttpMethod::from_bytes(method.as_bytes()).unwrap_or(HttpMethod::GET);

        Ok(HttpRequestConfig {
            url: parsed_url,
            method: method_enum,
            headers,
            body,
            timeout,
        })
    }

    fn collect_keyword_pairs(&self, args: &[Value]) -> RuntimeResult<Vec<(String, Value)>> {
        if args.len() % 2 != 0 {
            return Err(RuntimeError::ArityMismatch {
                function: "ccos.network.http-fetch".to_string(),
                expected: "even number of keyword arguments".to_string(),
                actual: args.len(),
            });
        }

        let mut pairs = Vec::with_capacity(args.len() / 2);
        let mut iter = args.iter();
        while let (Some(key), Some(value)) = (iter.next(), iter.next()) {
            let key_string = match key {
                Value::Keyword(k) => k.0.clone(),
                Value::String(s) => s.clone(),
                other => {
                    return Err(RuntimeError::TypeError {
                        expected: "keyword or string".to_string(),
                        actual: other.type_name().to_string(),
                        operation: "ccos.network.http-fetch".to_string(),
                    })
                }
            };
            pairs.push((strip_leading_colon(&key_string), value.clone()));
        }

        Ok(pairs)
    }

    fn assign_http_option(
        &self,
        key: String,
        value: &Value,
        url: &mut Option<String>,
        method: &mut String,
        headers: &mut Vec<(String, String)>,
        body: &mut Option<String>,
        timeout: &mut Option<Duration>,
    ) -> RuntimeResult<()> {
        match key.as_str() {
            "url" => {
                *url = Some(extract_plain_string(value, "url")?);
            }
            "method" => {
                *method = extract_plain_string(value, "method")?.to_uppercase();
            }
            "headers" => {
                *headers = extract_headers(value)?;
            }
            "body" => {
                *body = Some(extract_plain_string(value, "body")?);
            }
            "timeout" | "timeout-ms" => {
                *timeout = Some(extract_timeout_duration(value)?);
            }
            // Ignore unrecognized options to remain forward-compatible
            _ => {}
        }
        Ok(())
    }
}

fn extract_plain_string(value: &Value, field: &str) -> RuntimeResult<String> {
    match value {
        Value::String(s) => Ok(s.clone()),
        Value::Keyword(k) => Ok(k.0.clone()),
        Value::Integer(i) => Ok(i.to_string()),
        Value::Float(f) => Ok(f.to_string()),
        Value::Boolean(b) => Ok(b.to_string()),
        Value::Nil => Ok(String::new()),
        other => Err(RuntimeError::TypeError {
            expected: "string-compatible".to_string(),
            actual: other.type_name().to_string(),
            operation: format!("ccos.network.http-fetch/{}", field),
        }),
    }
}

fn extract_timeout_duration(value: &Value) -> RuntimeResult<Duration> {
    match value {
        Value::Integer(ms) if *ms >= 0 => Ok(Duration::from_millis(*ms as u64)),
        Value::Float(ms) if *ms >= 0.0 => Ok(Duration::from_millis(*ms as u64)),
        Value::Nil => Ok(Duration::from_millis(0)),
        other => Err(RuntimeError::TypeError {
            expected: "non-negative number".to_string(),
            actual: other.type_name().to_string(),
            operation: "ccos.network.http-fetch/timeout".to_string(),
        }),
    }
}

fn extract_headers(value: &Value) -> RuntimeResult<Vec<(String, String)>> {
    match value {
        Value::Map(map) => {
            let mut headers = Vec::with_capacity(map.len());
            for (key, val) in map {
                let header_key = map_key_to_string(key);
                let header_val = extract_plain_string(val, "header-value")?;
                headers.push((header_key, header_val));
            }
            Ok(headers)
        }
        Value::Nil => Ok(Vec::new()),
        other => Err(RuntimeError::TypeError {
            expected: "map".to_string(),
            actual: other.type_name().to_string(),
            operation: "ccos.network.http-fetch/headers".to_string(),
        }),
    }
}

fn map_key_to_string(key: &MapKey) -> String {
    match key {
        MapKey::String(s) => s.clone(),
        MapKey::Keyword(k) => k.0.clone(),
        MapKey::Integer(i) => i.to_string(),
    }
}

fn strip_leading_colon(input: &str) -> String {
    input.trim_start_matches(':').to_string()
}

struct HttpRequestConfig {
    url: Url,
    method: HttpMethod,
    headers: Vec<(String, String)>,
    body: Option<String>,
    timeout: Option<Duration>,
}

impl Default for CapabilityRegistry {
    fn default() -> Self {
        Self::new()
    }
}
