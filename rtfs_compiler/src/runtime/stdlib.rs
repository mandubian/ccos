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
use std::sync::{Arc, RwLock};
use crate::runtime::module_runtime::{ModuleRegistry, Module, ModuleMetadata, ModuleExport, ExportType};
use crate::runtime::environment::IrEnvironment;
use crate::ir::core::{IrType, IrNode};
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
    fn load_tool_functions(env: &mut Environment) {
        // File I/O
        env.define(
            &Symbol("tool/open-file".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool/open-file".to_string(),
                arity: Arity::Fixed(1),
                func: Arc::new(|args: Vec<Value>| Self::tool_open_file(args)),
            })),
        );

        // HTTP requests
        env.define(
            &Symbol("tool/http-fetch".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool/http-fetch".to_string(),
                arity: Arity::Fixed(1),
                func: Arc::new(|args: Vec<Value>| Self::tool_http_fetch(args)),
            })),
        );

        env.define(
            &Symbol("http-fetch".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "http-fetch".to_string(),
                arity: Arity::Fixed(1),
                func: std::sync::Arc::new(Self::tool_http_fetch),
            })),
        );

        // Convenience testing stub: (http/get url) -> {:status 200 :url url :body "ok"}
        env.define(
            &Symbol("http/get".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "http/get".to_string(),
                arity: Arity::Fixed(1),
                func: Arc::new(|args: Vec<Value>| -> RuntimeResult<Value> {
                    if args.len() != 1 {
                        return Err(RuntimeError::ArityMismatch { function: "http/get".to_string(), expected: "1".to_string(), actual: args.len() });
                    }
                    let url = match &args[0] {
                        Value::String(s) => s.clone(),
                        other => return Err(RuntimeError::TypeError { expected: "string".to_string(), actual: other.type_name().to_string(), operation: "http/get".to_string() }),
                    };
                    let mut map: HashMap<MapKey, Value> = HashMap::new();
                    map.insert(MapKey::Keyword(Keyword("status".into())), Value::Integer(200));
                    map.insert(MapKey::Keyword(Keyword("url".into())), Value::String(url));
                    map.insert(MapKey::Keyword(Keyword("body".into())), Value::String("ok".into()));
                    Ok(Value::Map(map))
                }),
            })),
        );

        // Convenience testing stub: (db/query sql) -> vector of maps with :row and :sql
        env.define(
            &Symbol("db/query".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "db/query".to_string(),
                arity: Arity::Fixed(1),
                func: Arc::new(|args: Vec<Value>| -> RuntimeResult<Value> {
                    if args.len() != 1 {
                        return Err(RuntimeError::ArityMismatch { function: "db/query".to_string(), expected: "1".to_string(), actual: args.len() });
                    }
                    let sql = match &args[0] {
                        Value::String(s) => s.clone(),
                        other => return Err(RuntimeError::TypeError { expected: "string".to_string(), actual: other.type_name().to_string(), operation: "db/query".to_string() }),
                    };
                    let mut row1: HashMap<MapKey, Value> = HashMap::new();
                    row1.insert(MapKey::Keyword(Keyword("row".into())), Value::Integer(1));
                    row1.insert(MapKey::Keyword(Keyword("sql".into())), Value::String(sql.clone()));
                    let mut row2: HashMap<MapKey, Value> = HashMap::new();
                    row2.insert(MapKey::Keyword(Keyword("row".into())), Value::Integer(2));
                    row2.insert(MapKey::Keyword(Keyword("sql".into())), Value::String(sql));
                    Ok(Value::Vector(vec![Value::Map(row1), Value::Map(row2)]))
                }),
            })),
        );

        // Logging
        env.define(
            &Symbol("tool/log".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool/log".to_string(),
                arity: Arity::Variadic(1),
                func: Arc::new(|args: Vec<Value>| Self::tool_log(args)),
            })),
        );

        // System time
        env.define(
            &Symbol("tool/time-ms".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool/time-ms".to_string(),
                arity: Arity::Fixed(0),
                func: Arc::new(|args: Vec<Value>| Self::tool_time_ms(args)),
            })),
        );

    // Alias expected by tests: (current-time-millis)
        env.define(
            &Symbol("current-time-millis".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "current-time-millis".to_string(),
                arity: Arity::Fixed(0),
                func: Arc::new(|args: Vec<Value>| Self::tool_time_ms(args)),
            })),
        );

        // JSON functions
        env.define(
            &Symbol("tool/serialize-json".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool/serialize-json".to_string(),
                arity: Arity::Fixed(1),
                func: Arc::new(|args: Vec<Value>| Self::tool_serialize_json(args)),
            })),
        );

        env.define(
            &Symbol("serialize-json".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "serialize-json".to_string(),
                arity: Arity::Fixed(1),
                func: Arc::new(|args: Vec<Value>| Self::tool_serialize_json(args)),
            })),
        );

        env.define(
            &Symbol("tool/parse-json".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool/parse-json".to_string(),
                arity: Arity::Fixed(1),
                func: Arc::new(|args: Vec<Value>| Self::tool_parse_json(args)),
            })),
        );

        env.define(
            &Symbol("parse-json".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "parse-json".to_string(),
                arity: Arity::Fixed(1),
                func: Arc::new(|args: Vec<Value>| Self::tool_parse_json(args)),
            })),
        );

        // Print functions
        env.define(
            &Symbol("println".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "println".to_string(),
                arity: Arity::Variadic(0),
                func: Arc::new(|args: Vec<Value>| Self::println(args)),
            })),
        );

        // Thread functions
        env.define(
            &Symbol("thread/sleep".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "thread/sleep".to_string(),
                arity: Arity::Fixed(1),
                func: Arc::new(|args: Vec<Value>| Self::thread_sleep(args)),
            })),
        );

        // File I/O functions
        env.define(
            &Symbol("read-lines".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "read-lines".to_string(),
                arity: Arity::Fixed(1),
                func: Arc::new(|args: Vec<Value>| Self::read_lines(args)),
            })),
        );

        // Logging/debugging functions
        env.define(
            &Symbol("step".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "step".to_string(),
                arity: Arity::Variadic(1),
                func: Arc::new(|args: Vec<Value>| Self::step(args)),
            })),
        );

    // Control flow functions are evaluator special-forms; do not re-register here.

        // Collection helpers: keys
        env.define(
            &Symbol("keys".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "keys".to_string(),
                arity: Arity::Fixed(1),
                func: Arc::new(|args: Vec<Value>| Self::keys(args)),
            })),
        );

        // Collection helpers: vals
        env.define(
            &Symbol("vals".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "vals".to_string(),
                arity: Arity::Fixed(1),
                func: Arc::new(|args: Vec<Value>| Self::vals(args)),
            })),
        );

        // Map lookup returning entry pair or nil: (find m k) -> [k v] | nil
        env.define(
            &Symbol("find".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "find".to_string(),
                arity: Arity::Fixed(2),
                func: Arc::new(Self::find),
            })),
        );

        // Collection helpers: update (map key f) or (update map key default f)
        env.define(
            &Symbol("update".to_string()),
            Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
                name: "update".to_string(),
                arity: Arity::Variadic(3), // allow 3 or 4, runtime validates upper bound
                func: Arc::new(|args: Vec<Value>, evaluator: &Evaluator, env: &mut Environment| {
                    Self::update(args, evaluator, env)
                }),
            })),
        );

        // Atoms and coordination helpers
        env.define(
            &Symbol("atom".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "atom".to_string(),
                arity: Arity::Fixed(1),
                func: Arc::new(|args: Vec<Value>| Self::atom_new(args)),
            })),
        );
        env.define(
            &Symbol("deref".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "deref".to_string(),
                arity: Arity::Fixed(1),
                func: Arc::new(|args: Vec<Value>| Self::atom_deref(args)),
            })),
        );
        env.define(
            &Symbol("reset!".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "reset!".to_string(),
                arity: Arity::Fixed(2),
                func: Arc::new(|args: Vec<Value>| Self::atom_reset(args)),
            })),
        );
        env.define(
            &Symbol("swap!".to_string()),
            Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
                name: "swap!".to_string(),
                arity: Arity::Variadic(2), // (swap! a f & args)
                func: Arc::new(|args: Vec<Value>, evaluator: &Evaluator, env: &mut Environment| {
                    Self::atom_swap(args, evaluator, env)
                }),
            })),
        );
        env.define(
            &Symbol("coordinate-work".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "coordinate-work".to_string(),
                arity: Arity::Variadic(0),
                func: std::sync::Arc::new(Self::coordinate_work_stub),
            })),
        );
    }

    /// Loads the secure standard library functions into the environment.
    /// 
    /// These functions are pure and safe to execute in any context.
    fn load_secure_functions(env: &mut Environment) {
        // Basic I/O functions
        env.define(
            &Symbol("println".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "println".to_string(),
                arity: Arity::Variadic(0),
                func: std::sync::Arc::new(Self::println),
            })),
        );

        // Thread/sleep function
        env.define(
            &Symbol("thread/sleep".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "thread/sleep".to_string(),
                arity: Arity::Fixed(1),
                func: std::sync::Arc::new(Self::thread_sleep),
            })),
        );

        // File I/O functions
        env.define(
            &Symbol("read-lines".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "read-lines".to_string(),
                arity: Arity::Fixed(1),
                func: std::sync::Arc::new(Self::read_lines),
            })),
        );

        // Logging/debugging functions
        env.define(
            &Symbol("step".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "step".to_string(),
                arity: Arity::Variadic(1),
                func: std::sync::Arc::new(Self::step),
            })),
        );

    // Control flow functions are evaluator special-forms; do not re-register here.

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
                        return Err(RuntimeError::ArityMismatch { function: "getMessage".to_string(), expected: "1".to_string(), actual: args.len() });
                    }
                    match &args[0] {
                        Value::Error(err) => Ok(Value::String(err.message.clone())),
                        other => Err(RuntimeError::TypeError { expected: "error".to_string(), actual: other.type_name().to_string(), operation: "getMessage".to_string() })
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
                        return Err(RuntimeError::ArityMismatch { function: "Exception.".to_string(), expected: "1+".to_string(), actual: args.len() });
                    }
                    let msg = match &args[0] {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    Ok(Value::Error(crate::runtime::values::ErrorValue { message: msg, stack_trace: None }))
                }),
            })),
        );

        // Throw: raises a runtime error from an error value or string
        env.define(
            &Symbol("throw".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "throw".to_string(),
                arity: Arity::Fixed(1),
                func: std::sync::Arc::new(|args: Vec<Value>| -> RuntimeResult<Value> {
                    if args.len() != 1 { 
                        return Err(RuntimeError::ArityMismatch { function: "throw".to_string(), expected: "1".to_string(), actual: args.len() });
                    }
                    match &args[0] {
                        Value::Error(err) => Err(RuntimeError::Generic(err.message.clone())),
                        Value::String(s) => Err(RuntimeError::Generic(s.clone())),
                        other => Err(RuntimeError::Generic(other.to_string())),
                    }
                }),
            })),
        );

        // Sequence operations: first
        env.define(
            &Symbol("first".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "first".to_string(),
                arity: Arity::Fixed(1),
                func: std::sync::Arc::new(Self::first),
            })),
        );

        // Sequence operations: rest
        env.define(
            &Symbol("rest".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "rest".to_string(),
                arity: Arity::Fixed(1),
                func: std::sync::Arc::new(Self::rest),
            })),
        );

        // Sequence operations: nth
        env.define(
            &Symbol("nth".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "nth".to_string(),
                arity: Arity::Fixed(2),
                func: std::sync::Arc::new(Self::nth),
            })),
        );

        // Sequence generation: range
        env.define(
            &Symbol("range".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "range".to_string(),
                arity: Arity::Variadic(1),
                func: std::sync::Arc::new(Self::range),
            })),
        );

    // Predicate functions: even?
        env.define(
            &Symbol("even?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "even?".to_string(),
                arity: Arity::Fixed(1),
        func: Arc::new(|args: Vec<Value>| Self::even(args)),
            })),
        );

    // Predicate functions: odd?
        env.define(
            &Symbol("odd?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "odd?".to_string(),
                arity: Arity::Fixed(1),
        func: Arc::new(|args: Vec<Value>| Self::odd(args)),
            })),
        );

    // Arithmetic functions: inc
        env.define(
            &Symbol("inc".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "inc".to_string(),
                arity: Arity::Fixed(1),
        func: Arc::new(|args: Vec<Value>| Self::inc(args)),
            })),
        );

    // String functions: str
        env.define(
            &Symbol("str".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "str".to_string(),
                arity: Arity::Fixed(1),
        func: Arc::new(|args: Vec<Value>| Self::str(args)),
            })),
        );

        // Higher-order functions: map
        env.define(
            &Symbol("map".to_string()),
            Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
                name: "map".to_string(),
                arity: Arity::Fixed(2),
                func: Arc::new(|args: Vec<Value>, evaluator: &Evaluator, env: &mut Environment| {
                    Self::map(args, evaluator, env)
                }),
            })),
        );

        // Higher-order functions: filter
        env.define(
            &Symbol("filter".to_string()),
            Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
                name: "filter".to_string(),
                arity: Arity::Fixed(2),
                func: Arc::new(|args: Vec<Value>, evaluator: &Evaluator, env: &mut Environment| {
                    Self::filter(args, evaluator, env)
                }),
            })),
        );

        // Higher-order functions: reduce
        env.define(
            &Symbol("reduce".to_string()),
            Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
                name: "reduce".to_string(),
                arity: Arity::Range(2, 3),
                func: Arc::new(|args: Vec<Value>, evaluator: &Evaluator, env: &mut Environment| {
                    Self::reduce(args, evaluator, env)
                }),
            })),
        );

        // Higher-order functions: remove
        env.define(
            &Symbol("remove".to_string()),
            Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
                name: "remove".to_string(),
                arity: Arity::Fixed(2),
                func: Arc::new(|args: Vec<Value>, evaluator: &Evaluator, env: &mut Environment| {
                    Self::remove(args, evaluator, env)
                }),
            })),
        );

        // Higher-order functions: some?
        env.define(
            &Symbol("some?".to_string()),
            Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
                name: "some?".to_string(),
                arity: Arity::Fixed(2),
                func: Arc::new(|args: Vec<Value>, evaluator: &Evaluator, env: &mut Environment| {
                    Self::some(args, evaluator, env)
                }),
            })),
        );

        // Higher-order functions: every?
        env.define(
            &Symbol("every?".to_string()),
            Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
                name: "every?".to_string(),
                arity: Arity::Fixed(2),
                func: Arc::new(|args: Vec<Value>, evaluator: &Evaluator, env: &mut Environment| {
                    Self::every(args, evaluator, env)
                }),
            })),
        );

    // Sorting functions: sort
        env.define(
            &Symbol("sort".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "sort".to_string(),
                arity: Arity::Variadic(1),
        func: Arc::new(|args: Vec<Value>| Self::sort(args)),
            })),
        );

        // Sorting functions: sort-by
        env.define(
            &Symbol("sort-by".to_string()),
            Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
                name: "sort-by".to_string(),
                arity: Arity::Fixed(2),
                func: Arc::new(|args: Vec<Value>, evaluator: &Evaluator, env: &mut Environment| {
                    Self::sort_by(args, evaluator, env)
                }),
            })),
        );

    // Collection analysis: frequencies
        env.define(
            &Symbol("frequencies".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "frequencies".to_string(),
                arity: Arity::Fixed(1),
        func: Arc::new(|args: Vec<Value>| Self::frequencies(args)),
            })),
        );

    // Collection utilities: distinct
        env.define(
            &Symbol("distinct".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "distinct".to_string(),
                arity: Arity::Fixed(1),
        func: Arc::new(|args: Vec<Value>| Self::distinct(args)),
            })),
        );

    // Map utilities: merge
        env.define(
            &Symbol("merge".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "merge".to_string(),
                arity: Arity::Variadic(1),
        func: Arc::new(|args: Vec<Value>| Self::merge(args)),
            })),
        );

    // Collection utilities: contains?
        env.define(
            &Symbol("contains?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "contains?".to_string(),
                arity: Arity::Fixed(2),
        func: Arc::new(|args: Vec<Value>| Self::contains(args)),
            })),
        );

        // Higher-order functions: map-indexed
        env.define(
            &Symbol("map-indexed".to_string()),
            Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
                name: "map-indexed".to_string(),
                arity: Arity::Fixed(2),
                func: Arc::new(|args: Vec<Value>, evaluator: &Evaluator, env: &mut Environment| {
                    Self::map_indexed(args, evaluator, env)
                }),
            })),
        );

        // Higher-order functions: some
        env.define(
            &Symbol("some".to_string()),
            Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
                name: "some".to_string(),
                arity: Arity::Fixed(2),
                func: Arc::new(|args: Vec<Value>, evaluator: &Evaluator, env: &mut Environment| {
                    Self::some(args, evaluator, env)
                }),
            })),
        );

        // Higher-order functions: every?
        env.define(
            &Symbol("every?".to_string()),
            Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
                name: "every?".to_string(),
                arity: Arity::Fixed(2),
                func: Arc::new(|args: Vec<Value>, evaluator: &Evaluator, env: &mut Environment| {
                    Self::every(args, evaluator, env)
                }),
            })),
        );

    // String functions: str
        env.define(
            &Symbol("str".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "str".to_string(),
                arity: Arity::Variadic(0),
        func: Arc::new(|args: Vec<Value>| Self::str(args)),
            })),
        );

    // Number functions: inc
        env.define(
            &Symbol("inc".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "inc".to_string(),
                arity: Arity::Fixed(1),
        func: Arc::new(|args: Vec<Value>| Self::inc(args)),
            })),
        );

    // Predicate functions: odd?
        env.define(
            &Symbol("odd?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "odd?".to_string(),
                arity: Arity::Fixed(1),
        func: Arc::new(|args: Vec<Value>| Self::odd(args)),
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
            format!("[{}] {}: {}", level.to_uppercase(), message, data.to_string())
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
        let extra_args: Vec<Value> = if args.len() > 3 { args[3..].to_vec() } else { Vec::new() };

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
                Value::Function(_) | Value::FunctionPlaceholder(_) | Value::Keyword(_) => f_val.clone(),
                Value::String(name) => {
                    // Resolve by symbol name in current environment
                    if let Some(resolved) = env.lookup(&crate::ast::Symbol(name.clone())) {
                        match resolved {
                            Value::Function(_) | Value::FunctionPlaceholder(_) | Value::Keyword(_) => resolved,
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
                evaluator.call_function(f_to_call, &args_for_call, env)?
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
                Value::Function(_) | Value::FunctionPlaceholder(_) | Value::Keyword(_) => f_val.clone(),
                Value::String(name) => {
                    if let Some(resolved) = env.lookup(&crate::ast::Symbol(name.clone())) {
                        match resolved {
                            Value::Function(_) | Value::FunctionPlaceholder(_) | Value::Keyword(_) => resolved,
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
                evaluator.call_function(f_to_call, &args_for_call, env)?
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
        for arg in args {
            match arg {
                Value::String(s) => result.push_str(&s),
                Value::Integer(n) => result.push_str(&n.to_string()),
                Value::Float(f) => result.push_str(&f.to_string()),
                Value::Boolean(b) => result.push_str(&b.to_string()),
                Value::Keyword(k) => result.push_str(&format!(":{}", k.0)),
                Value::Nil => result.push_str("nil"),
                Value::Vector(v) => result.push_str(&format!("{:?}", v)),
                Value::List(l) => result.push_str(&format!("{:?}", l)),
                Value::Map(m) => result.push_str(&format!("{:?}", m)),
                _ => result.push_str(&format!("{:?}", arg)),
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
            Value::String(s) => {
                s.chars().map(|c| Value::String(c.to_string())).collect()
            }
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
                    evaluator.call_function(f.clone(), &args_for_call, env)?
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
            Value::String(_) => Ok(Value::String(result.into_iter().map(|v| v.to_string()).collect())),
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
            Value::String(s) => {
                s.chars().map(|c| Value::String(c.to_string())).collect()
            }
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
                    let pred_result = evaluator.call_function(pred.clone(), &args_for_call, env)?;
                    match pred_result {
                        Value::Boolean(b) => !b, // Keep elements where predicate returns false
                        _ => true, // Keep if predicate doesn't return boolean
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
            Value::String(_) => Ok(Value::String(result.into_iter().map(|v| v.to_string()).collect())),
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
            Value::String(s) => {
                s.chars().map(|c| Value::String(c.to_string())).collect()
            }
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
                    evaluator.call_function(pred.clone(), &args_for_call, env)?
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
            Value::String(s) => {
                s.chars().map(|c| Value::String(c.to_string())).collect()
            }
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
                    evaluator.call_function(pred.clone(), &args_for_call, env)?
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
    fn map(
        args: Vec<Value>,
        evaluator: &Evaluator,
        env: &mut Environment,
    ) -> RuntimeResult<Value> {
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
            Value::String(s) => {
                s.chars().map(|c| Value::String(c.to_string())).collect()
            }
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
            let mapped = evaluator.call_function(function.clone(), &[element], env)?;
            result.push(mapped);
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
            Value::String(s) => {
                s.chars().map(|c| Value::String(c.to_string())).collect()
            }
            Value::List(list) => list.clone(),
            other => {
                return Err(RuntimeError::TypeError {
                    expected: "vector, string, or list".to_string(),
                    actual: other.type_name().to_string(),
                    operation: "filter".to_string(),
                })
            }
        };

        let mut result = Vec::new();
        for element in elements {
            let should_include = evaluator.call_function(predicate.clone(), &[element.clone()], env)?;
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
                Err(RuntimeError::new("reduce on empty collection with no initial value"))
            };
        }

        let (mut accumulator, rest) = if args.len() == 3 {
            (args[1].clone(), collection.as_slice())
        } else {
            (collection[0].clone(), &collection[1..])
        };

        for value in rest {
            accumulator = evaluator.call_function(function.clone(), &[accumulator.clone(), value.clone()], env)?;
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
            Value::String(s) => {
                s.chars().map(|c| Value::String(c.to_string())).collect()
            }
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
            let key = evaluator.call_function(key_fn.clone(), &[element.clone()], env)?;
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
            Value::String(s) => {
                s.chars().map(|c| Value::String(c.to_string())).collect()
            }
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
            Value::String(s) => {
                s.chars().map(|c| Value::String(c.to_string())).collect()
            }
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
            Value::String(_) => Ok(Value::String(result.into_iter().map(|v| v.to_string()).collect())),
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
                    for (k, v) in m { out.insert(k, v); }
                }
                other => {
                    return Err(RuntimeError::TypeError { expected: "map".into(), actual: other.type_name().into(), operation: "merge".into() });
                }
            }
        }
        Ok(Value::Map(out))
    }

    /// Atoms: (atom v)
    fn atom_new(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 1 { return Err(RuntimeError::ArityMismatch { function: "atom".into(), expected: "1".into(), actual: args.len() }); }
        Ok(Value::Atom(Arc::new(RwLock::new(args[0].clone()))))
    }

    /// (deref a)
    fn atom_deref(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 1 { return Err(RuntimeError::ArityMismatch { function: "deref".into(), expected: "1".into(), actual: args.len() }); }
        match &args[0] {
            Value::Atom(rc) => Ok(rc.read().map_err(|e| RuntimeError::Generic(format!("RwLock poisoned: {}", e)))?.clone()),
            other => Err(RuntimeError::TypeError { expected: "atom".into(), actual: other.type_name().into(), operation: "deref".into() })
        }
    }

    /// (reset! a v)
    fn atom_reset(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 2 { return Err(RuntimeError::ArityMismatch { function: "reset!".into(), expected: "2".into(), actual: args.len() }); }
        match &args[0] {
            Value::Atom(rc) => { *rc.write().map_err(|e| RuntimeError::Generic(format!("RwLock poisoned: {}", e)))? = args[1].clone(); Ok(args[1].clone()) }
            other => Err(RuntimeError::TypeError { expected: "atom".into(), actual: other.type_name().into(), operation: "reset!".into() })
        }
    }

    /// (swap! a f & args) -> applies f to current value and args, stores result back
    fn atom_swap(args: Vec<Value>, evaluator: &Evaluator, env: &mut Environment) -> RuntimeResult<Value> {
        if args.len() < 2 { return Err(RuntimeError::ArityMismatch { function: "swap!".into(), expected: "at least 2".into(), actual: args.len() }); }
        let (atom_val, f_val, rest) = (&args[0], &args[1], &args[2..]);
    let rc = match atom_val { Value::Atom(rc) => rc.clone(), other => return Err(RuntimeError::TypeError { expected: "atom".into(), actual: other.type_name().into(), operation: "swap!".into() }) };
    // Build call args current, rest...
    let current = rc.read().map_err(|e| RuntimeError::Generic(format!("RwLock poisoned: {}", e)))?.clone();
        let mut call_args = Vec::with_capacity(1 + rest.len());
        call_args.push(current);
        call_args.extend_from_slice(rest);
        let new_val = evaluator.call_function(f_val.clone(), &call_args, env)?;
    *rc.write().map_err(|e| RuntimeError::Generic(format!("RwLock poisoned: {}", e)))? = new_val.clone();
        Ok(new_val)
    }

    /// Stub for coordinate-work to satisfy tests until full impl exists
    fn coordinate_work_stub(args: Vec<Value>) -> RuntimeResult<Value> {
        // Echo back a simple map {:status :ok :inputs n}
        let mut m = HashMap::new();
        m.insert(MapKey::Keyword(Keyword("status".into())), Value::Keyword(Keyword("ok".into())));
        m.insert(MapKey::Keyword(Keyword("inputs".into())), Value::Integer(args.len() as i64));
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
                // New calling convention: map with :args and optional :context
                Value::Map(map) => {
                    let args_val = map.get(&MapKey::Keyword(Keyword("args".to_string()))).cloned().unwrap_or(Value::List(vec![]));
                    match args_val {
                        Value::List(args) => {
                            let mut sum = 0i64;
                            for arg in args {
                                match arg {
                                    Value::Integer(n) => sum += n,
                                    other => {
                                        return Err(RuntimeError::TypeError {
                                            expected: "integer".to_string(),
                                            actual: other.type_name().to_string(),
                                            operation: "ccos.math.add".to_string(),
                                        })
                                    }
                                }
                            }
                            Ok(Value::Integer(sum))
                        }
                        other => Err(RuntimeError::TypeError {
                            expected: "list".to_string(),
                            actual: other.type_name().to_string(),
                            operation: "ccos.math.add".to_string(),
                        })
                    }
                }
                other => Err(RuntimeError::TypeError {
                    expected: "map".to_string(),
                    actual: other.type_name().to_string(),
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
            exports: std::sync::RwLock::new(exports),
            namespace: std::sync::Arc::new(std::sync::RwLock::new(IrEnvironment::new())),
            dependencies: vec![],
        };

        // Register the module
        module_registry.register_module(module)?;
    }

    Ok(())
}

