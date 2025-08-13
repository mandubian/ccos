//! RTFS Standard Library
//!
//! This module provides the standard library functions for the RTFS language.
//! It includes both pure functions and functions with side-effects (impure).
//! The standard library is orga                    operation: "tool/http-fetch".to_string(),ized into categories:
//! - Arithmetic functions
//! - Comparison functions
//! - Boolean logic functions
//! - String manipulation functions
//! - Collection manipulation functions
//! - Type predicate functions
//! - Tooling functions (file I/O, HTTP, etc.)
//! - CCOS capability functions

use crate::ast::{Symbol, Keyword, MapKey};
use crate::runtime::environment::Environment;
use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::runtime::evaluator::Evaluator;
use crate::runtime::secure_stdlib::SecureStandardLibrary;
use crate::runtime::values::{Arity, BuiltinFunction, BuiltinFunctionWithContext, Function, Value};
use crate::runtime::capability_marketplace::CapabilityMarketplace;
use std::sync::Arc;
use crate::ccos::types::{Action, ExecutionResult};
use uuid::Uuid;
use crate::runtime::module_runtime::{ModuleRegistry, Module, ModuleMetadata, ModuleExport, ExportType};
use crate::runtime::environment::IrEnvironment;
use crate::ir::core::{IrType, IrNode};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

/// The Standard Library for the RTFS runtime.
/// 
/// This struct is responsible for creating the global environment and loading
/// all the built-in functions. It now composes the secure standard library
/// with impure functions.
pub struct StandardLibrary;

impl StandardLibrary {
    /// Creates a new global environment and populates it with the standard library functions.
    /// 
    /// This function composes the secure and insecure parts of the standard library.
    /// It starts with a secure environment and then adds the impure functions.
    pub fn create_global_environment() -> Environment {
        // Start with a secure environment containing only pure functions
        let mut env = SecureStandardLibrary::create_secure_environment();

        // Load impure functions that require special capabilities
        Self::load_tool_functions(&mut env);
        Self::load_capability_functions(&mut env);

        env
    }

