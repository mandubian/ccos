// CCOS Capability Registry
// This module manages dangerous operations that require special permissions,
// sandboxing, or delegation to secure execution environments.

use crate::runtime::values::{Value, Arity};
use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::ccos::capabilities::capability::Capability;
use crate::ccos::capabilities::provider::CapabilityProvider;
use crate::runtime::microvm::{MicroVMFactory, ExecutionContext, MicroVMConfig};
use crate::runtime::security::{RuntimeContext, SecurityAuthorizer};
use crate::ast::Keyword;
use std::collections::HashMap;
use std::sync::Arc;

/// Registry of CCOS capabilities that require special execution
pub struct CapabilityRegistry {
	capabilities: HashMap<String, Capability>,
	providers: HashMap<String, Box<dyn CapabilityProvider>>, // Pluggable providers
	microvm_factory: MicroVMFactory,
	microvm_provider: Option<String>,
}

impl CapabilityRegistry {
	pub fn new() -> Self {
		let mut registry = Self {
			capabilities: HashMap::new(),
			providers: HashMap::new(),
			microvm_factory: MicroVMFactory::new(),
			microvm_provider: None,
		};

		// Register system capabilities
		registry.register_system_capabilities();
		registry.register_io_capabilities();
		registry.register_network_capabilities();
		registry.register_agent_capabilities();

		registry
	}
	/// Register a capability provider (e.g., MCP, plugin, etc)
	pub fn register_provider(&mut self, provider_id: &str, provider: Box<dyn CapabilityProvider>) {
		self.providers.insert(provider_id.to_string(), provider);
	}

	/// Get a provider by ID
	pub fn get_provider(&self, provider_id: &str) -> Option<&Box<dyn CapabilityProvider>> {
		self.providers.get(provider_id)
	}
    
	/// Get all registered capabilities
	pub fn get_capabilities(&self) -> &HashMap<String, Capability> {
		&self.capabilities
	}
    
	fn register_system_capabilities(&mut self) {
		// Environment access capability
		self.capabilities.insert(
			"ccos.system.get-env".to_string(),
				Capability {
				id: "ccos.system.get-env".to_string(),
				arity: Arity::Fixed(1),
				func: Arc::new(|args| Self::get_env_capability(args)),
			},
		);
        
		// Time access capability
		self.capabilities.insert(
			"ccos.system.current-time".to_string(),
				Capability {
				id: "ccos.system.current-time".to_string(),
				arity: Arity::Fixed(0),
				func: Arc::new(|args| Self::current_time_capability(args)),
			},
		);
        
		// Timestamp capability
		self.capabilities.insert(
			"ccos.system.current-timestamp-ms".to_string(),
				Capability {
				id: "ccos.system.current-timestamp-ms".to_string(),
				arity: Arity::Fixed(0),
				func: Arc::new(|args| Self::current_timestamp_ms_capability(args)),
			},
		);
	}
    
