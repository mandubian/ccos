//! RTFS Standard Library
//!
//! This module provides the standard library functions for the RTFS language.
//! It includes both pure functions and functions with side-effects (impure).
//! The standard library is organized into categories:
//! - Arithmetic functions
//! - Comparison functions
//! - Boolean logic functions
//! - String manipulation functions
//! - Collection manipulation functions
//! - Type predicate functions
//! - Tooling functions (file I/O, HTTP, etc.)
//! - CCOS capability functions

use crate::ast::Symbol;
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
            &Symbol("tool.open-file".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool.open-file".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::tool_open_file),
            })),
        );

        // HTTP requests
        env.define(
            &Symbol("tool.http-fetch".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool.http-fetch".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::tool_http_fetch),
            })),
        );

        // Logging
        env.define(
            &Symbol("tool.log".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool.log".to_string(),
                arity: Arity::Variadic(1),
                func: Rc::new(Self::tool_log),
            })),
        );

        // System time
        env.define(
            &Symbol("tool.time-ms".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool.time-ms".to_string(),
                arity: Arity::Fixed(0),
                func: Rc::new(Self::tool_time_ms),
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
                function: "tool.open-file".to_string(),
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
                    operation: "tool.open-file".to_string(),
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
                function: "tool.http-fetch".to_string(),
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

    // --- CCOS Capability Function Implementations ---

    /// `(call "capability.name" arg1 arg2 ...)`
    /// 
    /// Dynamically invokes a CCOS capability. This is the main entry point
    /// for RTFS to interact with the broader CCOS environment.
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
            Value::String(s) => s,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "string".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "call".to_string(),
                })
            }
        };

        let capability_args = &args[1..];

        // Delegate the actual capability execution to the host
        evaluator.host.execute_capability(capability_name, capability_args)
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
                    expected: "list".to_string(),
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
pub fn load_stdlib(module_registry: &ModuleRegistry) -> RuntimeResult<()> {
    // Create module metadata
    let metadata = ModuleMetadata {
        name: "stdlib".to_string(),
        docstring: Some("RTFS Standard Library - Built-in functions and tools".to_string()),
        source_file: None,
        version: Some("1.0.0".to_string()),
        compiled_at: std::time::SystemTime::now(),
    };

    // Create the standard library environment to get all functions
    let env = StandardLibrary::create_global_environment();

    // Create module exports by creating the standard library functions directly
    let mut exports = HashMap::new();
    
    // Helper function to add a function export
    let mut add_function_export = |name: &str, value: Value| {
        let export = ModuleExport {
            original_name: name.to_string(),
            export_name: name.to_string(),
            value,
            ir_type: IrType::Any,
            export_type: ExportType::Function,
        };
        exports.insert(name.to_string(), export);
    };

    // Get function names and recreate them for export
    // We'll directly add the known functions since we can't easily iterate over Environment bindings
    let function_names = env.symbol_names();
    for name in function_names {
        if let Some(value) = env.lookup(&Symbol(name.clone())) {
            add_function_export(&name, value);
        }
    }

    // Create the module
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

    Ok(())
}