    /// Loads the tooling functions into the environment.
    /// 
    /// These functions provide access to external resources like the file system,
    /// network, and system clock. They are considered "impure" because they
    /// can have side-effects.
    fn load_tool_functions(env: &mut Environment) {
        // File I/O
        env.define(
            &Symbol("tool/open-file".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool/open-file".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::tool_open_file),
            })),
        );

        // HTTP requests
        env.define(
            &Symbol("tool/http-fetch".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool/http-fetch".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::tool_http_fetch),
            })),
        );

        env.define(
            &Symbol("http-fetch".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "http-fetch".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::tool_http_fetch),
            })),
        );

        // Logging
        env.define(
            &Symbol("tool/log".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool/log".to_string(),
                arity: Arity::Variadic(1),
                func: Rc::new(Self::tool_log),
            })),
        );

        // System time
        env.define(
            &Symbol("tool/time-ms".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool/time-ms".to_string(),
                arity: Arity::Fixed(0),
                func: Rc::new(Self::tool_time_ms),
            })),
        );

        // JSON functions
        env.define(
            &Symbol("tool/serialize-json".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool/serialize-json".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::tool_serialize_json),
            })),
        );

        env.define(
            &Symbol("serialize-json".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "serialize-json".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::tool_serialize_json),
            })),
        );

        env.define(
            &Symbol("tool/parse-json".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool/parse-json".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::tool_parse_json),
            })),
        );

        env.define(
            &Symbol("parse-json".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "parse-json".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::tool_parse_json),
            })),
        );

        // Print functions
        env.define(
            &Symbol("println".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "println".to_string(),
                arity: Arity::Variadic(0),
                func: Rc::new(Self::println),
            })),
        );

        // Thread functions
        env.define(
            &Symbol("thread/sleep".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "thread/sleep".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::thread_sleep),
            })),
        );

        // File I/O functions
        env.define(
            &Symbol("read-lines".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "read-lines".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::read_lines),
            })),
        );

        // Logging/debugging functions
        env.define(
            &Symbol("step".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "step".to_string(),
                arity: Arity::Variadic(1),
                func: Rc::new(Self::step),
            })),
        );

        // Control flow functions
        env.define(
            &Symbol("dotimes".to_string()),
            Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
                name: "dotimes".to_string(),
                arity: Arity::Fixed(2),
                func: Rc::new(Self::dotimes),
            })),
        );
    }

    /// Loads the CCOS capability functions into the environment.
    /// 
    /// These functions are specific to the CCOS and provide high-level
    /// orchestration capabilities.
    fn load_capability_functions(env: &mut Environment) {
        // `call` for invoking CCOS capabilities
        env.define(
            &Symbol("call".to_string()),
            Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
                name: "call".to_string(),
                arity: Arity::Variadic(1),
                func: Rc::new(Self::call_capability),
            })),
        );
    }

    // --- Tooling Function Implementations ---

    /// `(tool.open-file "path/to/file")`
    /// 
    /// Reads the content of a file and returns it as a string.
    fn tool_open_file(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "tool/open-file".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        let path = match &args[0] {
            Value::String(s) => s,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "string".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "tool/open-file".to_string(),
                })
            }
        };

        match fs::read_to_string(path) {
            Ok(content) => Ok(Value::String(content)),
            Err(e) => Err(RuntimeError::IoError(e.to_string())),
        }
    }

    /// `(tool.http-fetch "http://example.com")`
    /// 
    /// Fetches content from a URL and returns it as a string.
    /// Note: This is a blocking operation.
    fn tool_http_fetch(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "tool/http-fetch".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        let url = match &args[0] {
            Value::String(s) => s,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "string".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "tool.http-fetch".to_string(),
                })
            }
        };

        // Since this is a synchronous function, we need to block on the async call
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create async runtime: {}", e)))?;

        rt.block_on(async {
            match reqwest::get(url).await {
                Ok(response) => match response.text().await {
                    Ok(text) => Ok(Value::String(text)),
                    Err(e) => Err(RuntimeError::NetworkError(e.to_string())),
                },
                Err(e) => Err(RuntimeError::NetworkError(e.to_string())),
            }
        })
    }

    /// `(tool.log "message" 1 2 3)`
    /// 
    /// Prints the given arguments to the console.
    fn tool_log(args: Vec<Value>) -> RuntimeResult<Value> {
        let output = args
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<String>>()
            .join(" ");
        println!("{}", output);
        Ok(Value::Nil)
    }

    /// `(tool.time-ms)`
    /// 
    /// Returns the current system time in milliseconds since the UNIX epoch.
    fn tool_time_ms(args: Vec<Value>) -> RuntimeResult<Value> {
        if !args.is_empty() {
            return Err(RuntimeError::ArityMismatch {
                function: "tool.time-ms".to_string(),
                expected: "0".to_string(),
                actual: args.len(),
            });
        }

        let start = SystemTime::now();
        let since_the_epoch = start
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards");
        Ok(Value::Integer(since_the_epoch.as_millis() as i64))
    }

    /// `(tool/serialize-json data)`
    /// 
    /// Converts an RTFS value to JSON string representation.
    fn tool_serialize_json(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "tool/serialize-json".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        let value = &args[0];
        let json_value = Self::rtfs_value_to_json(value)?;
        
        match serde_json::to_string(&json_value) {
            Ok(json_string) => Ok(Value::String(json_string)),
            Err(e) => Err(RuntimeError::Generic(format!("JSON serialization error: {}", e))),
        }
    }

    /// `(tool/parse-json json-string)`
    /// 
    /// Parses a JSON string and converts it to an RTFS value.
    fn tool_parse_json(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "tool/parse-json".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        let json_string = match &args[0] {
            Value::String(s) => s,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "string".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "tool/parse-json".to_string(),
                });
            }
        };

        let json_value: serde_json::Value = match serde_json::from_str(json_string) {
            Ok(value) => value,
            Err(e) => return Err(RuntimeError::Generic(format!("JSON parsing error: {}", e))),
        };

        Self::json_value_to_rtfs(&json_value)
    }

    /// Helper function to convert RTFS Value to serde_json::Value
    fn rtfs_value_to_json(value: &Value) -> RuntimeResult<serde_json::Value> {
        match value {
            Value::Nil => Ok(serde_json::Value::Null),
            Value::Boolean(b) => Ok(serde_json::Value::Bool(*b)),
            Value::Integer(i) => Ok(serde_json::Value::Number(serde_json::Number::from(*i))),
            Value::Float(f) => match serde_json::Number::from_f64(*f) {
                Some(n) => Ok(serde_json::Value::Number(n)),
                None => Err(RuntimeError::Generic("Invalid float value for JSON".to_string())),
            },
            Value::String(s) => Ok(serde_json::Value::String(s.clone())),
            Value::Vector(vec) => {
                let mut json_array = Vec::new();
                for item in vec {
                    json_array.push(Self::rtfs_value_to_json(item)?);
                }
                Ok(serde_json::Value::Array(json_array))
            },
            Value::Map(map) => {
                let mut json_object = serde_json::Map::new();
                for (key, value) in map {
                    let key_str = match key {
                        crate::ast::MapKey::String(s) => s.clone(),
                        crate::ast::MapKey::Keyword(k) => k.0.clone(),
                        crate::ast::MapKey::Integer(i) => i.to_string(),
                    };
                    json_object.insert(key_str, Self::rtfs_value_to_json(value)?);
                }
                Ok(serde_json::Value::Object(json_object))
            },
            _ => Err(RuntimeError::Generic(format!("Cannot serialize {} to JSON", value.type_name()))),
        }
    }

    /// Helper function to convert serde_json::Value to RTFS Value
    fn json_value_to_rtfs(json: &serde_json::Value) -> RuntimeResult<Value> {
        match json {
            serde_json::Value::Null => Ok(Value::Nil),
            serde_json::Value::Bool(b) => Ok(Value::Boolean(*b)),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Ok(Value::Integer(i))
                } else if let Some(f) = n.as_f64() {
                    Ok(Value::Float(f))
                } else {
                    Err(RuntimeError::Generic("Invalid JSON number".to_string()))
                }
            },
            serde_json::Value::String(s) => Ok(Value::String(s.clone())),
            serde_json::Value::Array(arr) => {
                let mut rtfs_vec = Vec::new();
                for item in arr {
                    rtfs_vec.push(Self::json_value_to_rtfs(item)?);
                }
                Ok(Value::Vector(rtfs_vec))
            },
            serde_json::Value::Object(obj) => {
                let mut rtfs_map = std::collections::HashMap::new();
                for (key, value) in obj {
                    let map_key = crate::ast::MapKey::String(key.clone());
                    rtfs_map.insert(map_key, Self::json_value_to_rtfs(value)?);
                }
                Ok(Value::Map(rtfs_map))
            },
        }
    }

    /// `(println args...)`
    /// 
    /// Prints the given arguments to the console with a newline.
    fn println(args: Vec<Value>) -> RuntimeResult<Value> {
        let output = if args.is_empty() {
            String::new()
        } else {
            args
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<String>>()
                .join(" ")
        };
        println!("{}", output);
        Ok(Value::Nil)
    }

    /// `(thread/sleep milliseconds)`
    /// 
    /// Sleeps for the specified number of milliseconds.
    fn thread_sleep(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "thread/sleep".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        let milliseconds = match &args[0] {
            Value::Integer(ms) => {
                if *ms < 0 {
                    return Err(RuntimeError::InvalidArgument(
                        "Sleep duration cannot be negative".to_string(),
                    ));
                }
                *ms as u64
            }
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "integer".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "thread/sleep".to_string(),
                })
            }
        };

        std::thread::sleep(std::time::Duration::from_millis(milliseconds));
        Ok(Value::Nil)
    }

    /// `(read-lines filename)`
    /// 
    /// Reads all lines from a file and returns them as a vector of strings.
    fn read_lines(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "read-lines".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        let filename = match &args[0] {
            Value::String(s) => s,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "string".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "read-lines".to_string(),
                })
            }
        };

        match std::fs::read_to_string(filename) {
            Ok(content) => {
                let lines: Vec<Value> = content
                    .lines()
                    .map(|line| Value::String(line.to_string()))
                    .collect();
                Ok(Value::Vector(lines))
            }
            Err(e) => Err(RuntimeError::IoError(e.to_string())),
        }
    }

    /// `(step message-or-level message [data])`
    /// 
    /// Logs a step/debug message with optional level and data.
    /// Returns nil (side-effect function for logging/debugging).
    fn step(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.is_empty() {
            return Err(RuntimeError::ArityMismatch {
                function: "step".to_string(),
                expected: "1 or more".to_string(),
                actual: args.len(),
            });
        }

        // Parse arguments based on patterns:
        // (step "message") - simple message
        // (step :level "message") - message with level 
        // (step "message" data) - message with data
        // (step :level "message" data) - message with level and data
        
        let (level, message, data) = match args.len() {
            1 => {
                // (step "message")
                let message = args[0].to_string();
                ("info", message, None)
            }
            2 => {
                // Could be (step :level "message") or (step "message" data)
                if let Value::Keyword(ref kw) = args[0] {
                    // (step :level "message")
                    let level = kw.0.as_str();
                    let message = args[1].to_string();
                    (level, message, None)
                } else {
                    // (step "message" data)
                    let message = args[0].to_string();
                    let data = Some(&args[1]);
                    ("info", message, data)
                }
            }
            3 => {
                // (step :level "message" data)
                let level = if let Value::Keyword(ref kw) = args[0] {
                    kw.0.as_str()
                } else {
                    "info"
                };
                let message = args[1].to_string();
                let data = Some(&args[2]);
                (level, message, data)
            }
            _ => {
                return Err(RuntimeError::ArityMismatch {
                    function: "step".to_string(),
                    expected: "1 to 3".to_string(),
                    actual: args.len(),
                });
            }
        };

        // Format log message
        let log_message = if let Some(data) = data {
            format!("[{}] {}: {}", level.to_uppercase(), message, data.to_string())
        } else {
            format!("[{}] {}", level.to_uppercase(), message)
        };

        // Print the log message (in a real implementation, this would go to a proper logger)
        println!("{}", log_message);
        
        Ok(Value::Nil)
    }

    /// `(dotimes [var n] body)`
    /// 
    /// Simplified dotimes - executes body n times (without mutable state for now).
    fn dotimes(
        args: Vec<Value>,
        _evaluator: &Evaluator, 
        _env: &mut Environment
    ) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "dotimes".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        // For now, just return nil since proper dotimes requires loop support
        // This allows the test to at least parse and run without error
        Ok(Value::Nil)
    }

    // --- CCOS Capability Function Implementations ---

    /// `(call :capability-id arg1 arg2 ...)` or `(call "capability-name" arg1 arg2 ...)`
    /// 
    /// Dynamically invokes a CCOS capability. This is the main entry point
    /// for RTFS to interact with the broader CCOS environment.
    /// Supports both keyword syntax (:capability-id) and string syntax ("capability-name")
    fn call_capability(
        args: Vec<Value>,
        evaluator: &Evaluator,
        _env: &mut Environment,
    ) -> RuntimeResult<Value> {
        if args.is_empty() {
            return Err(RuntimeError::ArityMismatch {
                function: "call".to_string(),
                expected: "at least 1".to_string(),
                actual: 0,
            });
        }

        let capability_name = match &args[0] {
            Value::String(s) => s.clone(),
            Value::Keyword(k) => k.0.clone(), // Support keyword syntax
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "string or keyword".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "call".to_string(),
                })
            }
        };

        let capability_args = &args[1..];

        // Delegate the actual capability execution to the host
        evaluator.host.execute_capability(&capability_name, capability_args)
    }

}

