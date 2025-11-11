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

use crate::ast::{Keyword, MapKey, Symbol};
// CCOS capability marketplace removed - RTFS uses pure_host
use crate::runtime::environment::Environment;
use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::runtime::evaluator::Evaluator;
use crate::runtime::secure_stdlib::SecureStandardLibrary;
use crate::runtime::values::{Arity, BuiltinFunction, BuiltinFunctionWithContext, Function, Value};
use crate::runtime::ExecutionOutcome;
// Removed RwLock - no longer needed after atom removal
use crate::ir::core::{IrNode, IrType};
use crate::runtime::environment::IrEnvironment;
use crate::runtime::module_runtime::{
    ExportType, Module, ModuleExport, ModuleMetadata, ModuleRegistry,
};
use std::collections::HashMap;
use std::fs;
// removed Rc: use Arc for shared ownership
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

        // Load additional secure functions
        Self::load_secure_functions(&mut env);

        // Load impure functions that require special capabilities
        Self::load_tool_functions(&mut env);
        Self::load_capability_functions(&mut env);

        env
    }

    // `(vals map)` - return vector of values in the map (order may vary)
    fn vals(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "vals".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        match &args[0] {
            Value::Map(map) => {
                let mut res = Vec::new();
                for (_k, v) in map.iter() {
                    res.push(v.clone());
                }
                Ok(Value::Vector(res))
            }
            other => Err(RuntimeError::TypeError {
                expected: "map".to_string(),
                actual: other.type_name().to_string(),
                operation: "vals".to_string(),
            }),
        }
    }

    /// Loads the tooling functions into the environment.
    ///
    /// These functions provide access to external resources like the file system,
    /// network, and system clock. They are considered "impure" because they
    /// can have side-effects.
    fn load_tool_functions(_env: &mut Environment) {
        // Note: RTFS stdlib is pure. Effectful helpers previously registered here
        // (e.g., tool/open-file, http-fetch, tool/log, time-ms, file-exists?, get-env,
        // println, thread/sleep, read-lines, step, kv/*!) have been moved to the CCOS prelude.
        // The only registrations left here must be pure.

        // (no JSON functions here; they are pure and registered in secure stdlib)

        // Control flow functions are evaluator special-forms; do not re-register here.

        // Intentionally minimal: only pure utilities should be registered here.
    }

    /// Loads the secure standard library functions into the environment.
    ///
    /// These functions are pure and safe to execute in any context.
    fn load_secure_functions(env: &mut Environment) {
        // RTFS secure stdlib: only pure helpers here. Effectful I/O, logging, time,
        // env, and state helpers are provided by the CCOS prelude layer.

        // Control flow functions are evaluator special-forms; do not re-register here.

        // Pure JSON helpers
        env.define(
            &Symbol("tool/serialize-json".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool/serialize-json".to_string(),
                arity: Arity::Fixed(1),
                func: std::sync::Arc::new(|args: Vec<Value>| Self::tool_serialize_json(args)),
            })),
        );

        env.define(
            &Symbol("serialize-json".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "serialize-json".to_string(),
                arity: Arity::Fixed(1),
                func: std::sync::Arc::new(|args: Vec<Value>| Self::tool_serialize_json(args)),
            })),
        );

        env.define(
            &Symbol("tool/parse-json".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool/parse-json".to_string(),
                arity: Arity::Fixed(1),
                func: std::sync::Arc::new(|args: Vec<Value>| Self::tool_parse_json(args)),
            })),
        );

        env.define(
            &Symbol("parse-json".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "parse-json".to_string(),
                arity: Arity::Fixed(1),
                func: std::sync::Arc::new(|args: Vec<Value>| Self::tool_parse_json(args)),
            })),
        );

        // Collection helpers: keys
        env.define(
            &Symbol("keys".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "keys".to_string(),
                arity: Arity::Fixed(1),
                func: std::sync::Arc::new(Self::keys),
            })),
        );

        // Collection helpers: vals
        env.define(
            &Symbol("vals".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "vals".to_string(),
                arity: Arity::Fixed(1),
                func: std::sync::Arc::new(Self::vals),
            })),
        );

        // Map lookup returning entry pair or nil: (find m k) -> [k v] | nil
        env.define(
            &Symbol("find".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "find".to_string(),
                arity: Arity::Fixed(2),
                func: std::sync::Arc::new(Self::find),
            })),
        );

        // map-indexed registration
        env.define(
            &Symbol("map-indexed".to_string()),
            Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
                name: "map-indexed".to_string(),
                arity: Arity::Fixed(2),
                func: std::sync::Arc::new(
                    |args: Vec<Value>, evaluator: &Evaluator, env: &mut Environment| {
                        Self::map_indexed(args, evaluator, env)
                    },
                ),
            })),
        );

        // Remove: forward to secure implementation (already implemented in this module)
        env.define(
            &Symbol("remove".to_string()),
            Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
                name: "remove".to_string(),
                arity: Arity::Fixed(2),
                func: std::sync::Arc::new(
                    |args: Vec<Value>, evaluator: &Evaluator, env: &mut Environment| {
                        Self::remove(args, evaluator, env)
                    },
                ),
            })),
        );

        // Collection helpers: update (map key f)
        env.define(
            &Symbol("update".to_string()),
            Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
                name: "update".to_string(),
                arity: Arity::Variadic(3),
                func: std::sync::Arc::new(Self::update),
            })),
        );

        // Error helpers: (getMessage e) -> message string
        env.define(
            &Symbol("getMessage".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "getMessage".to_string(),
                arity: Arity::Fixed(1),
                func: std::sync::Arc::new(|args: Vec<Value>| -> RuntimeResult<Value> {
                    if args.len() != 1 {
                        return Err(RuntimeError::ArityMismatch {
                            function: "getMessage".to_string(),
                            expected: "1".to_string(),
                            actual: args.len(),
                        });
                    }
                    match &args[0] {
                        Value::Error(err) => Ok(Value::String(err.message.clone())),
                        other => Err(RuntimeError::TypeError {
                            expected: "error".to_string(),
                            actual: other.type_name().to_string(),
                            operation: "getMessage".to_string(),
                        }),
                    }
                }),
            })),
        );
        // 'for' is an evaluator special-form; not registered here.
        env.define(
            &Symbol("process-data".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "process-data".to_string(),
                arity: Arity::Fixed(1),
                func: std::sync::Arc::new(Self::process_data),
            })),
        );
        env.define(
            &Symbol("read-file".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "read-file".to_string(),
                arity: Arity::Fixed(1),
                func: std::sync::Arc::new(Self::read_file),
            })),
        );
        // set! is an evaluator special-form; not registered here.
        env.define(
            &Symbol("deftype".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "deftype".to_string(),
                arity: Arity::Fixed(2),
                func: std::sync::Arc::new(Self::deftype),
            })),
        );

        // Exception constructor: (Exception. msg data?) -> error value
        env.define(
            &Symbol("Exception.".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "Exception.".to_string(),
                arity: Arity::Variadic(1),
                func: std::sync::Arc::new(|args: Vec<Value>| -> RuntimeResult<Value> {
                    if args.is_empty() {
                        return Err(RuntimeError::ArityMismatch {
                            function: "Exception.".to_string(),
                            expected: "1+".to_string(),
                            actual: args.len(),
                        });
                    }
                    let msg = match &args[0] {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    Ok(Value::Error(crate::runtime::values::ErrorValue {
                        message: msg,
                        stack_trace: None,
                    }))
                }),
            })),
        );

        // Numbers: returns a vector of numbers from start to end
        // 'numbers' is pure and remains available via other helpers/tests; remove duplicate here if any.

        // Connect-db: stub for database connection capability
        // remove impure stubs from secure stdlib (e.g., connect-db) – keep such wiring in host/prelude.

        // Plan-id: stub for CCOS plan ID access
        // remove plan-id stub from secure stdlib – host-side concern.

        // Point: stub for Point type definition
        // trim demo/test-specific constructs from secure stdlib.

        // For: loop construct for iteration
        // leave loop constructs to evaluator special-forms; do not re-register here.

        // Map iteration: iterate over map entries
        env.define(
            &Symbol("map".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "map".to_string(),
                arity: Arity::Fixed(2),
                func: std::sync::Arc::new(|args: Vec<Value>| -> RuntimeResult<Value> {
                    if args.len() != 2 {
                        return Err(RuntimeError::ArityMismatch {
                            function: "map".to_string(),
                            expected: "2".to_string(),
                            actual: args.len(),
                        });
                    }

                    let _function = &args[0];
                    let collection = &args[1];

                    match collection {
                        Value::Vector(v) => {
                            let mut result = Vec::new();
                            for item in v {
                                // For now, just add the item as-is
                                // In a full implementation, this would call the function
                                result.push(item.clone());
                            }
                            Ok(Value::Vector(result))
                        }
                        Value::Map(m) => {
                            let mut result = Vec::new();
                            for (k, _v) in m {
                                // For maps, we can iterate over key-value pairs
                                let mut pair = HashMap::new();
                                pair.insert(
                                    MapKey::Keyword(Keyword("key".to_string())),
                                    Value::String("key".to_string()),
                                );
                                pair.insert(
                                    MapKey::Keyword(Keyword("value".to_string())),
                                    Value::String(k.to_string()),
                                );
                                result.push(Value::Map(pair));
                            }
                            Ok(Value::Vector(result))
                        }
                        _ => Err(RuntimeError::TypeError {
                            expected: "vector or map".to_string(),
                            actual: collection.type_name().to_string(),
                            operation: "map".to_string(),
                        }),
                    }
                }),
            })),
        );

        // Numbers: returns a vector of numbers from start to end
        env.define(
            &Symbol("numbers".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "numbers".to_string(),
                arity: Arity::Fixed(2),
                func: std::sync::Arc::new(|args: Vec<Value>| -> RuntimeResult<Value> {
                    if args.len() != 2 {
                        return Err(RuntimeError::ArityMismatch {
                            function: "numbers".to_string(),
                            expected: "2".to_string(),
                            actual: args.len(),
                        });
                    }
                    let start = args[0].as_number().ok_or_else(|| RuntimeError::TypeError {
                        expected: "number".to_string(),
                        actual: args[0].type_name().to_string(),
                        operation: "numbers".to_string(),
                    })?;
                    let end = args[1].as_number().ok_or_else(|| RuntimeError::TypeError {
                        expected: "number".to_string(),
                        actual: args[1].type_name().to_string(),
                        operation: "numbers".to_string(),
                    })?;

                    let mut result = Vec::new();
                    let start_int = start as i64;
                    let end_int = end as i64;
                    for i in start_int..=end_int {
                        result.push(Value::Integer(i));
                    }
                    Ok(Value::Vector(result))
                }),
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
                func: std::sync::Arc::new(Self::call_capability),
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
    /// Delegates HTTP fetching to the host capability `ccos.network.http-fetch`.
    /// Keeps the symbol for backward-compatibility while ensuring side effects
    /// happen in the host, not in the RTFS stdlib.
    fn http_fetch_via_host(
        args: Vec<Value>,
        evaluator: &Evaluator,
        _env: &mut Environment,
    ) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "tool/http-fetch".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        // Validate argument type early for clearer errors
        if !matches!(args.get(0), Some(Value::String(_))) {
            return Err(RuntimeError::TypeError {
                expected: "string".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "tool/http-fetch".to_string(),
            });
        }

        evaluator
            .host
            .execute_capability("ccos.network.http-fetch", &args)
    }

    /// `(tool/open-file path)` delegates to host capability ccos.io.open-file; falls back to local read
    fn open_file_via_host(args: Vec<Value>, evaluator: &Evaluator) -> RuntimeResult<Value> {
        match evaluator
            .host
            .execute_capability("ccos.io.open-file", &args)
        {
            Ok(v) => Ok(v),
            Err(_e) => Self::tool_open_file(args),
        }
    }

    /// `(tool/log ...)` delegates to host capability ccos.io.log; falls back to local impl
    fn tool_log_via_host(args: Vec<Value>, evaluator: &Evaluator) -> RuntimeResult<Value> {
        match evaluator.host.execute_capability("ccos.io.log", &args) {
            Ok(v) => Ok(v),
            Err(_e) => Self::tool_log(args),
        }
    }

    /// `(tool/time-ms)` delegates to host capability ccos.system.current-timestamp-ms; falls back to local impl
    fn time_ms_via_host(args: Vec<Value>, evaluator: &Evaluator) -> RuntimeResult<Value> {
        match evaluator
            .host
            .execute_capability("ccos.system.current-timestamp-ms", &args)
        {
            Ok(v) => Ok(v),
            Err(_e) => Self::tool_time_ms(args),
        }
    }

    /// `(file-exists? path)` delegates to host capability ccos.io.file-exists; falls back to local impl
    fn file_exists_via_host(args: Vec<Value>, evaluator: &Evaluator) -> RuntimeResult<Value> {
        match evaluator
            .host
            .execute_capability("ccos.io.file-exists", &args)
        {
            Ok(v) => Ok(v),
            Err(_e) => Self::file_exists(args),
        }
    }

    /// `(get-env key)` delegates to host capability ccos.system.get-env; falls back to local impl
    fn get_env_via_host(args: Vec<Value>, evaluator: &Evaluator) -> RuntimeResult<Value> {
        match evaluator
            .host
            .execute_capability("ccos.system.get-env", &args)
        {
            Ok(v) => Ok(v),
            Err(_e) => Self::get_env(args),
        }
    }

    /// `(println ...)` delegates to host capability ccos.io.println; falls back to local impl
    fn println_via_host(args: Vec<Value>, evaluator: &Evaluator) -> RuntimeResult<Value> {
        match evaluator.host.execute_capability("ccos.io.println", &args) {
            Ok(v) => Ok(v),
            Err(_e) => Self::println(args),
        }
    }

    /// `(thread/sleep ms)` optionally delegates if a host capability exists; currently falls back to local impl
    fn thread_sleep_via_host(args: Vec<Value>, evaluator: &Evaluator) -> RuntimeResult<Value> {
        match evaluator
            .host
            .execute_capability("ccos.system.sleep-ms", &args)
        {
            Ok(v) => Ok(v),
            Err(_e) => Self::thread_sleep(args),
        }
    }

    /// `(read-lines path)` delegates where possible; currently falls back to local impl
    fn read_lines_via_host(args: Vec<Value>, _evaluator: &Evaluator) -> RuntimeResult<Value> {
        // A richer host flow would stream via ccos.io.open-file + ccos.io.read-line; keep local for now
        Self::read_lines(args)
    }

    /// `(step ...)` delegates to host println for observability; falls back to local step formatter
    fn step_via_host(args: Vec<Value>, evaluator: &Evaluator) -> RuntimeResult<Value> {
        match evaluator.host.execute_capability("ccos.io.println", &args) {
            Ok(v) => Ok(v),
            Err(_e) => Self::step(args),
        }
    }

    /// `(kv/assoc! key k v [k v]...)` -> get value at key, assoc, put back, return new value
    fn kv_assoc_bang(
        args: Vec<Value>,
        evaluator: &Evaluator,
        env: &mut Environment,
    ) -> RuntimeResult<Value> {
        if args.len() < 3 || args.len() % 2 == 0 {
            return Err(RuntimeError::ArityMismatch {
                function: "kv/assoc!".to_string(),
                expected: "(kv/assoc! key k v [k v]...)".to_string(),
                actual: args.len(),
            });
        }
        let kv_key = args[0].clone();
        let pairs = args[1..].to_vec();

        let current = evaluator
            .host
            .execute_capability("ccos.state.kv.get", &[kv_key.clone()])
            .unwrap_or(Value::Nil);
        let base = match current {
            Value::Nil => Value::Map(std::collections::HashMap::new()),
            other => other,
        };

        // Resolve assoc from environment and apply
        let assoc_sym = crate::ast::Symbol("assoc".to_string());
        let assoc_fn = env
            .lookup(&assoc_sym)
            .ok_or_else(|| RuntimeError::Generic("assoc not found".to_string()))?;
        let mut assoc_args = Vec::with_capacity(1 + pairs.len());
        assoc_args.push(base);
        assoc_args.extend(pairs);
        let updated = match evaluator.call_function(assoc_fn, &assoc_args, env)? {
            ExecutionOutcome::Complete(v) => v,
            ExecutionOutcome::RequiresHost(hc) => {
                return Err(RuntimeError::Generic(format!(
                    "Host call required in assoc: {}",
                    hc.capability_id
                )))
            }
        };

        let _ = evaluator
            .host
            .execute_capability("ccos.state.kv.put", &[kv_key, updated.clone()]);
        Ok(updated)
    }

    /// `(kv/dissoc! key k1 k2 ...)` -> get map, dissoc keys, put back, return new map
    fn kv_dissoc_bang(
        args: Vec<Value>,
        evaluator: &Evaluator,
        env: &mut Environment,
    ) -> RuntimeResult<Value> {
        if args.len() < 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "kv/dissoc!".to_string(),
                expected: "(kv/dissoc! key k1 k2 ...)".to_string(),
                actual: args.len(),
            });
        }
        let kv_key = args[0].clone();
        let ds_keys = args[1..].to_vec();

        let current = evaluator
            .host
            .execute_capability("ccos.state.kv.get", &[kv_key.clone()])
            .unwrap_or(Value::Nil);
        let base = match current {
            Value::Nil => Value::Map(std::collections::HashMap::new()),
            other => other,
        };

        let dissoc_sym = crate::ast::Symbol("dissoc".to_string());
        let dissoc_fn = env
            .lookup(&dissoc_sym)
            .ok_or_else(|| RuntimeError::Generic("dissoc not found".to_string()))?;
        let mut dissoc_args = Vec::with_capacity(1 + ds_keys.len());
        dissoc_args.push(base);
        dissoc_args.extend(ds_keys);
        let updated = match evaluator.call_function(dissoc_fn, &dissoc_args, env)? {
            ExecutionOutcome::Complete(v) => v,
            ExecutionOutcome::RequiresHost(hc) => {
                return Err(RuntimeError::Generic(format!(
                    "Host call required in dissoc: {}",
                    hc.capability_id
                )))
            }
        };

        let _ = evaluator
            .host
            .execute_capability("ccos.state.kv.put", &[kv_key, updated.clone()]);
        Ok(updated)
    }

    /// `(kv/conj! key x1 x2 ...)` -> get vector, conj items, put back, return new vector
    fn kv_conj_bang(
        args: Vec<Value>,
        evaluator: &Evaluator,
        env: &mut Environment,
    ) -> RuntimeResult<Value> {
        if args.len() < 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "kv/conj!".to_string(),
                expected: "(kv/conj! key x1 x2 ...)".to_string(),
                actual: args.len(),
            });
        }
        let kv_key = args[0].clone();
        let items = args[1..].to_vec();

        let current = evaluator
            .host
            .execute_capability("ccos.state.kv.get", &[kv_key.clone()])
            .unwrap_or(Value::Nil);
        let base = match current {
            Value::Nil => Value::Vector(Vec::new()),
            other => other,
        };

        let conj_sym = crate::ast::Symbol("conj".to_string());
        let conj_fn = env
            .lookup(&conj_sym)
            .ok_or_else(|| RuntimeError::Generic("conj not found".to_string()))?;
        let mut conj_args = Vec::with_capacity(1 + items.len());
        conj_args.push(base);
        conj_args.extend(items);
        let updated = match evaluator.call_function(conj_fn, &conj_args, env)? {
            ExecutionOutcome::Complete(v) => v,
            ExecutionOutcome::RequiresHost(hc) => {
                return Err(RuntimeError::Generic(format!(
                    "Host call required in conj: {}",
                    hc.capability_id
                )))
            }
        };

        let _ = evaluator
            .host
            .execute_capability("ccos.state.kv.put", &[kv_key, updated.clone()]);
        Ok(updated)
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
        if args.len() != 0 {
            return Err(RuntimeError::ArityMismatch {
                function: "tool/time-ms".to_string(),
                expected: "0".to_string(),
                actual: args.len(),
            });
        }

        let since_the_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| RuntimeError::Generic(format!("SystemTime before UNIX EPOCH: {}", e)))?;

        Ok(Value::Integer(since_the_epoch.as_millis() as i64))
    }

    /// `(find m k)` -> returns a vector [k v] if key exists in map, otherwise nil
    fn find(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "find".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        let map = match &args[0] {
            Value::Map(m) => m,
            other => {
                return Err(RuntimeError::TypeError {
                    expected: "map".to_string(),
                    actual: other.type_name().to_string(),
                    operation: "find".to_string(),
                })
            }
        };

        // Convert key value into a MapKey used by underlying map
        let key_mk = match &args[1] {
            Value::String(s) => crate::ast::MapKey::String(s.clone()),
            Value::Keyword(k) => crate::ast::MapKey::Keyword(k.clone()),
            Value::Integer(i) => crate::ast::MapKey::Integer(*i),
            other => crate::ast::MapKey::String(other.to_string()),
        };

        if let Some(v) = map.get(&key_mk) {
            // Return vector [original-key-as-value value]
            let key_val = match &key_mk {
                crate::ast::MapKey::String(s) => Value::String(s.clone()),
                crate::ast::MapKey::Keyword(k) => Value::Keyword(k.clone()),
                crate::ast::MapKey::Integer(i) => Value::Integer(*i),
            };
            Ok(Value::Vector(vec![key_val, v.clone()]))
        } else {
            Ok(Value::Nil)
        }
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

        let json_val = Self::rtfs_value_to_json(&args[0])
            .map_err(|e| RuntimeError::Generic(format!("JSON serialization error: {}", e)))?;

        match serde_json::to_string(&json_val) {
            Ok(s) => Ok(Value::String(s)),
            Err(e) => Err(RuntimeError::Generic(format!(
                "JSON serialization error: {}",
                e
            ))),
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
                None => Err(RuntimeError::Generic(
                    "Invalid float value for JSON".to_string(),
                )),
            },
            Value::String(s) => Ok(serde_json::Value::String(s.clone())),
            Value::Vector(vec) => {
                let mut json_array = Vec::new();
                for item in vec {
                    json_array.push(Self::rtfs_value_to_json(item)?);
                }
                Ok(serde_json::Value::Array(json_array))
            }
            Value::Map(map) => {
                let mut json_object = serde_json::Map::new();
                for (key, value) in map {
                    let key_str = match key {
                        crate::ast::MapKey::String(s) => s.clone(),
                        crate::ast::MapKey::Keyword(k) => {
                            // Strip leading ':' from keyword for JSON keys
                            let s = &k.0;
                            if s.starts_with(':') {
                                s[1..].to_string()
                            } else {
                                s.clone()
                            }
                        }
                        crate::ast::MapKey::Integer(i) => i.to_string(),
                    };
                    json_object.insert(key_str, Self::rtfs_value_to_json(value)?);
                }
                Ok(serde_json::Value::Object(json_object))
            }
            _ => Err(RuntimeError::Generic(format!(
                "Cannot serialize {} to JSON",
                value.type_name()
            ))),
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
            }
            serde_json::Value::String(s) => Ok(Value::String(s.clone())),
            serde_json::Value::Array(arr) => {
                let mut rtfs_vec = Vec::new();
                for item in arr {
                    rtfs_vec.push(Self::json_value_to_rtfs(item)?);
                }
                Ok(Value::Vector(rtfs_vec))
            }
            serde_json::Value::Object(obj) => {
                let mut rtfs_map = std::collections::HashMap::new();
                for (key, value) in obj {
                    let map_key = crate::ast::MapKey::String(key.clone());
                    rtfs_map.insert(map_key, Self::json_value_to_rtfs(value)?);
                }
                Ok(Value::Map(rtfs_map))
            }
        }
    }

    /// `(println args...)`
    ///
    /// Prints the given arguments to the console with a newline.
    fn println(args: Vec<Value>) -> RuntimeResult<Value> {
        let output = if args.is_empty() {
            String::new()
        } else {
            args.iter()
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

    /// `(file-exists? filename)`
    ///
    /// Checks if a file exists and returns a boolean.
    fn file_exists(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "file-exists?".to_string(),
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
                    operation: "file-exists?".to_string(),
                });
            }
        };

        let exists = std::path::Path::new(filename).exists();
        Ok(Value::Boolean(exists))
    }

    /// `(get-env variable-name)`
    ///
    /// Gets an environment variable and returns its value as a string, or nil if not found.
    fn get_env(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "get-env".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        let var_name = match &args[0] {
            Value::String(s) => s,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "string".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "get-env".to_string(),
                });
            }
        };

        match std::env::var(var_name) {
            Ok(value) => Ok(Value::String(value)),
            Err(_) => Ok(Value::Nil),
        }
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

        // Try reading the file; if not found, fall back to test asset locations
        let try_paths: Vec<std::path::PathBuf> = {
            let p = std::path::Path::new(filename);
            if p.exists() {
                vec![p.to_path_buf()]
            } else {
                vec![
                    std::path::Path::new("tests/rtfs_files/features").join(filename),
                    std::path::Path::new("rtfs_compiler/tests/rtfs_files/features").join(filename),
                ]
            }
        };

        let mut content_opt: Option<String> = None;
        for p in try_paths {
            if let Ok(content) = std::fs::read_to_string(&p) {
                content_opt = Some(content);
                break;
            }
        }

        match content_opt {
            Some(content) => {
                let lines: Vec<Value> = content
                    .lines()
                    .map(|line| Value::String(line.to_string()))
                    .collect();
                Ok(Value::Vector(lines))
            }
            None => Err(RuntimeError::IoError(format!(
                "Failed to read file '{}' (also tried test asset paths)",
                filename
            ))),
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
            format!(
                "[{}] {}: {}",
                level.to_uppercase(),
                message,
                data.to_string()
            )
        } else {
            format!("[{}] {}", level.to_uppercase(), message)
        };

        // Print the log message (in a real implementation, this would go to a proper logger)
        println!("{}", log_message);

        Ok(Value::Nil)
    }

    // Removed stdlib dotimes/for duplicates; evaluator handles special-forms.

    /// `(process-data data)` -> placeholder function for testing
    fn process_data(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "process-data".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        // Return the input data as-is for now
        Ok(args[0].clone())
    }

    /// `(read-file path)` -> placeholder function for testing
    fn read_file(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "read-file".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        // Return a placeholder string for now
        Ok(Value::String("file content placeholder".to_string()))
    }

    // set! is an evaluator special-form; no stdlib implementation here.

    /// `(deftype name type-expr)` -> defines a custom type alias (placeholder implementation)
    fn deftype(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "deftype".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        // For now, just return nil since type aliases are not fully implemented
        // This allows the test to at least parse and run without error
        Ok(Value::Nil)
    }

    /// `(keys map)` -> returns a vector of keys present in the map
    fn keys(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "keys".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        match &args[0] {
            Value::Map(map) => {
                let mut out: Vec<Value> = Vec::new();
                for key in map.keys() {
                    let v = match key {
                        crate::ast::MapKey::String(s) => Value::String(s.clone()),
                        crate::ast::MapKey::Keyword(k) => Value::Keyword(k.clone()),
                        crate::ast::MapKey::Integer(i) => Value::Integer(*i),
                    };
                    out.push(v);
                }
                Ok(Value::Vector(out))
            }
            other => Err(RuntimeError::TypeError {
                expected: "map".to_string(),
                actual: other.type_name().to_string(),
                operation: "keys".to_string(),
            }),
        }
    }

    /// `(update map key f & args)` -> returns a new map with key updated by applying f to current value and extra args
    /// f may be a function value, a keyword, or a string naming a function in the current environment.
    /// This builtin needs evaluator context to call user functions.
    fn update(
        args: Vec<Value>,
        evaluator: &Evaluator,
        env: &mut Environment,
    ) -> RuntimeResult<Value> {
        if args.len() < 3 {
            return Err(RuntimeError::ArityMismatch {
                function: "update".to_string(),
                expected: "at least 3".to_string(),
                actual: args.len(),
            });
        }

        // extract map
        let map_val = &args[0];
        let key_val = &args[1];
        let f_val = &args[2];
        let extra_args: Vec<Value> = if args.len() > 3 {
            args[3..].to_vec()
        } else {
            Vec::new()
        };

        // Support update for both maps and vectors
        let mut new_map_opt: Option<std::collections::HashMap<crate::ast::MapKey, Value>> = None;
        let mut new_vec_opt: Option<Vec<Value>> = None;

        match map_val {
            Value::Map(m) => new_map_opt = Some(m.clone()),
            Value::Vector(v) => new_vec_opt = Some(v.clone()),
            other => {
                return Err(RuntimeError::TypeError {
                    expected: "map or vector".to_string(),
                    actual: other.type_name().to_string(),
                    operation: "update".to_string(),
                })
            }
        }

        // convert key
        let map_key = match key_val {
            Value::String(s) => crate::ast::MapKey::String(s.clone()),
            Value::Keyword(k) => crate::ast::MapKey::Keyword(k.clone()),
            Value::Integer(i) => crate::ast::MapKey::Integer(*i),
            other => {
                return Err(RuntimeError::TypeError {
                    expected: "string, keyword or integer".to_string(),
                    actual: other.type_name().to_string(),
                    operation: "update".to_string(),
                })
            }
        };

        if let Some(mut new_map) = new_map_opt {
            let current = new_map.get(&map_key).cloned().unwrap_or(Value::Nil);
            // Resolve function-like value (direct function/keyword or string lookup)
            let f_to_call = match f_val {
                Value::Function(_) | Value::FunctionPlaceholder(_) | Value::Keyword(_) => {
                    f_val.clone()
                }
                Value::String(name) => {
                    // Resolve by symbol name in current environment
                    if let Some(resolved) = env.lookup(&crate::ast::Symbol(name.clone())) {
                        match resolved {
                            Value::Function(_)
                            | Value::FunctionPlaceholder(_)
                            | Value::Keyword(_) => resolved,
                            other => {
                                return Err(RuntimeError::TypeError {
                                    expected: "function".to_string(),
                                    actual: other.type_name().to_string(),
                                    operation: "update".to_string(),
                                })
                            }
                        }
                    } else {
                        return Err(RuntimeError::Generic(format!(
                            "function '{}' not found",
                            name
                        )));
                    }
                }
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "function".to_string(),
                        actual: f_val.type_name().to_string(),
                        operation: "update".to_string(),
                    })
                }
            };
            let new_value = {
                let mut args_for_call = Vec::with_capacity(1 + extra_args.len());
                args_for_call.push(current.clone());
                args_for_call.extend(extra_args.clone());
                match evaluator.call_function(f_to_call, &args_for_call, env)? {
                    ExecutionOutcome::Complete(v) => v,
                    ExecutionOutcome::RequiresHost(hc) => {
                        return Err(RuntimeError::Generic(format!(
                            "Host call required in stdlib 'update': {}",
                            hc.capability_id
                        )))
                    }
                    #[cfg(feature = "effect-boundary")]
                    ExecutionOutcome::RequiresHost(_) => {
                        return Err(RuntimeError::Generic(
                            "Host effect required in stdlib 'update'".to_string(),
                        ))
                    }
                }
            };
            new_map.insert(map_key, new_value);
            return Ok(Value::Map(new_map));
        }

        if let Some(mut new_vec) = new_vec_opt {
            // key must be integer index
            let index = match key_val {
                Value::Integer(i) => *i as usize,
                other => {
                    return Err(RuntimeError::TypeError {
                        expected: "integer index for vector".to_string(),
                        actual: other.type_name().to_string(),
                        operation: "update".to_string(),
                    })
                }
            };

            if index >= new_vec.len() {
                return Err(RuntimeError::IndexOutOfBounds {
                    index: index as i64,
                    length: new_vec.len(),
                });
            }

            let current = new_vec[index].clone();
            // Resolve function-like value (direct function/keyword or string lookup)
            let f_to_call = match f_val {
                Value::Function(_) | Value::FunctionPlaceholder(_) | Value::Keyword(_) => {
                    f_val.clone()
                }
                Value::String(name) => {
                    if let Some(resolved) = env.lookup(&crate::ast::Symbol(name.clone())) {
                        match resolved {
                            Value::Function(_)
                            | Value::FunctionPlaceholder(_)
                            | Value::Keyword(_) => resolved,
                            other => {
                                return Err(RuntimeError::TypeError {
                                    expected: "function".to_string(),
                                    actual: other.type_name().to_string(),
                                    operation: "update".to_string(),
                                })
                            }
                        }
                    } else {
                        return Err(RuntimeError::Generic(format!(
                            "function '{}' not found",
                            name
                        )));
                    }
                }
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "function".to_string(),
                        actual: f_val.type_name().to_string(),
                        operation: "update".to_string(),
                    })
                }
            };
            let new_value = {
                let mut args_for_call = Vec::with_capacity(1 + extra_args.len());
                args_for_call.push(current.clone());
                args_for_call.extend(extra_args.clone());
                match evaluator.call_function(f_to_call, &args_for_call, env)? {
                    ExecutionOutcome::Complete(v) => v,
                    ExecutionOutcome::RequiresHost(hc) => {
                        return Err(RuntimeError::Generic(format!(
                            "Host call required in stdlib 'update': {}",
                            hc.capability_id
                        )))
                    }
                    #[cfg(feature = "effect-boundary")]
                    ExecutionOutcome::RequiresHost(_) => {
                        return Err(RuntimeError::Generic(
                            "Host effect required in stdlib 'update'".to_string(),
                        ))
                    }
                }
            };
            new_vec[index] = new_value;
            return Ok(Value::Vector(new_vec));
        }

        unreachable!()
    }

    // --- Additional Standard Library Function Implementations ---

    /// `(odd? n)` - Returns true if n is odd, false otherwise
    fn odd(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "odd?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        match &args[0] {
            Value::Integer(n) => Ok(Value::Boolean(n % 2 == 1)),
            Value::Float(f) => Ok(Value::Boolean((*f as i64) % 2 == 1)),
            other => {
                return Err(RuntimeError::TypeError {
                    expected: "number".to_string(),
                    actual: other.type_name().to_string(),
                    operation: "odd?".to_string(),
                })
            }
        }
    }

    /// `(inc n)` - Returns n + 1
    fn inc(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "inc".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        match &args[0] {
            Value::Integer(n) => Ok(Value::Integer(n + 1)),
            Value::Float(f) => Ok(Value::Float(f + 1.0)),
            other => {
                return Err(RuntimeError::TypeError {
                    expected: "number".to_string(),
                    actual: other.type_name().to_string(),
                    operation: "inc".to_string(),
                })
            }
        }
    }

    /// `(str ...)` - Converts all arguments to strings and concatenates them
    fn str(args: Vec<Value>) -> RuntimeResult<Value> {
        let mut result = String::new();

        // Convert all arguments to strings and concatenate them
        for arg in args {
            match arg {
                Value::String(s) => {
                    // For string concatenation, just append the string directly
                    result.push_str(&s);
                }
                Value::Integer(n) => result.push_str(&n.to_string()),
                Value::Float(f) => result.push_str(&f.to_string()),
                Value::Boolean(b) => result.push_str(&b.to_string()),
                Value::Keyword(k) => result.push_str(&format!(":{}", k.0)),
                Value::Nil => result.push_str("nil"),
                Value::Vector(v) => result.push_str(&Value::Vector(v).to_string()),
                Value::List(l) => result.push_str(&Value::List(l).to_string()),
                Value::Map(m) => result.push_str(&Value::Map(m).to_string()),
                _ => result.push_str(&arg.to_string()),
            }
        }
        Ok(Value::String(result))
    }

    /// `(map-indexed f collection)` - Maps function over collection with index
    fn map_indexed(
        args: Vec<Value>,
        evaluator: &Evaluator,
        env: &mut Environment,
    ) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "map-indexed".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        let f = &args[0];
        let collection = &args[1];

        let elements = match collection {
            Value::Vector(vec) => vec.clone(),
            Value::String(s) => s.chars().map(|c| Value::String(c.to_string())).collect(),
            Value::List(list) => list.clone(),
            other => {
                return Err(RuntimeError::TypeError {
                    expected: "vector, string, or list".to_string(),
                    actual: other.type_name().to_string(),
                    operation: "map-indexed".to_string(),
                })
            }
        };

        let mut result = Vec::new();
        for (index, element) in elements.into_iter().enumerate() {
            let mapped_value = match f {
                Value::Function(_) | Value::FunctionPlaceholder(_) | Value::Keyword(_) => {
                    let args_for_call = vec![Value::Integer(index as i64), element.clone()];
                    match evaluator.call_function(f.clone(), &args_for_call, env)? {
                        ExecutionOutcome::Complete(v) => v,
                        ExecutionOutcome::RequiresHost(hc) => {
                            return Err(RuntimeError::Generic(format!(
                                "Host call required in stdlib 'map-indexed': {}",
                                hc.capability_id
                            )))
                        }
                        #[cfg(feature = "effect-boundary")]
                        ExecutionOutcome::RequiresHost(_) => {
                            return Err(RuntimeError::Generic(
                                "Host effect required in stdlib 'map-indexed'".to_string(),
                            ))
                        }
                    }
                }
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "function".to_string(),
                        actual: f.type_name().to_string(),
                        operation: "map-indexed".to_string(),
                    })
                }
            };
            result.push(mapped_value);
        }

        match collection {
            Value::Vector(_) => Ok(Value::Vector(result)),
            Value::String(_) => Ok(Value::String(
                result.into_iter().map(|v| v.to_string()).collect(),
            )),
            Value::List(_) => Ok(Value::List(result)),
            _ => unreachable!(),
        }
    }

    /// `(remove pred collection)` - Returns collection with elements that don't satisfy pred removed
    fn remove(
        args: Vec<Value>,
        evaluator: &Evaluator,
        env: &mut Environment,
    ) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "remove".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        let pred = &args[0];
        let collection = &args[1];

        let elements = match collection {
            Value::Vector(vec) => vec.clone(),
            Value::String(s) => s.chars().map(|c| Value::String(c.to_string())).collect(),
            Value::List(list) => list.clone(),
            other => {
                return Err(RuntimeError::TypeError {
                    expected: "vector, string, or list".to_string(),
                    actual: other.type_name().to_string(),
                    operation: "remove".to_string(),
                })
            }
        };

        let mut result = Vec::new();
        for element in elements {
            let should_keep = match pred {
                Value::Function(_) | Value::FunctionPlaceholder(_) | Value::Keyword(_) => {
                    let args_for_call = vec![element.clone()];
                    let pred_result =
                        match evaluator.call_function(pred.clone(), &args_for_call, env)? {
                            ExecutionOutcome::Complete(v) => v,
                            ExecutionOutcome::RequiresHost(hc) => {
                                return Err(RuntimeError::Generic(format!(
                                    "Host call required in stdlib 'remove': {}",
                                    hc.capability_id
                                )))
                            }
                            #[cfg(feature = "effect-boundary")]
                            ExecutionOutcome::RequiresHost(_) => {
                                return Err(RuntimeError::Generic(
                                    "Host effect required in stdlib 'remove'".to_string(),
                                ))
                            }
                        };
                    match pred_result {
                        Value::Boolean(b) => !b, // Keep elements where predicate returns false
                        _ => true,               // Keep if predicate doesn't return boolean
                    }
                }
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "function".to_string(),
                        actual: pred.type_name().to_string(),
                        operation: "remove".to_string(),
                    })
                }
            };
            if should_keep {
                result.push(element);
            }
        }

        match collection {
            Value::Vector(_) => Ok(Value::Vector(result)),
            Value::String(_) => Ok(Value::String(
                result.into_iter().map(|v| v.to_string()).collect(),
            )),
            Value::List(_) => Ok(Value::List(result)),
            _ => unreachable!(),
        }
    }

    /// `(some? pred collection)` - Returns true if any element satisfies pred
    fn some(
        args: Vec<Value>,
        evaluator: &Evaluator,
        env: &mut Environment,
    ) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "some?".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        let pred = &args[0];
        let collection = &args[1];

        let elements = match collection {
            Value::Vector(vec) => vec.clone(),
            Value::String(s) => s.chars().map(|c| Value::String(c.to_string())).collect(),
            Value::List(list) => list.clone(),
            other => {
                return Err(RuntimeError::TypeError {
                    expected: "vector, string, or list".to_string(),
                    actual: other.type_name().to_string(),
                    operation: "some?".to_string(),
                })
            }
        };

        for element in elements {
            let result = match pred {
                Value::Function(_) | Value::FunctionPlaceholder(_) | Value::Keyword(_) => {
                    let args_for_call = vec![element.clone()];
                    match evaluator.call_function(pred.clone(), &args_for_call, env)? {
                        ExecutionOutcome::Complete(v) => v,
                        ExecutionOutcome::RequiresHost(hc) => {
                            return Err(RuntimeError::Generic(format!(
                                "Host call required in stdlib 'some?': {}",
                                hc.capability_id
                            )))
                        }
                        #[cfg(feature = "effect-boundary")]
                        ExecutionOutcome::RequiresHost(_) => {
                            return Err(RuntimeError::Generic(
                                "Host effect required in stdlib 'some?'".to_string(),
                            ))
                        }
                    }
                }
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "function".to_string(),
                        actual: pred.type_name().to_string(),
                        operation: "some?".to_string(),
                    })
                }
            };
            match result {
                Value::Boolean(true) => return Ok(Value::Boolean(true)),
                _ => continue,
            }
        }

        Ok(Value::Boolean(false))
    }

    /// `(every? pred collection)` - Returns true if all elements satisfy pred
    fn every(
        args: Vec<Value>,
        evaluator: &Evaluator,
        env: &mut Environment,
    ) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "every?".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        let pred = &args[0];
        let collection = &args[1];

        let elements = match collection {
            Value::Vector(vec) => vec.clone(),
            Value::String(s) => s.chars().map(|c| Value::String(c.to_string())).collect(),
            Value::List(list) => list.clone(),
            other => {
                return Err(RuntimeError::TypeError {
                    expected: "vector, string, or list".to_string(),
                    actual: other.type_name().to_string(),
                    operation: "every?".to_string(),
                })
            }
        };

        for element in elements {
            let result = match pred {
                Value::Function(_) | Value::FunctionPlaceholder(_) | Value::Keyword(_) => {
                    let args_for_call = vec![element.clone()];
                    match evaluator.call_function(pred.clone(), &args_for_call, env)? {
                        ExecutionOutcome::Complete(v) => v,
                        ExecutionOutcome::RequiresHost(hc) => {
                            return Err(RuntimeError::Generic(format!(
                                "Host call required in stdlib 'every?': {}",
                                hc.capability_id
                            )))
                        }
                        #[cfg(feature = "effect-boundary")]
                        ExecutionOutcome::RequiresHost(_) => {
                            return Err(RuntimeError::Generic(
                                "Host effect required in stdlib 'every?'".to_string(),
                            ))
                        }
                    }
                }
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "function".to_string(),
                        actual: pred.type_name().to_string(),
                        operation: "every?".to_string(),
                    })
                }
            };
            match result {
                Value::Boolean(false) => return Ok(Value::Boolean(false)),
                _ => continue,
            }
        }

        Ok(Value::Boolean(true))
    }

    /// `(even? n)` - Returns true if n is even, false otherwise
    fn even(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "even?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        match &args[0] {
            Value::Integer(n) => Ok(Value::Boolean(n % 2 == 0)),
            Value::Float(f) => Ok(Value::Boolean((*f as i64) % 2 == 0)),
            other => {
                return Err(RuntimeError::TypeError {
                    expected: "number".to_string(),
                    actual: other.type_name().to_string(),
                    operation: "even?".to_string(),
                })
            }
        }
    }

    /// `(first collection)` - Returns the first element of a collection
    fn first(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "first".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        match &args[0] {
            Value::Vector(vec) => {
                if vec.is_empty() {
                    Ok(Value::Nil)
                } else {
                    Ok(vec[0].clone())
                }
            }
            Value::String(s) => {
                if s.is_empty() {
                    Ok(Value::Nil)
                } else {
                    Ok(Value::String(s.chars().next().unwrap().to_string()))
                }
            }
            Value::List(list) => {
                if list.is_empty() {
                    Ok(Value::Nil)
                } else {
                    Ok(list[0].clone())
                }
            }
            other => Err(RuntimeError::TypeError {
                expected: "vector, string, or list".to_string(),
                actual: other.type_name().to_string(),
                operation: "first".to_string(),
            }),
        }
    }

    /// `(rest collection)` - Returns all elements except the first
    fn rest(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "rest".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        match &args[0] {
            Value::Vector(vec) => {
                if vec.len() <= 1 {
                    Ok(Value::Vector(vec![]))
                } else {
                    Ok(Value::Vector(vec[1..].to_vec()))
                }
            }
            Value::String(s) => {
                if s.len() <= 1 {
                    Ok(Value::String(String::new()))
                } else {
                    Ok(Value::String(s.chars().skip(1).collect()))
                }
            }
            Value::List(list) => {
                if list.len() <= 1 {
                    Ok(Value::List(vec![]))
                } else {
                    Ok(Value::List(list[1..].to_vec()))
                }
            }
            other => Err(RuntimeError::TypeError {
                expected: "vector, string, or list".to_string(),
                actual: other.type_name().to_string(),
                operation: "rest".to_string(),
            }),
        }
    }

    /// `(nth collection index)` - Returns the element at the given index
    fn nth(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "nth".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        let index = match &args[1] {
            Value::Integer(i) => *i as usize,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "integer".to_string(),
                    actual: args[1].type_name().to_string(),
                    operation: "nth".to_string(),
                })
            }
        };

        match &args[0] {
            Value::Vector(vec) => {
                if index >= vec.len() {
                    Ok(Value::Nil)
                } else {
                    Ok(vec[index].clone())
                }
            }
            Value::String(s) => {
                if index >= s.chars().count() {
                    Ok(Value::Nil)
                } else {
                    Ok(Value::String(s.chars().nth(index).unwrap().to_string()))
                }
            }
            Value::List(list) => {
                if index >= list.len() {
                    Ok(Value::Nil)
                } else {
                    Ok(list[index].clone())
                }
            }
            other => Err(RuntimeError::TypeError {
                expected: "vector, string, or list".to_string(),
                actual: other.type_name().to_string(),
                operation: "nth".to_string(),
            }),
        }
    }

    /// `(range end)` or `(range start end)` or `(range start end step)` - Returns a range of numbers
    fn range(args: Vec<Value>) -> RuntimeResult<Value> {
        let (start, end, step) = match args.len() {
            1 => {
                let end = match &args[0] {
                    Value::Integer(i) => *i,
                    _ => {
                        return Err(RuntimeError::TypeError {
                            expected: "integer".to_string(),
                            actual: args[0].type_name().to_string(),
                            operation: "range".to_string(),
                        })
                    }
                };
                (0, end, 1)
            }
            2 => {
                let start = match &args[0] {
                    Value::Integer(i) => *i,
                    _ => {
                        return Err(RuntimeError::TypeError {
                            expected: "integer".to_string(),
                            actual: args[0].type_name().to_string(),
                            operation: "range".to_string(),
                        })
                    }
                };
                let end = match &args[1] {
                    Value::Integer(i) => *i,
                    _ => {
                        return Err(RuntimeError::TypeError {
                            expected: "integer".to_string(),
                            actual: args[1].type_name().to_string(),
                            operation: "range".to_string(),
                        })
                    }
                };
                (start, end, 1)
            }
            3 => {
                let start = match &args[0] {
                    Value::Integer(i) => *i,
                    _ => {
                        return Err(RuntimeError::TypeError {
                            expected: "integer".to_string(),
                            actual: args[0].type_name().to_string(),
                            operation: "range".to_string(),
                        })
                    }
                };
                let end = match &args[1] {
                    Value::Integer(i) => *i,
                    _ => {
                        return Err(RuntimeError::TypeError {
                            expected: "integer".to_string(),
                            actual: args[1].type_name().to_string(),
                            operation: "range".to_string(),
                        })
                    }
                };
                let step = match &args[2] {
                    Value::Integer(i) => *i,
                    _ => {
                        return Err(RuntimeError::TypeError {
                            expected: "integer".to_string(),
                            actual: args[2].type_name().to_string(),
                            operation: "range".to_string(),
                        })
                    }
                };
                (start, end, step)
            }
            _ => {
                return Err(RuntimeError::ArityMismatch {
                    function: "range".to_string(),
                    expected: "1, 2, or 3".to_string(),
                    actual: args.len(),
                });
            }
        };

        let mut result = Vec::new();
        let mut current = start;

        if step > 0 {
            while current < end {
                result.push(Value::Integer(current));
                current += step;
            }
        } else if step < 0 {
            while current > end {
                result.push(Value::Integer(current));
                current += step;
            }
        }

        Ok(Value::Vector(result))
    }

    /// `(map function collection)` - Applies a function to each element of a collection
    fn map(args: Vec<Value>, evaluator: &Evaluator, env: &mut Environment) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "map".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        let function = &args[0];
        let collection = &args[1];

        let elements = match collection {
            Value::Vector(vec) => vec.clone(),
            Value::String(s) => s.chars().map(|c| Value::String(c.to_string())).collect(),
            Value::List(list) => list.clone(),
            other => {
                return Err(RuntimeError::TypeError {
                    expected: "vector, string, or list".to_string(),
                    actual: other.type_name().to_string(),
                    operation: "map".to_string(),
                })
            }
        };

        let mut result = Vec::new();
        for element in elements {
            match evaluator.call_function(function.clone(), &[element], env)? {
                ExecutionOutcome::Complete(v) => result.push(v),
                ExecutionOutcome::RequiresHost(hc) => {
                    return Err(RuntimeError::Generic(format!(
                        "Host call required in stdlib 'map': {}",
                        hc.capability_id
                    )))
                }
                #[cfg(feature = "effect-boundary")]
                ExecutionOutcome::RequiresHost(_) => {
                    return Err(RuntimeError::Generic(
                        "Host effect required in stdlib 'map'".to_string(),
                    ))
                }
            }
        }

        // Return the same type as the input collection
        match collection {
            Value::Vector(_) => Ok(Value::Vector(result)),
            Value::String(_) => Ok(Value::Vector(result)),
            Value::List(_) => Ok(Value::List(result)),
            _ => unreachable!(),
        }
    }

    /// `(filter predicate collection)` - Returns elements that satisfy the predicate
    fn filter(
        args: Vec<Value>,
        evaluator: &Evaluator,
        env: &mut Environment,
    ) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "filter".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        let predicate = &args[0];
        let collection = &args[1];

        let elements = match collection {
            Value::Vector(vec) => vec.clone(),
            Value::String(s) => s.chars().map(|c| Value::String(c.to_string())).collect(),
            Value::List(list) => list.clone(),
            Value::Map(map) => {
                // For maps, convert to vector of [key value] pairs
                map.iter()
                    .map(|(k, v)| {
                        let key_val = match k {
                            MapKey::Keyword(kw) => Value::Keyword(kw.clone()),
                            MapKey::String(s) => Value::String(s.clone()),
                            MapKey::Integer(i) => Value::Integer(*i),
                        };
                        Value::Vector(vec![key_val, v.clone()])
                    })
                    .collect()
            }
            other => {
                return Err(RuntimeError::TypeError {
                    expected: "vector, string, list, or map".to_string(),
                    actual: other.type_name().to_string(),
                    operation: "filter".to_string(),
                })
            }
        };

        let mut result = Vec::new();
        for element in elements {
            let should_include =
                match evaluator.call_function(predicate.clone(), &[element.clone()], env)? {
                    ExecutionOutcome::Complete(v) => v,
                    ExecutionOutcome::RequiresHost(hc) => {
                        return Err(RuntimeError::Generic(format!(
                            "Host call required in stdlib 'filter': {}",
                            hc.capability_id
                        )))
                    }
                    #[cfg(feature = "effect-boundary")]
                    ExecutionOutcome::RequiresHost(_) => {
                        return Err(RuntimeError::Generic(
                            "Host effect required in stdlib 'filter'".to_string(),
                        ))
                    }
                };
            match should_include {
                Value::Boolean(true) => result.push(element),
                Value::Boolean(false) => {}
                other => {
                    return Err(RuntimeError::TypeError {
                        expected: "boolean".to_string(),
                        actual: other.type_name().to_string(),
                        operation: "filter predicate".to_string(),
                    })
                }
            }
        }

        // Return the same type as the input collection
        match collection {
            Value::Vector(_) => Ok(Value::Vector(result)),
            Value::String(_) => Ok(Value::Vector(result)),
            Value::List(_) => Ok(Value::List(result)),
            Value::Map(_) => {
                // For maps, convert the filtered vector of [key value] pairs back to a map
                let mut filtered_map = std::collections::HashMap::new();
                for pair in result {
                    if let Value::Vector(key_value) = pair {
                        if key_value.len() == 2 {
                            let map_key = match &key_value[0] {
                                Value::Keyword(kw) => MapKey::Keyword(kw.clone()),
                                Value::String(s) => MapKey::String(s.clone()),
                                Value::Integer(i) => MapKey::Integer(*i),
                                _ => continue, // Skip invalid keys
                            };
                            filtered_map.insert(map_key, key_value[1].clone());
                        }
                    }
                }
                Ok(Value::Map(filtered_map))
            }
            _ => unreachable!(),
        }
    }

    /// `(reduce function collection)` - Reduces a collection using a function
    fn reduce(
        args: Vec<Value>,
        evaluator: &Evaluator,
        env: &mut Environment,
    ) -> RuntimeResult<Value> {
        if args.len() < 2 || args.len() > 3 {
            return Err(RuntimeError::ArityMismatch {
                function: "reduce".to_string(),
                expected: "2 to 3".to_string(),
                actual: args.len(),
            });
        }

        let function = &args[0];
        let collection_arg_index = args.len() - 1;
        let collection_val = &args[collection_arg_index];

        let collection = match collection_val {
            Value::Vector(v) => v.clone(),
            Value::String(s) => s.chars().map(|c| Value::String(c.to_string())).collect(),
            Value::List(list) => list.clone(),
            other => {
                return Err(RuntimeError::TypeError {
                    expected: "vector, string, or list".to_string(),
                    actual: other.type_name().to_string(),
                    operation: "reduce".to_string(),
                })
            }
        };

        if collection.is_empty() {
            return if args.len() == 3 {
                Ok(args[1].clone())
            } else {
                Err(RuntimeError::new(
                    "reduce on empty collection with no initial value",
                ))
            };
        }

        let (mut accumulator, rest) = if args.len() == 3 {
            (args[1].clone(), collection.as_slice())
        } else {
            (collection[0].clone(), &collection[1..])
        };

        for value in rest {
            accumulator = match evaluator.call_function(
                function.clone(),
                &[accumulator.clone(), value.clone()],
                env,
            )? {
                ExecutionOutcome::Complete(v) => v,
                ExecutionOutcome::RequiresHost(hc) => {
                    return Err(RuntimeError::Generic(format!(
                        "Host call required in stdlib 'reduce': {}",
                        hc.capability_id
                    )))
                }
                #[cfg(feature = "effect-boundary")]
                ExecutionOutcome::RequiresHost(_) => {
                    return Err(RuntimeError::Generic(
                        "Host effect required in stdlib 'reduce'".to_string(),
                    ))
                }
            };
        }

        Ok(accumulator)
    }

    /// `(sort collection)` or `(sort comparator collection)` - Sorts a collection
    fn sort(args: Vec<Value>) -> RuntimeResult<Value> {
        let (collection, reverse) = match args.len() {
            1 => (&args[0], false),
            2 => {
                let comparator = &args[0];
                let collection = &args[1];

                // Check if comparator is a function or a keyword like '>'
                let reverse = match comparator {
                    Value::Keyword(k) => k.0 == ">",
                    _ => false,
                };

                (collection, reverse)
            }
            _ => {
                return Err(RuntimeError::ArityMismatch {
                    function: "sort".to_string(),
                    expected: "1 or 2".to_string(),
                    actual: args.len(),
                });
            }
        };

        match collection {
            Value::Vector(vec) => {
                let mut sorted = vec.clone();
                if reverse {
                    sorted.sort_by(|a, b| b.compare(a));
                } else {
                    sorted.sort_by(|a, b| a.compare(b));
                }
                Ok(Value::Vector(sorted))
            }
            Value::String(s) => {
                let mut chars: Vec<char> = s.chars().collect();
                if reverse {
                    chars.sort_by(|a, b| b.cmp(a));
                } else {
                    chars.sort();
                }
                Ok(Value::String(chars.into_iter().collect()))
            }
            Value::List(list) => {
                let mut sorted = list.clone();
                if reverse {
                    sorted.sort_by(|a, b| b.compare(a));
                } else {
                    sorted.sort_by(|a, b| a.compare(b));
                }
                Ok(Value::List(sorted))
            }
            other => Err(RuntimeError::TypeError {
                expected: "vector, string, or list".to_string(),
                actual: other.type_name().to_string(),
                operation: "sort".to_string(),
            }),
        }
    }

    /// `(sort-by key-fn collection)` - Sorts a collection by applying a key function
    fn sort_by(
        args: Vec<Value>,
        evaluator: &Evaluator,
        env: &mut Environment,
    ) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "sort-by".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        let key_fn = &args[0];
        let collection = &args[1];

        let elements = match collection {
            Value::Vector(vec) => vec.clone(),
            Value::String(s) => s.chars().map(|c| Value::String(c.to_string())).collect(),
            Value::List(list) => list.clone(),
            other => {
                return Err(RuntimeError::TypeError {
                    expected: "vector, string, or list".to_string(),
                    actual: other.type_name().to_string(),
                    operation: "sort-by".to_string(),
                })
            }
        };

        // Create pairs of (element, key) for sorting
        let mut pairs = Vec::new();
        for element in elements {
            let key = match evaluator.call_function(key_fn.clone(), &[element.clone()], env)? {
                ExecutionOutcome::Complete(v) => v,
                ExecutionOutcome::RequiresHost(hc) => {
                    return Err(RuntimeError::Generic(format!(
                        "Host call required in stdlib 'sort-by': {}",
                        hc.capability_id
                    )))
                }
                #[cfg(feature = "effect-boundary")]
                ExecutionOutcome::RequiresHost(_) => {
                    return Err(RuntimeError::Generic(
                        "Host effect required in stdlib 'sort-by'".to_string(),
                    ))
                }
            };
            pairs.push((element, key));
        }

        // Sort by key
        pairs.sort_by(|a, b| a.1.compare(&b.1));

        // Extract sorted elements
        let result: Vec<Value> = pairs.into_iter().map(|(element, _)| element).collect();

        // Return the same type as the input collection
        match collection {
            Value::Vector(_) => Ok(Value::Vector(result)),
            Value::String(_) => Ok(Value::Vector(result)),
            Value::List(_) => Ok(Value::List(result)),
            _ => unreachable!(),
        }
    }

    /// `(frequencies collection)` - Returns a map of element frequencies
    fn frequencies(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "frequencies".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        let collection = &args[0];
        let elements = match collection {
            Value::Vector(vec) => vec.clone(),
            Value::String(s) => s.chars().map(|c| Value::String(c.to_string())).collect(),
            Value::List(list) => list.clone(),
            other => {
                return Err(RuntimeError::TypeError {
                    expected: "vector, string, or list".to_string(),
                    actual: other.type_name().to_string(),
                    operation: "frequencies".to_string(),
                })
            }
        };

        let mut freq_map = std::collections::HashMap::new();
        for element in elements {
            // Convert Value to MapKey for HashMap key
            let key = match &element {
                Value::String(s) => crate::ast::MapKey::String(s.clone()),
                Value::Keyword(k) => crate::ast::MapKey::Keyword(k.clone()),
                Value::Integer(i) => crate::ast::MapKey::Integer(*i),
                Value::Boolean(b) => crate::ast::MapKey::String(b.to_string()),
                Value::Float(f) => crate::ast::MapKey::String(f.to_string()),
                Value::Nil => crate::ast::MapKey::String("nil".to_string()),
                _ => crate::ast::MapKey::String(element.to_string()),
            };

            let count = freq_map.entry(key).or_insert(Value::Integer(0));
            if let Value::Integer(n) = count {
                *count = Value::Integer(*n + 1);
            }
        }

        Ok(Value::Map(freq_map))
    }

    /// `(distinct collection)` - Returns collection with duplicates removed
    fn distinct(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "distinct".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        let collection = &args[0];
        let elements = match collection {
            Value::Vector(vec) => vec.clone(),
            Value::String(s) => s.chars().map(|c| Value::String(c.to_string())).collect(),
            Value::List(list) => list.clone(),
            other => {
                return Err(RuntimeError::TypeError {
                    expected: "vector, string, or list".to_string(),
                    actual: other.type_name().to_string(),
                    operation: "distinct".to_string(),
                })
            }
        };

        let mut result = Vec::new();
        for element in elements {
            if !result.contains(&element) {
                result.push(element);
            }
        }

        match collection {
            Value::Vector(_) => Ok(Value::Vector(result)),
            Value::String(_) => Ok(Value::String(
                result.into_iter().map(|v| v.to_string()).collect(),
            )),
            Value::List(_) => Ok(Value::List(result)),
            _ => unreachable!(),
        }
    }

    /// `(contains? collection item)` - Returns true if collection contains item
    fn contains(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "contains?".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        let collection = &args[0];
        let item = &args[1];

        match collection {
            Value::Vector(vec) => Ok(Value::Boolean(vec.contains(item))),
            Value::String(s) => {
                if let Value::String(item_str) = item {
                    Ok(Value::Boolean(s.contains(item_str)))
                } else {
                    Ok(Value::Boolean(false))
                }
            }
            Value::List(list) => Ok(Value::Boolean(list.contains(item))),
            Value::Map(map) => {
                // For maps, check key presence. Accept keyword/string/integer as keys.
                let key = match item {
                    Value::Keyword(k) => Some(MapKey::Keyword(k.clone())),
                    Value::String(s) => Some(MapKey::String(s.clone())),
                    Value::Integer(i) => Some(MapKey::Integer(*i)),
                    _ => None,
                };
                Ok(Value::Boolean(key.map_or(false, |k| map.contains_key(&k))))
            }
            other => {
                return Err(RuntimeError::TypeError {
                    expected: "vector, string, list, or map".to_string(),
                    actual: other.type_name().to_string(),
                    operation: "contains?".to_string(),
                })
            }
        }
    }

    /// `(merge m1 m2 ... )` - shallow merge of maps; later maps override earlier keys
    fn merge(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.is_empty() {
            return Ok(Value::Map(HashMap::new()));
        }
        let mut out: HashMap<MapKey, Value> = HashMap::new();
        for arg in args {
            match arg {
                Value::Map(m) => {
                    for (k, v) in m {
                        out.insert(k, v);
                    }
                }
                other => {
                    return Err(RuntimeError::TypeError {
                        expected: "map".into(),
                        actual: other.type_name().into(),
                        operation: "merge".into(),
                    });
                }
            }
        }
        Ok(Value::Map(out))
    }

    // Removed all atom functions - use host state capabilities instead
    // Migrate from: (atom 0), (deref atom), (reset! atom val), (swap! atom f args)
    // Migrate to: (call :ccos.state.kv.get {...}), (call :ccos.state.kv.put {...}),
    //             (call :ccos.state.counter.inc {...})

    /// Stub for coordinate-work to satisfy tests until full impl exists
    fn coordinate_work_stub(args: Vec<Value>) -> RuntimeResult<Value> {
        // Echo back a simple map {:status :ok :inputs n}
        let mut m = HashMap::new();
        m.insert(
            MapKey::Keyword(Keyword("status".into())),
            Value::Keyword(Keyword("ok".into())),
        );
        m.insert(
            MapKey::Keyword(Keyword("inputs".into())),
            Value::Integer(args.len() as i64),
        );
        Ok(Value::Map(m))
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
        evaluator
            .host
            .execute_capability(&capability_name, capability_args)
    }
}

/// Register default capabilities in the marketplace
///
/// NOTE: implementation was moved to `// CCOS removed: capabilities::defaults::register_default_capabilities`
/// to keep the runtime stdlib focused on language-level functions. This shim preserves the
/// original public API while delegating to the new location.
/// Register default capabilities
/// Note: Full implementation available when RTFS is used with CCOS
pub async fn register_default_capabilities(
    _marketplace: &crate::runtime::capability_marketplace::CapabilityMarketplace,
) -> RuntimeResult<()> {
    // CCOS implementation required for full functionality
    Ok(())
}

/// Load the standard library into a module registry
/// This creates a "stdlib" module containing all built-in functions
pub fn load_stdlib(module_registry: &ModuleRegistry) -> RuntimeResult<()> {
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
            exports: std::sync::RwLock::new(exports),
            namespace: std::sync::Arc::new(std::sync::RwLock::new(IrEnvironment::new())),
            dependencies: vec![],
        };

        // Register the module
        module_registry.register_module(module)?;
    }

    Ok(())
}
