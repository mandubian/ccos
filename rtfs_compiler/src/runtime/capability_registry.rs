//! CCOS Capability Registry
//!
//! This module manages dangerous operations that require special permissions,
//! sandboxing, or delegation to secure execution environments.

use crate::runtime::values::{Value, Arity};
use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::runtime::capability::Capability;
use crate::ast::Keyword;
use std::collections::HashMap;
use std::rc::Rc;

/// Registry of CCOS capabilities that require special execution
pub struct CapabilityRegistry {
    capabilities: HashMap<String, Capability>,
}

impl CapabilityRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            capabilities: HashMap::new(),
        };
        
        // Register system capabilities
        registry.register_system_capabilities();
        registry.register_io_capabilities();
        registry.register_network_capabilities();
        registry.register_agent_capabilities();
        
        registry
    }
    
    fn register_system_capabilities(&mut self) {
        // Environment access capability
        self.capabilities.insert(
            "ccos.system.get-env".to_string(),
            Capability {
                id: "ccos.system.get-env".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(|args| Self::get_env_capability(args)),
            },
        );
        
        // Time access capability
        self.capabilities.insert(
            "ccos.system.current-time".to_string(),
            Capability {
                id: "ccos.system.current-time".to_string(),
                arity: Arity::Fixed(0),
                func: Rc::new(|args| Self::current_time_capability(args)),
            },
        );
        
        // Timestamp capability
        self.capabilities.insert(
            "ccos.system.current-timestamp-ms".to_string(),
            Capability {
                id: "ccos.system.current-timestamp-ms".to_string(),
                arity: Arity::Fixed(0),
                func: Rc::new(|args| Self::current_timestamp_ms_capability(args)),
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
                func: Rc::new(|args| Self::file_exists_capability(args)),
            },
        );
        
        self.capabilities.insert(
            "ccos.io.open-file".to_string(),
            Capability {
                id: "ccos.io.open-file".to_string(),
                arity: Arity::Variadic(1),
                func: Rc::new(|args| Self::open_file_capability(args)),
            },
        );
        
        self.capabilities.insert(
            "ccos.io.read-line".to_string(),
            Capability {
                id: "ccos.io.read-line".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(|args| Self::read_line_capability(args)),
            },
        );
        
        self.capabilities.insert(
            "ccos.io.write-line".to_string(),
            Capability {
                id: "ccos.io.write-line".to_string(),
                arity: Arity::Fixed(2),
                func: Rc::new(|args| Self::write_line_capability(args)),
            },
        );
        
        self.capabilities.insert(
            "ccos.io.close-file".to_string(),
            Capability {
                id: "ccos.io.close-file".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(|args| Self::close_file_capability(args)),
            },
        );
        
        // JSON operations
        self.capabilities.insert(
            "ccos.data.parse-json".to_string(),
            Capability {
                id: "ccos.data.parse-json".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(|args| Self::parse_json_capability(args)),
            },
        );
        
        self.capabilities.insert(
            "ccos.data.serialize-json".to_string(),
            Capability {
                id: "ccos.data.serialize-json".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(|args| Self::serialize_json_capability(args)),
            },
        );
        
        // Logging capabilities
        self.capabilities.insert(
            "ccos.io.log".to_string(),
            Capability {
                id: "ccos.io.log".to_string(),
                arity: Arity::Variadic(1),
                func: Rc::new(|args| Self::log_capability(args)),
            },
        );
        
        self.capabilities.insert(
            "ccos.io.print".to_string(),
            Capability {
                id: "ccos.io.print".to_string(),
                arity: Arity::Variadic(1),
                func: Rc::new(|args| Self::print_capability(args)),
            },
        );
        
        self.capabilities.insert(
            "ccos.io.println".to_string(),
            Capability {
                id: "ccos.io.println".to_string(),
                arity: Arity::Variadic(1),
                func: Rc::new(|args| Self::println_capability(args)),
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
                func: Rc::new(|args| Self::http_fetch_capability(args)),
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
                func: Rc::new(|args| Self::discover_agents_capability(args)),
            },
        );
        
        self.capabilities.insert(
            "ccos.agent.task-coordination".to_string(),
            Capability {
                id: "ccos.agent.task-coordination".to_string(),
                arity: Arity::Variadic(0),
                func: Rc::new(|args| Self::task_coordination_capability(args)),
            },
        );
        
        self.capabilities.insert(
            "ccos.agent.ask-human".to_string(),
            Capability {
                id: "ccos.agent.ask-human".to_string(),
                arity: Arity::Variadic(1),
                func: Rc::new(|args| Self::ask_human_capability(args)),
            },
        );
        
        self.capabilities.insert(
            "ccos.agent.discover-and-assess-agents".to_string(),
            Capability {
                id: "ccos.agent.discover-and-assess-agents".to_string(),
                arity: Arity::Variadic(0),
                func: Rc::new(|args| Self::discover_and_assess_agents_capability(args)),
            },
        );
        
        self.capabilities.insert(
            "ccos.agent.establish-system-baseline".to_string(),
            Capability {
                id: "ccos.agent.establish-system-baseline".to_string(),
                arity: Arity::Variadic(0),
                func: Rc::new(|args| Self::establish_system_baseline_capability(args)),
            },
        );
    }
    
    pub fn get_capability(&self, id: &str) -> Option<&Capability> {
        self.capabilities.get(id)
    }
    
    pub fn list_capabilities(&self) -> Vec<&str> {
        self.capabilities.keys().map(|k| k.as_str()).collect()
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
        // TODO: Implement with proper sandboxing
        Err(RuntimeError::Generic(
            "File operations require secure microVM execution".to_string(),
        ))
    }
    
    fn read_line_capability(_args: Vec<Value>) -> RuntimeResult<Value> {
        // TODO: Implement with proper sandboxing
        Err(RuntimeError::Generic(
            "File operations require secure microVM execution".to_string(),
        ))
    }
    
    fn write_line_capability(_args: Vec<Value>) -> RuntimeResult<Value> {
        // TODO: Implement with proper sandboxing
        Err(RuntimeError::Generic(
            "File operations require secure microVM execution".to_string(),
        ))
    }
    
    fn close_file_capability(_args: Vec<Value>) -> RuntimeResult<Value> {
        // TODO: Implement with proper sandboxing
        Err(RuntimeError::Generic(
            "File operations require secure microVM execution".to_string(),
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
    
    // Network capability implementations
    fn http_fetch_capability(_args: Vec<Value>) -> RuntimeResult<Value> {
        // TODO: Implement with proper sandboxing, URL validation, rate limiting
        Err(RuntimeError::Generic(
            "Network operations require secure microVM execution".to_string(),
        ))
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
            Value::String(_s) => {
                // Generate ticket id
                let ticket_id = format!("prompt-{}", uuid::Uuid::new_v4());
                // TODO: Integrate with Arbiter::issue_user_prompt
                Ok(Value::ResourceHandle(ticket_id))
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