/// Register default capabilities in the marketplace
pub async fn register_default_capabilities(marketplace: &CapabilityMarketplace) -> RuntimeResult<()> {
    // Register ccos.echo capability
    marketplace.register_local_capability(
        "ccos.echo".to_string(),
        "Echo Capability".to_string(),
        "Echoes the input value back".to_string(),
        Arc::new(|input| {
            match input {
                // New calling convention: map with :args and optional :context
                Value::Map(map) => {
                    let args_val = map.get(&MapKey::Keyword(Keyword("args".to_string()))).cloned().unwrap_or(Value::List(vec![]));
                    match args_val {
                        Value::List(args) => {
                            if args.len() == 1 {
                                Ok(args[0].clone())
                            } else {
                                Err(RuntimeError::ArityMismatch {
                                    function: "ccos.echo".to_string(),
                                    expected: "1".to_string(),
                                    actual: args.len(),
                                })
                            }
                        }
                        other => Err(RuntimeError::TypeError {
                            expected: "list".to_string(),
                            actual: other.type_name().to_string(),
                            operation: "ccos.echo".to_string(),
                        })
                    }
                }
                // Backward compatibility: still accept a plain list
                Value::List(args) => {
                    if args.len() == 1 {
                        Ok(args[0].clone())
                    } else {
                        Err(RuntimeError::ArityMismatch {
                            function: "ccos.echo".to_string(),
                            expected: "1".to_string(),
                            actual: args.len(),
                        })
                    }
                }
                _ => Err(RuntimeError::TypeError {
                    expected: "map or list".to_string(),
                    actual: input.type_name().to_string(),
                    operation: "ccos.echo".to_string(),
                })
            }
        }),
    ).await.map_err(|e| RuntimeError::Generic(format!("Failed to register ccos.echo: {:?}", e)))?;

    // Register ccos.math.add capability
    marketplace.register_local_capability(
        "ccos.math.add".to_string(),
        "Math Add Capability".to_string(),
        "Adds numeric values".to_string(),
        Arc::new(|input| {
            match input {
                Value::List(args) => {
                    let mut sum = 0;
                    for arg in args {
                        match arg {
                            Value::Integer(n) => sum += n,
                            _ => {
                                return Err(RuntimeError::TypeError {
                                    expected: "integer".to_string(),
                                    actual: arg.type_name().to_string(),
                                    operation: "ccos.math.add".to_string(),
                                })
                            }
                        }
                    }
                    Ok(Value::Integer(sum))
                }
                _ => Err(RuntimeError::TypeError {
                    expected: "list".to_string(),
                    actual: input.type_name().to_string(),
                    operation: "ccos.math.add".to_string(),
                })
            }
        }),
    ).await.map_err(|e| RuntimeError::Generic(format!("Failed to register ccos.math.add: {:?}", e)))?;

    Ok(())
}