	fn register_io_capabilities(&mut self) {
		// File operations
		self.capabilities.insert(
			"ccos.io.file-exists".to_string(),
			Capability {
				id: "ccos.io.file-exists".to_string(),
				arity: Arity::Fixed(1),
				func: Arc::new(|args| Self::file_exists_capability(args)),
			},
		);
        
		self.capabilities.insert(
			"ccos.io.open-file".to_string(),
			Capability {
				id: "ccos.io.open-file".to_string(),
				arity: Arity::Variadic(1),
				func: Arc::new(|args| Self::open_file_capability(args)),
			},
		);
        
		self.capabilities.insert(
			"ccos.io.read-line".to_string(),
			Capability {
				id: "ccos.io.read-line".to_string(),
				arity: Arity::Fixed(1),
				func: Arc::new(|args| Self::read_line_capability(args)),
			},
		);
        
		self.capabilities.insert(
			"ccos.io.write-line".to_string(),
			Capability {
				id: "ccos.io.write-line".to_string(),
				arity: Arity::Fixed(2),
				func: Arc::new(|args| Self::write_line_capability(args)),
			},
		);
        
		self.capabilities.insert(
			"ccos.io.close-file".to_string(),
			Capability {
				id: "ccos.io.close-file".to_string(),
				arity: Arity::Fixed(1),
				func: Arc::new(|args| Self::close_file_capability(args)),
			},
		);
        
		// JSON operations
		self.capabilities.insert(
			"ccos.data.parse-json".to_string(),
			Capability {
				id: "ccos.data.parse-json".to_string(),
				arity: Arity::Fixed(1),
				func: Arc::new(|args| Self::parse_json_capability(args)),
			},
		);
        
		self.capabilities.insert(
			"ccos.data.serialize-json".to_string(),
			Capability {
				id: "ccos.data.serialize-json".to_string(),
				arity: Arity::Fixed(1),
				func: Arc::new(|args| Self::serialize_json_capability(args)),
			},
		);
        
		// Logging capabilities
		self.capabilities.insert(
			"ccos.io.log".to_string(),
			Capability {
				id: "ccos.io.log".to_string(),
				arity: Arity::Variadic(1),
				func: Arc::new(|args| Self::log_capability(args)),
			},
		);
        
		self.capabilities.insert(
			"ccos.io.print".to_string(),
			Capability {
				id: "ccos.io.print".to_string(),
				arity: Arity::Variadic(1),
				func: Arc::new(|args| Self::print_capability(args)),
			},
		);
        
		self.capabilities.insert(
			"ccos.io.println".to_string(),
			Capability {
				id: "ccos.io.println".to_string(),
				arity: Arity::Variadic(1),
				func: Arc::new(|args| Self::println_capability(args)),
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
		// Agent operations
		self.capabilities.insert(
			"ccos.agent.discover-agents".to_string(),
			Capability {
				id: "ccos.agent.discover-agents".to_string(),
				arity: Arity::Variadic(0),
				func: Arc::new(|args| Self::discover_agents_capability(args)),
			},
		);
        
		self.capabilities.insert(
			"ccos.agent.task-coordination".to_string(),
			Capability {
				id: "ccos.agent.task-coordination".to_string(),
				arity: Arity::Variadic(0),
				func: Arc::new(|args| Self::task_coordination_capability(args)),
			},
		);
        
		self.capabilities.insert(
			"ccos.agent.ask-human".to_string(),
			Capability {
				id: "ccos.agent.ask-human".to_string(),
				arity: Arity::Variadic(1),
				func: Arc::new(|args| Self::ask_human_capability(args)),
			},
		);
        
		self.capabilities.insert(
			"ccos.agent.discover-and-assess-agents".to_string(),
			Capability {
				id: "ccos.agent.discover-and-assess-agents".to_string(),
				arity: Arity::Variadic(0),
				func: Arc::new(|args| Self::discover_and_assess_agents_capability(args)),
			},
		);
        
		self.capabilities.insert(
			"ccos.agent.establish-system-baseline".to_string(),
			Capability {
				id: "ccos.agent.establish-system-baseline".to_string(),
				arity: Arity::Variadic(0),
				func: Arc::new(|args| Self::establish_system_baseline_capability(args)),
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
	fn execute_in_microvm(&self, capability_id: &str, args: Vec<Value>, runtime_context: Option<&RuntimeContext>) -> RuntimeResult<Value> {
		// For HTTP operations, return a mock response for testing
		if capability_id == "ccos.network.http-fetch" {
			// Return mock response for testing without async runtime
			let mut response_map = std::collections::HashMap::new();
			response_map.insert(
				crate::ast::MapKey::String("status".to_string()),
				Value::Integer(200),
			);
            
			let url = args.get(0).and_then(|v| v.as_string()).unwrap_or("http://localhost:9999/mock");
			response_map.insert(
				crate::ast::MapKey::String("body".to_string()),
				Value::String(format!("{{\"args\": {{}}, \"headers\": {{}}, \"origin\": \"127.0.0.1\", \"url\": \"{}\"}}", url)),
			);
            
			let mut headers_map = std::collections::HashMap::new();
			headers_map.insert(
				crate::ast::MapKey::String("content-type".to_string()),
				Value::String("application/json".to_string()),
			);
			response_map.insert(
				crate::ast::MapKey::String("headers".to_string()),
				Value::Map(headers_map),
			);

			return Ok(Value::Map(response_map));
		}

		// For other capabilities, use the MicroVM provider
		let default_provider = "mock".to_string();
		let provider_name = self.microvm_provider.as_ref().unwrap_or(&default_provider);
        
		// Get the provider (should already be initialized from set_microvm_provider)
		let provider = self.microvm_factory.get_provider(provider_name)
			.ok_or_else(|| RuntimeError::Generic(format!("MicroVM provider '{}' not found", provider_name)))?;
        
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
    
	/// Execute a capability, delegating to provider if registered
	pub fn execute_capability_with_microvm(&self, capability_id: &str, args: Vec<Value>, runtime_context: Option<&RuntimeContext>) -> RuntimeResult<Value> {
		// If a provider is registered for this capability, delegate
		if let Some(provider) = self.providers.get(capability_id) {
			// Use default execution context for now
            let context = crate::ccos::capabilities::provider::ExecutionContext {
				trace_id: uuid::Uuid::new_v4().to_string(),
				timeout: std::time::Duration::from_secs(10),
			};
			return provider.execute_capability(capability_id, &Value::Vector(args), &context);
		}

		// Check if this capability requires MicroVM isolation
		let requires_microvm = matches!(capability_id, 
			"ccos.network.http-fetch" | 
			"ccos.io.open-file" | 
			"ccos.io.read-line" | 
			"ccos.io.write-line" | 
			"ccos.io.close-file" |
			"ccos.system.get-env"
		);

		if requires_microvm {
			self.execute_in_microvm(capability_id, args, runtime_context)
		} else {
			// For capabilities that don't require MicroVM, execute normally
			match self.get_capability(capability_id) {
				Some(capability) => (capability.func)(args),
				None => Err(RuntimeError::Generic(format!("Capability '{}' not found", capability_id))),
			}
		}
	}
    
	// System capability implementations
	fn get_env_capability(args: Vec<Value>) -> RuntimeResult<Value> {
		// TODO: Add permission checking, sandboxing
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
		// TODO: Add path validation, sandbox checking
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
    
	fn open_file_capability(_args: Vec<Value>) -> RuntimeResult<Value> {
		// This should be called through execute_in_microvm for proper isolation
		Err(RuntimeError::Generic(
			"File operations must be executed through MicroVM isolation. Use CapabilityRegistry::execute_in_microvm()".to_string(),
		))
	}
    
	fn read_line_capability(_args: Vec<Value>) -> RuntimeResult<Value> {
		// This should be called through execute_in_microvm for proper isolation
		Err(RuntimeError::Generic(
			"File operations must be executed through MicroVM isolation. Use CapabilityRegistry::execute_in_microvm()".to_string(),
		))
	}
    
	fn write_line_capability(_args: Vec<Value>) -> RuntimeResult<Value> {
		// This should be called through execute_in_microvm for proper isolation
		Err(RuntimeError::Generic(
			"File operations must be executed through MicroVM isolation. Use CapabilityRegistry::execute_in_microvm()".to_string(),
		))
	}
    
	fn close_file_capability(_args: Vec<Value>) -> RuntimeResult<Value> {
		// This should be called through execute_in_microvm for proper isolation
		Err(RuntimeError::Generic(
			"File operations must be executed through MicroVM isolation. Use CapabilityRegistry::execute_in_microvm()".to_string(),
		))
	}
    
	fn parse_json_capability(args: Vec<Value>) -> RuntimeResult<Value> {
		// TODO: Add input validation, size limits
		if args.len() != 1 {
			return Err(RuntimeError::ArityMismatch {
				function: "ccos.data.parse-json".to_string(),
				expected: "1".to_string(),
				actual: args.len(),
			});
		}
        
		match &args[0] {
			Value::String(json_str) => {
				// TODO: Add size limits, validation
				match serde_json::from_str::<serde_json::Value>(json_str) {
					Ok(json_value) => Ok(Self::json_value_to_rtfs_value(&json_value)),
					Err(e) => Err(RuntimeError::Generic(format!("JSON parsing error: {}", e))),
				}
			}
			_ => Err(RuntimeError::TypeError {
				expected: "string".to_string(),
				actual: args[0].type_name().to_string(),
				operation: "ccos.data.parse-json".to_string(),
			}),
		}
	}
    
	fn serialize_json_capability(args: Vec<Value>) -> RuntimeResult<Value> {
		// TODO: Add output size limits
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
		// TODO: Add log level checking, rate limiting
		let message = args
			.iter()
			.map(|v| format!("{:?}", v))
			.collect::<Vec<_>>()
			.join(" ");
		println!("[CCOS-LOG] {}", message);
		Ok(Value::Nil)
	}
    
	fn print_capability(args: Vec<Value>) -> RuntimeResult<Value> {
		// TODO: Add output rate limiting
		let message = args
			.iter()
			.map(|v| format!("{:?}", v))
			.collect::<Vec<_>>()
			.join(" ");
		print!("{}", message);
		Ok(Value::Nil)
	}
    
	fn println_capability(args: Vec<Value>) -> RuntimeResult<Value> {
		// TODO: Add output rate limiting
		let message = args
			.iter()
			.map(|v| format!("{:?}", v))
			.collect::<Vec<_>>()
			.join(" ");
		println!("{}", message);
		Ok(Value::Nil)
	}
    
	// Agent capability implementations
	fn discover_agents_capability(_args: Vec<Value>) -> RuntimeResult<Value> {
		// TODO: Implement with proper capability marketplace integration
		Ok(Value::Vector(vec![]))
	}
    
	fn task_coordination_capability(_args: Vec<Value>) -> RuntimeResult<Value> {
		// TODO: Implement with proper CCOS task coordination
		Ok(Value::Map(std::collections::HashMap::new()))
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
				// Display the prompt to the user
				print!("{}: ", prompt);
				std::io::Write::flush(&mut std::io::stdout()).map_err(|e| {
					RuntimeError::Generic(format!("Failed to flush stdout: {}", e))
				})?;
                
				// Read user input
				let mut input = String::new();
				std::io::stdin().read_line(&mut input).map_err(|e| {
					RuntimeError::Generic(format!("Failed to read user input: {}", e))
				})?;
                
				// Trim whitespace and return the input
				let user_response = input.trim().to_string();
				Ok(Value::String(user_response))
			},
			_ => {
				return Err(RuntimeError::TypeError {
					expected: "string".to_string(),
					actual: args[0].type_name().to_string(),
					operation: "ccos.agent.ask-human".to_string(),
				});
			}
		}
	}
    
	fn discover_and_assess_agents_capability(_args: Vec<Value>) -> RuntimeResult<Value> {
		// TODO: Implement with proper agent discovery system
		Ok(Value::Vector(vec![]))
	}
    
	fn establish_system_baseline_capability(_args: Vec<Value>) -> RuntimeResult<Value> {
		// TODO: Implement with proper system baseline establishment
		Ok(Value::Map(std::collections::HashMap::new()))
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
            #[cfg(feature = "legacy-atoms")]
            Value::Atom(_) => Ok(serde_json::Value::String("<atom>".to_string())),
            #[cfg(not(feature = "legacy-atoms"))]
            Value::Atom(_) => Ok(serde_json::Value::String("<legacy-atom>".to_string())),
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

impl Default for CapabilityRegistry {
	fn default() -> Self {
		Self::new()
	}
}