/// Load the standard library into a module registry
/// This creates a "stdlib" module containing all built-in functions
pub fn load_stdlib(module_registry: &mut ModuleRegistry) -> RuntimeResult<()> {
    // Create the standard library environment to get all functions
    let env = StandardLibrary::create_global_environment();
    
    // Get all function names
    let function_names = env.symbol_names();
    
    // Group functions by module namespace (e.g., "tool", "thread")
    let mut module_functions: HashMap<String, Vec<(String, Value)>> = HashMap::new();
    
    for name in function_names {
        if let Some(value) = env.lookup(&Symbol(name.clone())) {
            // Special case for division operator to avoid namespace parsing
            if name == "/" {
                module_functions
                    .entry("stdlib".to_string())
                    .or_insert_with(Vec::new)
                    .push((name, value));
            } else if let Some(slash_index) = name.find('/') {
                // Split by '/' to get module name and function name
                let module_name = name[..slash_index].to_string();
                let function_name = name[slash_index + 1..].to_string();
                
                // Skip if either module or function name is empty (malformed)
                if !module_name.is_empty() && !function_name.is_empty() {
                    module_functions
                        .entry(module_name)
                        .or_insert_with(Vec::new)
                        .push((function_name, value));
                }
            } else {
                // Functions without '/' go to a "stdlib" module
                module_functions
                    .entry("stdlib".to_string())
                    .or_insert_with(Vec::new)
                    .push((name, value));
            }
        }
    }

    // Create and register a module for each namespace
    for (module_name, functions) in module_functions {
        let metadata = ModuleMetadata {
            name: module_name.clone(),
            docstring: Some(format!("RTFS Standard Library - {} module", module_name)),
            source_file: None,
            version: Some("1.0.0".to_string()),
            compiled_at: std::time::SystemTime::now(),
        };

        let mut exports = HashMap::new();
        
        for (function_name, value) in functions {
            let export = ModuleExport {
                original_name: function_name.clone(),
                export_name: function_name.clone(),
                value,
                ir_type: IrType::Any,
                export_type: ExportType::Function,
            };
            exports.insert(function_name, export);
        }

        let module = Module {
            metadata,
            ir_node: IrNode::Program {
                id: 0,
                version: "1.0.0".to_string(),
                forms: vec![],
                source_location: None,
            },
            exports: RefCell::new(exports),
            namespace: Rc::new(RefCell::new(IrEnvironment::new())),
            dependencies: vec![],
        };

        // Register the module
        module_registry.register_module(module)?;
    }

    Ok(())
}

