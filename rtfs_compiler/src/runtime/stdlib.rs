// Standard library implementation for RTFS
// Contains all built-in functions and tool interfaces

use std::collections::HashMap;
use crate::ast::{Symbol, MapKey, Expression};
use crate::runtime::{Value, RuntimeError, RuntimeResult, Environment};
use crate::runtime::values::{Function, Arity};

pub struct StandardLibrary;

impl StandardLibrary {
    /// Create a new environment with all standard library functions loaded
    pub fn create_global_environment() -> Environment {
        let mut env = Environment::new();
        
        // Load all built-in functions
        Self::load_arithmetic_functions(&mut env);
        Self::load_comparison_functions(&mut env);
        Self::load_boolean_functions(&mut env);
        Self::load_string_functions(&mut env);
        Self::load_collection_functions(&mut env);
        Self::load_type_predicate_functions(&mut env);
        Self::load_tool_functions(&mut env);
        Self::load_agent_functions(&mut env);
        Self::load_task_functions(&mut env);
        
        env
    }
    
    /// Load arithmetic functions (+, -, *, /)
    fn load_arithmetic_functions(env: &mut Environment) {
        // Addition (+)
        env.define(&Symbol("+".to_string()), Value::Function(Function::Builtin {
            name: "+".to_string(),
            arity: Arity::AtLeast(1),
            func: Self::add,
        }));
        
        // Subtraction (-)
        env.define(&Symbol("-".to_string()), Value::Function(Function::Builtin {
            name: "-".to_string(),
            arity: Arity::AtLeast(1),
            func: Self::subtract,
        }));
        
        // Multiplication (*)
        env.define(&Symbol("*".to_string()), Value::Function(Function::Builtin {
            name: "*".to_string(),
            arity: Arity::AtLeast(1),
            func: Self::multiply,
        }));
          // Division (/)
        env.define(&Symbol("/".to_string()), Value::Function(Function::Builtin {
            name: "/".to_string(),
            arity: Arity::AtLeast(1),
            func: Self::divide,
        }));
        
        // Max
        env.define(&Symbol("max".to_string()), Value::Function(Function::Builtin {
            name: "max".to_string(),
            arity: Arity::AtLeast(1),
            func: Self::max_value,
        }));
        
        // Min
        env.define(&Symbol("min".to_string()), Value::Function(Function::Builtin {
            name: "min".to_string(),
            arity: Arity::AtLeast(1),
            func: Self::min_value,
        }));
    }
    
    /// Load comparison functions (=, !=, >, <, >=, <=)
    fn load_comparison_functions(env: &mut Environment) {
        env.define(&Symbol("=".to_string()), Value::Function(Function::Builtin {
            name: "=".to_string(),
            arity: Arity::AtLeast(1),
            func: Self::equal,
        }));
        
        env.define(&Symbol("!=".to_string()), Value::Function(Function::Builtin {
            name: "!=".to_string(),
            arity: Arity::Exact(2),
            func: Self::not_equal,
        }));
        
        env.define(&Symbol(">".to_string()), Value::Function(Function::Builtin {
            name: ">".to_string(),
            arity: Arity::Exact(2),
            func: Self::greater_than,
        }));
        
        env.define(&Symbol("<".to_string()), Value::Function(Function::Builtin {
            name: "<".to_string(),
            arity: Arity::Exact(2),
            func: Self::less_than,
        }));
        
        env.define(&Symbol(">=".to_string()), Value::Function(Function::Builtin {
            name: ">=".to_string(),
            arity: Arity::Exact(2),
            func: Self::greater_equal,
        }));
        
        env.define(&Symbol("<=".to_string()), Value::Function(Function::Builtin {
            name: "<=".to_string(),
            arity: Arity::Exact(2),
            func: Self::less_equal,
        }));
    }
    
    /// Load boolean functions (and, or, not)
    fn load_boolean_functions(env: &mut Environment) {
        env.define(&Symbol("and".to_string()), Value::Function(Function::Builtin {
            name: "and".to_string(),
            arity: Arity::Any,
            func: Self::and,
        }));
        
        env.define(&Symbol("or".to_string()), Value::Function(Function::Builtin {
            name: "or".to_string(),
            arity: Arity::Any,
            func: Self::or,
        }));
        
        env.define(&Symbol("not".to_string()), Value::Function(Function::Builtin {
            name: "not".to_string(),
            arity: Arity::Exact(1),
            func: Self::not,
        }));
    }
    
    /// Load string functions (str, string-length, substring)
    fn load_string_functions(env: &mut Environment) {
        env.define(&Symbol("str".to_string()), Value::Function(Function::Builtin {
            name: "str".to_string(),
            arity: Arity::Any,
            func: Self::str,
        }));
        
        env.define(&Symbol("string-length".to_string()), Value::Function(Function::Builtin {
            name: "string-length".to_string(),
            arity: Arity::Exact(1),
            func: Self::string_length,
        }));
        
        env.define(&Symbol("substring".to_string()), Value::Function(Function::Builtin {
            name: "substring".to_string(),
            arity: Arity::Range(2, 3),
            func: Self::substring,
        }));
    }
    
    /// Load collection functions (get, assoc, dissoc, count, conj, vector, map)
    fn load_collection_functions(env: &mut Environment) {
        env.define(&Symbol("get".to_string()), Value::Function(Function::Builtin {
            name: "get".to_string(),
            arity: Arity::Range(2, 3),
            func: Self::get,
        }));
        
        env.define(&Symbol("assoc".to_string()), Value::Function(Function::Builtin {
            name: "assoc".to_string(),
            arity: Arity::AtLeast(3),
            func: Self::assoc,
        }));
        
        env.define(&Symbol("dissoc".to_string()), Value::Function(Function::Builtin {
            name: "dissoc".to_string(),
            arity: Arity::AtLeast(2),
            func: Self::dissoc,
        }));
        
        env.define(&Symbol("count".to_string()), Value::Function(Function::Builtin {
            name: "count".to_string(),
            arity: Arity::Exact(1),
            func: Self::count,
        }));
        
        env.define(&Symbol("conj".to_string()), Value::Function(Function::Builtin {
            name: "conj".to_string(),
            arity: Arity::AtLeast(2),
            func: Self::conj,
        }));
        
        env.define(&Symbol("vector".to_string()), Value::Function(Function::Builtin {
            name: "vector".to_string(),
            arity: Arity::Any,
            func: Self::vector,
        }));
        env.define(&Symbol("hash-map".to_string()), Value::Function(Function::Builtin {
            name: "hash-map".to_string(),
            arity: Arity::Any,
            func: Self::hash_map,
        }));          
        env.define(&Symbol("map".to_string()), Value::Function(Function::BuiltinWithEvaluator {
            name: "map".to_string(),
            arity: Arity::AtLeast(2),
            func: Self::map_with_evaluator,
        }));
          env.define(&Symbol("filter".to_string()), Value::Function(Function::BuiltinWithEvaluator {
            name: "filter".to_string(),
            arity: Arity::Exact(2),
            func: Self::filter_with_evaluator,
        }));
          env.define(&Symbol("reduce".to_string()), Value::Function(Function::Builtin {
            name: "reduce".to_string(),
            arity: Arity::Range(2, 3),
            func: Self::reduce,
        }));

        // List functions
        env.define(&Symbol("empty?".to_string()), Value::Function(Function::Builtin {
            name: "empty?".to_string(),
            arity: Arity::Exact(1),
            func: Self::empty_p,
        }));

        env.define(&Symbol("cons".to_string()), Value::Function(Function::Builtin {
            name: "cons".to_string(),
            arity: Arity::Exact(2),
            func: Self::cons,
        }));

        env.define(&Symbol("first".to_string()), Value::Function(Function::Builtin {
            name: "first".to_string(),
            arity: Arity::Exact(1),
            func: Self::first,
        }));

        env.define(&Symbol("rest".to_string()), Value::Function(Function::Builtin {
            name: "rest".to_string(),
            arity: Arity::Exact(1),
            func: Self::rest,
        }));
        
        // Additional collection functions
        env.define(&Symbol("get-in".to_string()), Value::Function(Function::Builtin {
            name: "get-in".to_string(),
            arity: Arity::Range(2, 3),
            func: Self::get_in,
        }));
        
        env.define(&Symbol("partition".to_string()), Value::Function(Function::Builtin {
            name: "partition".to_string(),
            arity: Arity::Exact(2),
            func: Self::partition,
        }));
    }
      /// Load type predicate functions (int?, float?, string?, etc.)
    fn load_type_predicate_functions(env: &mut Environment) {
        env.define(&Symbol("int?".to_string()), Value::Function(Function::Builtin {
            name: "int?".to_string(),
            arity: Arity::Exact(1),
            func: Self::int_p,
        }));
        
        env.define(&Symbol("float?".to_string()), Value::Function(Function::Builtin {
            name: "float?".to_string(),
            arity: Arity::Exact(1),
            func: Self::float_p,
        }));
        
        env.define(&Symbol("number?".to_string()), Value::Function(Function::Builtin {
            name: "number?".to_string(),
            arity: Arity::Exact(1),
            func: Self::number_p,
        }));
        
        env.define(&Symbol("string?".to_string()), Value::Function(Function::Builtin {
            name: "string?".to_string(),
            arity: Arity::Exact(1),
            func: Self::string_p,
        }));
        
        env.define(&Symbol("bool?".to_string()), Value::Function(Function::Builtin {
            name: "bool?".to_string(),
            arity: Arity::Exact(1),
            func: Self::nil_p,
        }));
        
        env.define(&Symbol("nil?".to_string()), Value::Function(Function::Builtin {
            name: "nil?".to_string(),
            arity: Arity::Exact(1),
            func: Self::nil_p,
        }));
        
        env.define(&Symbol("map?".to_string()), Value::Function(Function::Builtin {
            name: "map?".to_string(),
            arity: Arity::Exact(1),
            func: Self::map_p,
        }));
        
        env.define(&Symbol("vector?".to_string()), Value::Function(Function::Builtin {
            name: "vector?".to_string(),
            arity: Arity::Exact(1),
            func: Self::vector_p,
        }));
        
        env.define(&Symbol("keyword?".to_string()), Value::Function(Function::Builtin {
            name: "keyword?".to_string(),
            arity: Arity::Exact(1),
            func: Self::keyword_p,
        }));
        
        env.define(&Symbol("symbol?".to_string()), Value::Function(Function::Builtin {
            name: "symbol?".to_string(),
            arity: Arity::Exact(1),
            func: Self::symbol_p,
        }));
        
        env.define(&Symbol("fn?".to_string()), Value::Function(Function::Builtin {
            name: "fn?".to_string(),
            arity: Arity::Exact(1),
            func: Self::fn_p,
        }));
    }
    
    /// Load tool interface functions (placeholder implementations)
    fn load_tool_functions(env: &mut Environment) {
        // For now, we'll create placeholder implementations
        // These would need to be implemented with actual I/O, networking, etc.
          env.define(&Symbol("tool:log".to_string()), Value::Function(Function::Builtin {
            name: "tool:log".to_string(),
            arity: Arity::Any, // Changed from Exact(1) to Any to match implementation
            func: Self::tool_log,
        }));
        
        env.define(&Symbol("tool:print".to_string()), Value::Function(Function::Builtin {
            name: "tool:print".to_string(),
            arity: Arity::Any,
            func: Self::tool_print,
        }));
        
        env.define(&Symbol("tool:current-time".to_string()), Value::Function(Function::Builtin {
            name: "tool:current-time".to_string(),
            arity: Arity::Exact(0),
            func: Self::tool_current_time,
        }));
        
        env.define(&Symbol("tool:parse-json".to_string()), Value::Function(Function::Builtin {
            name: "tool:parse-json".to_string(),
            arity: Arity::Exact(1),
            func: Self::tool_parse_json,
        }));
        
        env.define(&Symbol("tool:serialize-json".to_string()), Value::Function(Function::Builtin {
            name: "tool:serialize-json".to_string(),
            arity: Arity::Exact(1),
            func: Self::tool_serialize_json,
        }));
        
        // Enhanced tool functions for resource management
        env.define(&Symbol("tool:open-file".to_string()), Value::Function(Function::Builtin {
            name: "tool:open-file".to_string(),
            arity: Arity::Range(1, 3),
            func: Self::tool_open_file,
        }));
        
        env.define(&Symbol("tool:read-line".to_string()), Value::Function(Function::Builtin {
            name: "tool:read-line".to_string(),
            arity: Arity::Exact(1),
            func: Self::tool_read_line,
        }));
        
        env.define(&Symbol("tool:write-line".to_string()), Value::Function(Function::Builtin {
            name: "tool:write-line".to_string(),
            arity: Arity::Exact(2),
            func: Self::tool_write_line,
        }));
        
        env.define(&Symbol("tool:close-file".to_string()), Value::Function(Function::Builtin {
            name: "tool:close-file".to_string(),
            arity: Arity::Exact(1),
            func: Self::tool_close_file,
        }));
        
        env.define(&Symbol("tool:get-env".to_string()), Value::Function(Function::Builtin {
            name: "tool:get-env".to_string(),
            arity: Arity::Range(1, 2),
            func: Self::tool_get_env,
        }));
        
        env.define(&Symbol("tool:http-fetch".to_string()), Value::Function(Function::Builtin {
            name: "tool:http-fetch".to_string(),
            arity: Arity::Range(1, 2),
            func: Self::tool_http_fetch,
        }));        env.define(&Symbol("tool:file-exists?".to_string()), Value::Function(Function::Builtin {
            name: "tool:file-exists?".to_string(),
            arity: Arity::Exact(1),
            func: Self::tool_file_exists_p,
        }));
        
        // Add convenience aliases without prefixes
        env.define(&Symbol("log".to_string()), Value::Function(Function::Builtin {
            name: "log".to_string(),
            arity: Arity::Any,
            func: Self::tool_log,
        }));
        
        env.define(&Symbol("current-time".to_string()), Value::Function(Function::Builtin {
            name: "current-time".to_string(),
            arity: Arity::Exact(0),
            func: Self::tool_current_time,
        }));
        
        env.define(&Symbol("parse-json".to_string()), Value::Function(Function::Builtin {
            name: "parse-json".to_string(),
            arity: Arity::Exact(1),
            func: Self::tool_parse_json,
        }));
        
        env.define(&Symbol("serialize-json".to_string()), Value::Function(Function::Builtin {
            name: "serialize-json".to_string(),
            arity: Arity::Exact(1),
            func: Self::tool_serialize_json,
        }));
        
        env.define(&Symbol("get-env".to_string()), Value::Function(Function::Builtin {
            name: "get-env".to_string(),
            arity: Arity::Exact(1), // Changed from Range(1, 2) to match implementation
            func: Self::tool_get_env,
        }));
        
        env.define(&Symbol("file-exists?".to_string()), Value::Function(Function::Builtin {
            name: "file-exists?".to_string(),
            arity: Arity::Exact(1),
            func: Self::tool_file_exists_p,
        }));}
    
    /// Load agent system functions
    fn load_agent_functions(env: &mut Environment) {
        // Agent discovery function
        env.define(&Symbol("discover-agents".to_string()), Value::Function(Function::Builtin {
            name: "discover-agents".to_string(),
            arity: Arity::Exact(0), // Changed to match implementation
            func: Self::discover_agents,
        }));          // Task coordination function
        env.define(&Symbol("task-coordination".to_string()), Value::Function(Function::Builtin {
            name: "task-coordination".to_string(),
            arity: Arity::AtLeast(1),
            func: Self::task_coordination,
        }));
          // Mathematical functions
        env.define(&Symbol("fact".to_string()), Value::Function(Function::Builtin {
            name: "fact".to_string(),
            arity: Arity::Exact(1),
            func: Self::factorial,
        }));
        
        // Add factorial alias for convenience
        env.define(&Symbol("factorial".to_string()), Value::Function(Function::Builtin {
            name: "factorial".to_string(),
            arity: Arity::Exact(1),
            func: Self::factorial,
        }));
        
        env.define(&Symbol("max".to_string()), Value::Function(Function::Builtin {
            name: "max".to_string(),
            arity: Arity::AtLeast(1),
            func: Self::max_value,
        }));
        
        env.define(&Symbol("min".to_string()), Value::Function(Function::Builtin {
            name: "min".to_string(),
            arity: Arity::AtLeast(1),
            func: Self::min_value,
        }));
        
        env.define(&Symbol("length".to_string()), Value::Function(Function::Builtin {
            name: "length".to_string(),
            arity: Arity::Exact(1),
            func: Self::length_value,
        }));
        
        // Advanced agent system functions
        env.define(&Symbol("discover-and-assess-agents".to_string()), Value::Function(Function::Builtin {
            name: "discover-and-assess-agents".to_string(),
            arity: Arity::Exact(0), // Changed to match implementation
            func: Self::discover_and_assess_agents,
        }));
        
        env.define(&Symbol("establish-system-baseline".to_string()), Value::Function(Function::Builtin {
            name: "establish-system-baseline".to_string(),
            arity: Arity::Exact(0), // Changed to match implementation
            func: Self::establish_system_baseline,
        }));
        
        // Tool functions for agent coordination
        env.define(&Symbol("tool:current-timestamp-ms".to_string()), Value::Function(Function::Builtin {
            name: "tool:current-timestamp-ms".to_string(),
            arity: Arity::Exact(0),
            func: Self::current_timestamp_ms,
        }));
        
        // Add alias without prefix for convenience
        env.define(&Symbol("current-timestamp-ms".to_string()), Value::Function(Function::Builtin {
            name: "current-timestamp-ms".to_string(),
            arity: Arity::Exact(0),
            func: Self::current_timestamp_ms,
        }));
    }

    /// Load task-related functions
    fn load_task_functions(env: &mut Environment) {        
        env.define(&Symbol("rtfs.task/current".to_string()), Value::Function(Function::BuiltinWithEvaluator {
            name: "rtfs.task/current".to_string(),
            arity: Arity::Exact(0),
            func: Self::task_current_with_evaluator,
        }));
    }

    // Helper for converting Value to MapKey
    fn value_to_map_key(value: &Value) -> RuntimeResult<MapKey> {
        match value {
            Value::String(s) => Ok(MapKey::String(s.clone())),
            Value::Keyword(k) => Ok(MapKey::Keyword(k.clone())),
            _ => Err(RuntimeError::TypeError {
                expected: "string or keyword".to_string(),
                actual: value.type_name().to_string(),
                operation: "map key".to_string(),
            }),
        }
    }

    // All other function implementations that were missing
    fn max_value(args: &[Value]) -> RuntimeResult<Value> {
        if args.is_empty() {
            return Err(RuntimeError::new("max requires at least one argument"));
        }
        let mut max_val = args[0].clone();
        for val in &args[1..] {
            let is_greater = match (val, &max_val) {
                (Value::Integer(a), Value::Integer(b)) => a > b,
                (Value::Float(a), Value::Float(b)) => a > b,
                (Value::Integer(a), Value::Float(b)) => *a as f64 > *b,
                (Value::Float(a), Value::Integer(b)) => *a > *b as f64,
                _ => return Err(RuntimeError::new("max can only compare numbers")),
            };
            if is_greater {
                max_val = val.clone();
            }
        }
        Ok(max_val)
    }

    fn min_value(args: &[Value]) -> RuntimeResult<Value> {
        if args.is_empty() {
            return Err(RuntimeError::new("min requires at least one argument"));
        }
        let mut min_val = args[0].clone();
        for val in &args[1..] {
            let is_less = match (val, &min_val) {
                (Value::Integer(a), Value::Integer(b)) => a < b,
                (Value::Float(a), Value::Float(b)) => a < b,
                (Value::Integer(a), Value::Float(b)) => (*a as f64) < *b,
                (Value::Float(a), Value::Integer(b)) => *a < (*b as f64),
                _ => return Err(RuntimeError::new("min can only compare numbers")),
            };
            if is_less {
                min_val = val.clone();
            }
        }
        Ok(min_val)
    }

    fn conj(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() < 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "conj".to_string(),
                expected: "at least 2".to_string(),
                actual: args.len(),
            });
        }
        let mut collection = args[0].clone();
        match &mut collection {
            Value::Vector(vec) => {
                for item in &args[1..] {
                    vec.push(item.clone());
                }
                Ok(collection)
            },
            Value::Map(map) => {
                if args.len() != 3 {
                    return Err(RuntimeError::ArityMismatch {
                        function: "conj".to_string(),
                        expected: "3 for maps".to_string(),
                        actual: args.len(),
                    });
                }
                let key = Self::value_to_map_key(&args[1])?;
                map.insert(key, args[2].clone());
                Ok(collection)
            },
            _ => Err(RuntimeError::TypeError {
                expected: "vector or map".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "conj".to_string(),
            }),
        }
    }    fn map_with_evaluator(args: &[Expression], evaluator: &crate::runtime::evaluator::Evaluator, env: &mut Environment) -> RuntimeResult<Value> {
        if args.len() < 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "map".to_string(),
                expected: "at least 2".to_string(),
                actual: args.len(),
            });
        }
        
        let func = evaluator.eval_expr(&args[0], env)?;
        
        // Evaluate all collections
        let mut collections = Vec::new();
        for i in 1..args.len() {
            let collection = evaluator.eval_expr(&args[i], env)?;
            match collection {
                Value::Vector(vec) => collections.push(vec),
                _ => return Err(RuntimeError::TypeError {
                    expected: "vector".to_string(),
                    actual: collection.type_name().to_string(),
                    operation: "map".to_string(),
                }),
            }
        }
        
        // Find the shortest collection length to determine how many iterations
        let min_length = collections.iter().map(|v| v.len()).min().unwrap_or(0);
        
        let mut results = Vec::new();
        for i in 0..min_length {
            // Collect arguments for this iteration from all collections
            let mut args_for_call = Vec::new();
            for collection in &collections {
                args_for_call.push(collection[i].clone());
            }
            
            let result = evaluator.call_function(func.clone(), &args_for_call, env)?;
            results.push(result);
        }
        
        Ok(Value::Vector(results))
    }

    fn filter_with_evaluator(args: &[Expression], evaluator: &crate::runtime::evaluator::Evaluator, env: &mut Environment) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "filter".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }
        
        let func = evaluator.eval_expr(&args[0], env)?;
        let collection = evaluator.eval_expr(&args[1], env)?;
        
        match collection {
            Value::Vector(vec) => {
                let mut results = Vec::new();
                for item in vec {
                    let result = evaluator.call_function(func.clone(), &[item.clone()], env)?;
                    if let Value::Boolean(true) = result {
                        results.push(item);
                    } else if !matches!(result, Value::Boolean(false) | Value::Nil) {
                        results.push(item);
                    }
                }
                Ok(Value::Vector(results))
            },
            _ => Err(RuntimeError::TypeError {
                expected: "vector".to_string(),
                actual: collection.type_name().to_string(),
                operation: "filter".to_string(),
            }),
        }
    }    fn reduce(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() < 2 || args.len() > 3 {
            return Err(RuntimeError::new("reduce requires 2 or 3 arguments"));
        }

        let function = &args[0];

        let collection_arg_index = args.len() - 1;
        let collection_val = &args[collection_arg_index];

        let collection = match collection_val {
            Value::Vector(v) => v.clone(),
            _ => return Err(RuntimeError::new("reduce expects a vector as its last argument")),
        };

        if collection.is_empty() {
            return if args.len() == 3 {
                Ok(args[1].clone()) // initial value
            } else {
                Err(RuntimeError::new("reduce on empty collection with no initial value"))
            };
        }

        let (mut accumulator, rest) = if args.len() == 3 {
            (args[1].clone(), collection.as_slice())
        } else {
            (collection[0].clone(), &collection[1..])
        };        let func_ptr = match function {
            Value::Function(Function::Builtin { func, .. }) => func.clone(),
            _ => return Err(RuntimeError::new("reduce requires a builtin function")),
        };

        for value in rest {
            let func_args = vec![accumulator.clone(), value.clone()];
            accumulator = func_ptr(&func_args)?;
        }

        Ok(accumulator)
    }

    fn empty_p(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "empty?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        match &args[0] {
            Value::Vector(v) => Ok(Value::Boolean(v.is_empty())),
            Value::Map(m) => Ok(Value::Boolean(m.is_empty())),
            Value::String(s) => Ok(Value::Boolean(s.is_empty())),
            Value::Nil => Ok(Value::Boolean(true)),
            _ => Ok(Value::Boolean(false)),
        }
    }

    fn cons(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "cons".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }
        match &args[1] {
            Value::Vector(v) => {
                let mut new_vec = vec![args[0].clone()];
                new_vec.extend_from_slice(v);
                Ok(Value::Vector(new_vec))
            },
            _ => Err(RuntimeError::TypeError {
                expected: "vector".to_string(),
                actual: args[1].type_name().to_string(),
                operation: "cons".to_string(),
            }),
        }
    }

    fn first(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "first".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        match &args[0] {
            Value::Vector(v) => Ok(v.first().cloned().unwrap_or(Value::Nil)),
            _ => Err(RuntimeError::TypeError {
                expected: "vector".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "first".to_string(),
            }),
        }
    }

    fn rest(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "rest".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        match &args[0] {
            Value::Vector(v) => {
                if v.is_empty() {
                    Ok(Value::Vector(vec![]))
                } else {
                    Ok(Value::Vector(v[1..].to_vec()))
                }
            },
            _ => Err(RuntimeError::TypeError {
                expected: "vector".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "rest".to_string(),
            }),
        }
    }

    fn get_in(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() < 2 || args.len() > 3 {
            return Err(RuntimeError::ArityMismatch {
                function: "get-in".to_string(),
                expected: "2-3".to_string(),
                actual: args.len(),
            });
        }
        
        let collection = &args[0];
        let path = &args[1];
        let default = if args.len() == 3 { args[2].clone() } else { Value::Nil };
        
        let path_vec = match path {
            Value::Vector(v) => v,
            _ => return Err(RuntimeError::TypeError {
                expected: "vector".to_string(),
                actual: path.type_name().to_string(),
                operation: "get-in".to_string(),
            }),
        };
        
        let mut current = collection.clone();
        for key in path_vec {
            match (&current, key) {
                (Value::Map(map), key) => {
                    let map_key = Self::value_to_map_key(key)?;
                    current = map.get(&map_key).cloned().unwrap_or(Value::Nil);
                    if matches!(current, Value::Nil) {
                        return Ok(default);
                    }
                },
                (Value::Vector(vec), Value::Integer(index)) => {
                    let idx = *index as usize;
                    current = vec.get(idx).cloned().unwrap_or(Value::Nil);
                    if matches!(current, Value::Nil) {
                        return Ok(default);
                    }
                },
                _ => return Ok(default),
            }
        }
        
        Ok(current)
    }

    fn partition(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "partition".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }
        
        let size = match &args[0] {
            Value::Integer(n) => *n as usize,
            _ => return Err(RuntimeError::TypeError {
                expected: "integer".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "partition".to_string(),
            }),
        };
        
        if size == 0 {
            return Err(RuntimeError::InvalidArgument(
                "Partition size must be greater than 0".to_string()
            ));
        }
        
        let collection = match &args[1] {
            Value::Vector(v) => v,
            _ => return Err(RuntimeError::TypeError {
                expected: "vector".to_string(),
                actual: args[1].type_name().to_string(),
                operation: "partition".to_string(),
            }),
        };
        
        let mut partitions = Vec::new();
        for chunk in collection.chunks(size) {
            if chunk.len() == size {
                partitions.push(Value::Vector(chunk.to_vec()));
            }
        }
        
        Ok(Value::Vector(partitions))
    }
}

// Implementation of built-in functions
impl StandardLibrary {
    // Arithmetic functions
    fn add(args: &[Value]) -> RuntimeResult<Value> {
        if args.is_empty() {
            return Ok(Value::Integer(0));
        }
        
        let mut result_int: Option<i64> = None;
        let mut result_float: Option<f64> = None;
        
        for arg in args {
            match arg {
                Value::Integer(n) => {
                    if let Some(float_acc) = result_float {
                        result_float = Some(float_acc + *n as f64);
                    } else if let Some(int_acc) = result_int {
                        result_int = Some(int_acc + n);
                    } else {
                        result_int = Some(*n);
                    }
                },
                Value::Float(f) => {
                    let current = result_float.unwrap_or(result_int.unwrap_or(0) as f64);
                    result_float = Some(current + f);
                    result_int = None;
                },
                _ => return Err(RuntimeError::TypeError {
                    expected: "number".to_string(),
                    actual: arg.type_name().to_string(),
                    operation: "+".to_string(),
                }),
            }
        }
        
        if let Some(f) = result_float {
            Ok(Value::Float(f))
        } else {
            Ok(Value::Integer(result_int.unwrap_or(0)))
        }
    }
    
    fn subtract(args: &[Value]) -> RuntimeResult<Value> {
        if args.is_empty() {
            return Err(RuntimeError::ArityMismatch {
                function: "-".to_string(),
                expected: "at least 1".to_string(),
                actual: 0,
            });
        }
        
        if args.len() == 1 {
            // Negation
            match &args[0] {
                Value::Integer(n) => Ok(Value::Integer(-n)),
                Value::Float(f) => Ok(Value::Float(-f)),
                _ => Err(RuntimeError::TypeError {
                    expected: "number".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "-".to_string(),
                }),
            }
        } else {
            // Subtraction
            let mut result = match &args[0] {
                Value::Integer(n) => (*n as f64, false),
                Value::Float(f) => (*f, true),
                _ => return Err(RuntimeError::TypeError {
                    expected: "number".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "-".to_string(),
                }),
            };
            
            for arg in &args[1..] {
                match arg {
                    Value::Integer(n) => {
                        result.0 -= *n as f64;
                    },
                    Value::Float(f) => {
                        result.0 -= f;
                        result.1 = true;
                    },
                    _ => return Err(RuntimeError::TypeError {
                        expected: "number".to_string(),
                        actual: arg.type_name().to_string(),
                        operation: "-".to_string(),
                    }),
                }
            }
            
            if result.1 {
                Ok(Value::Float(result.0))
            } else {
                Ok(Value::Integer(result.0 as i64))
            }
        }
    }
    
    fn multiply(args: &[Value]) -> RuntimeResult<Value> {
        if args.is_empty() {
            return Ok(Value::Integer(1));
        }
        
        let mut result_int: Option<i64> = None;
        let mut result_float: Option<f64> = None;
        
        for arg in args {
            match arg {
                Value::Integer(n) => {
                    if let Some(float_acc) = result_float {
                        result_float = Some(float_acc * *n as f64);
                    } else if let Some(int_acc) = result_int {
                        result_int = Some(int_acc * n);
                    } else {
                        result_int = Some(*n);
                    }
                },
                Value::Float(f) => {
                    let current = result_float.unwrap_or(result_int.unwrap_or(1) as f64);
                    result_float = Some(current * f);
                    result_int = None;
                },
                _ => return Err(RuntimeError::TypeError {
                    expected: "number".to_string(),
                    actual: arg.type_name().to_string(),
                    operation: "*".to_string(),
                }),
            }
        }
        
        if let Some(f) = result_float {
            Ok(Value::Float(f))
        } else {
            Ok(Value::Integer(result_int.unwrap_or(1)))
        }
    }
    
    fn divide(args: &[Value]) -> RuntimeResult<Value> {
        if args.is_empty() {
            return Err(RuntimeError::ArityMismatch {
                function: "/".to_string(),
                expected: "at least 1".to_string(),
                actual: 0,
            });
        }
        
        let mut result = match &args[0] {
            Value::Integer(n) => *n as f64,
            Value::Float(f) => *f,
            _ => return Err(RuntimeError::TypeError {
                expected: "number".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "/".to_string(),
            }),
        };
        
        for arg in &args[1..] {
            let divisor = match arg {
                Value::Integer(n) => *n as f64,
                Value::Float(f) => *f,
                _ => return Err(RuntimeError::TypeError {
                    expected: "number".to_string(),
                    actual: arg.type_name().to_string(),
                    operation: "/".to_string(),
                }),
            };
            
            if divisor == 0.0 {
                return Err(RuntimeError::DivisionByZero);
            }
            
            result /= divisor;
        }
        
        Ok(Value::Float(result))
    }
    
    // Comparison functions
    fn equal(args: &[Value]) -> RuntimeResult<Value> {
        if args.is_empty() {
            return Ok(Value::Boolean(true));
        }
        
        let first = &args[0];
        for arg in &args[1..] {
            if first != arg {
                return Ok(Value::Boolean(false));
            }
        }
        Ok(Value::Boolean(true))
    }
    
    fn not_equal(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "!=".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(args[0] != args[1]))
    }
    
    fn greater_than(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: ">".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }
        
        Self::compare_values(&args[0], &args[1], ">", |a, b| a > b)
    }
    
    fn less_than(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "<".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }
        
        Self::compare_values(&args[0], &args[1], "<", |a, b| a < b)
    }
    
    fn greater_equal(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: ">=".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }
        
        Self::compare_values(&args[0], &args[1], ">=", |a, b| a >= b)
    }
    
    fn less_equal(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "<=".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }
        
        Self::compare_values(&args[0], &args[1], "<=", |a, b| a <= b)
    }
    
    fn compare_values(
        a: &Value, 
        b: &Value, 
        op: &str, 
        cmp: fn(f64, f64) -> bool
    ) -> RuntimeResult<Value> {
        let (a_val, b_val) = match (a, b) {
            (Value::Integer(a), Value::Integer(b)) => (*a as f64, *b as f64),
            (Value::Integer(a), Value::Float(b)) => (*a as f64, *b),
            (Value::Float(a), Value::Integer(b)) => (*a, *b as f64),
            (Value::Float(a), Value::Float(b)) => (*a, *b),
            (Value::String(a), Value::String(b)) => {
                return Ok(Value::Boolean(match op {
                    ">" => a > b,
                    "<" => a < b,
                    ">=" => a >= b,
                    "<=" => a <= b,
                    _ => false,
                }));
            },
            _ => return Err(RuntimeError::TypeError {
                expected: "comparable types".to_string(),
                actual: format!("{} and {}", a.type_name(), b.type_name()),
                operation: op.to_string(),
            }),
        };
        
        Ok(Value::Boolean(cmp(a_val, b_val)))
    }
    
    // Boolean functions
    fn and(args: &[Value]) -> RuntimeResult<Value> {
        for arg in args {
            if !arg.is_truthy() {
                return Ok(arg.clone());
            }
        }
        Ok(args.last().cloned().unwrap_or(Value::Boolean(true)))
    }
    
    fn or(args: &[Value]) -> RuntimeResult<Value> {
        for arg in args {
            if arg.is_truthy() {
                return Ok(arg.clone());
            }
        }
        Ok(args.last().cloned().unwrap_or(Value::Nil))
    }
    
    fn not(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "not".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(!args[0].is_truthy()))
    }
    
    // String functions
    fn str(args: &[Value]) -> RuntimeResult<Value> {
        let result = args.iter()
            .map(|v| v.to_string())
            .collect::<Vec<String>>()
            .join("");
        Ok(Value::String(result))
    }
    
    fn string_length(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "string-length".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        
        match &args[0] {
            Value::String(s) => Ok(Value::Integer(s.chars().count() as i64)),
            _ => Err(RuntimeError::TypeError {
                expected: "string".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "string-length".to_string(),
            }),
        }
    }
    
    fn substring(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() < 2 || args.len() > 3 {
            return Err(RuntimeError::ArityMismatch {
                function: "substring".to_string(),
                expected: "2 or 3".to_string(),
                actual: args.len(),
            });
        }
        
        let string = match &args[0] {
            Value::String(s) => s,
            _ => return Err(RuntimeError::TypeError {
                expected: "string".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "substring".to_string(),
            }),
        };
        
        let start = match &args[1] {
            Value::Integer(n) => *n as usize,
            _ => return Err(RuntimeError::TypeError {
                expected: "integer".to_string(),
                actual: args[1].type_name().to_string(),
                operation: "substring".to_string(),
            }),
        };
        
        let end = if args.len() == 3 {
            match &args[2] {
                Value::Integer(n) => Some(*n as usize),
                _ => return Err(RuntimeError::TypeError {
                    expected: "integer".to_string(),
                    actual: args[2].type_name().to_string(),
                    operation: "substring".to_string(),
                }),
            }
        } else {
            None
        };
        
        let chars: Vec<char> = string.chars().collect();
        let slice = if let Some(end) = end {
            chars.get(start..end)
        } else {
            chars.get(start..)
        };
        
        match slice {
            Some(chars) => Ok(Value::String(chars.iter().collect())),
            None => Err(RuntimeError::IndexOutOfBounds {
                index: start as i64,
                length: chars.len(),
            }),
        }
    }
    
    // Collection functions (simplified implementations)
    fn get(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() < 2 || args.len() > 3 {
            return Err(RuntimeError::ArityMismatch {
                function: "get".to_string(),
                expected: "2 or 3".to_string(),
                actual: args.len(),
            });
        }
        
        let default = args.get(2).cloned().unwrap_or(Value::Nil);
        
        match (&args[0], &args[1]) {
            (Value::Map(map), key) => {
                let map_key = Self::value_to_map_key(key)?;
                Ok(map.get(&map_key).cloned().unwrap_or(default))
            },
            (Value::Vector(vec), Value::Integer(index)) => {
                let idx = *index as usize;
                Ok(vec.get(idx).cloned().unwrap_or(default))
            },
            _ => Err(RuntimeError::TypeError {
                expected: "map or vector with appropriate key/index".to_string(),
                actual: format!("{} with {}", args[0].type_name(), args[1].type_name()),
                operation: "get".to_string(),
            }),
        }
    }
    
    fn count(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "count".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        
        match &args[0] {
            Value::Vector(v) => Ok(Value::Integer(v.len() as i64)),
            Value::Map(m) => Ok(Value::Integer(m.len() as i64)),
            Value::String(s) => Ok(Value::Integer(s.chars().count() as i64)),
            _ => Err(RuntimeError::TypeError {
                expected: "vector, map, or string".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "count".to_string(),
            }),
        }
    }
    
    fn vector(args: &[Value]) -> RuntimeResult<Value> {
        Ok(Value::Vector(args.to_vec()))
    }
    
    fn hash_map(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() % 2 != 0 {
            return Err(RuntimeError::ArityMismatch {
                function: "map".to_string(),
                expected: "even number of arguments".to_string(),
                actual: args.len(),
            });
        }
        
        let mut result = HashMap::new();
        for chunk in args.chunks(2) {
            let key = Self::value_to_map_key(&chunk[0])?;
            let value = chunk[1].clone();
            result.insert(key, value);
        }
        
        Ok(Value::Map(result))
    }
    
    // Placeholder implementations for other collection functions
    fn assoc(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() < 3 {
            return Err(RuntimeError::ArityMismatch {
                function: "assoc".to_string(),
                expected: "at least 3".to_string(),
                actual: args.len(),
            });
        }
        
        match &args[0] {
            Value::Map(map) => {
                let mut new_map = map.clone();
                
                // Process key-value pairs
                for chunk in args[1..].chunks(2) {
                    if chunk.len() == 2 {
                        let key = Self::value_to_map_key(&chunk[0])?;
                        let value = chunk[1].clone();
                        new_map.insert(key, value);
                    }
                }
                
                Ok(Value::Map(new_map))
            },
            Value::Vector(vec) => {
                if args.len() != 3 {
                    return Err(RuntimeError::ArityMismatch {
                        function: "assoc".to_string(),
                        expected: "3 arguments for vector".to_string(),
                        actual: args.len(),
                    });
                }
                
                let index = match &args[1] {
                    Value::Integer(i) => *i as usize,
                    _ => return Err(RuntimeError::TypeError {
                        expected: "integer".to_string(),
                        actual: args[1].type_name().to_string(),
                        operation: "assoc".to_string(),
                    }),
                };
                
                let mut new_vec = vec.clone();
                if index < new_vec.len() {
                    new_vec[index] = args[2].clone();
                    Ok(Value::Vector(new_vec))
                } else {
                    Err(RuntimeError::IndexOutOfBounds {
                        index: index as i64,
                        length: new_vec.len(),
                    })
                }
            },
            _ => Err(RuntimeError::TypeError {
                expected: "map or vector".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "assoc".to_string(),
            }),
        }
    }
    
    fn dissoc(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() < 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "dissoc".to_string(),
                expected: "at least 2".to_string(),
                actual: args.len(),
            });
        }
        
        match &args[0] {
            Value::Map(map) => {
                let mut new_map = map.clone();
                
                // Remove all specified keys
                for key_val in &args[1..] {
                    let key = Self::value_to_map_key(key_val)?;
                    new_map.remove(&key);
                }
                
                Ok(Value::Map(new_map))
            },
            _ => Err(RuntimeError::TypeError {
                expected: "map".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "dissoc".to_string(),
            }),
        }
    }
    
    // Type predicate functions
    fn int_p(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "int?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Integer(_))))
    }
    
    fn float_p(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "float?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Float(_))))
    }
    
    fn number_p(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "number?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Integer(_) | Value::Float(_))))
    }
    
    fn string_p(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "string?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::String(_))))
    }
    
    fn bool_p(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "bool?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Boolean(_))))
    }
    
    fn nil_p(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "nil?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Nil)))
    }
    
    fn map_p(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "map?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Map(_))))
    }
    
    fn vector_p(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "vector?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Vector(_))))
    }
    
    fn keyword_p(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "keyword?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Keyword(_))))
    }
    
    fn symbol_p(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "symbol?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Symbol(_))))
    }
    
    fn fn_p(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "fn?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Function(_))))
    }

    // Tool functions
    fn tool_log(args: &[Value]) -> RuntimeResult<Value> {
        let message = args.iter()
            .map(|v| format!("{:?}", v))
            .collect::<Vec<_>>()
            .join(" ");
        println!("[LOG] {}", message);
        Ok(Value::Nil)
    }

    fn tool_print(args: &[Value]) -> RuntimeResult<Value> {
        let message = args.iter()
            .map(|v| format!("{:?}", v))
            .collect::<Vec<_>>()
            .join(" ");
        print!("{}", message);
        Ok(Value::Nil)
    }

    fn tool_current_time(args: &[Value]) -> RuntimeResult<Value> {
        if !args.is_empty() {
            return Err(RuntimeError::ArityMismatch {
                function: "tool:current-time".to_string(),
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

    fn tool_parse_json(_args: &[Value]) -> RuntimeResult<Value> {
        // Placeholder - JSON parsing not implemented
        Err(RuntimeError::NotImplemented("JSON parsing not implemented".to_string()))
    }

    fn tool_serialize_json(_args: &[Value]) -> RuntimeResult<Value> {
        // Placeholder - JSON serialization not implemented
        Err(RuntimeError::NotImplemented("JSON serialization not implemented".to_string()))
    }

    fn tool_open_file(_args: &[Value]) -> RuntimeResult<Value> {
        // Placeholder - File operations not implemented
        Err(RuntimeError::NotImplemented("File operations not implemented".to_string()))
    }

    fn tool_read_line(_args: &[Value]) -> RuntimeResult<Value> {
        // Placeholder - File operations not implemented
        Err(RuntimeError::NotImplemented("File operations not implemented".to_string()))
    }

    fn tool_write_line(_args: &[Value]) -> RuntimeResult<Value> {
        // Placeholder - File operations not implemented
        Err(RuntimeError::NotImplemented("File operations not implemented".to_string()))
    }

    fn tool_close_file(_args: &[Value]) -> RuntimeResult<Value> {
        // Placeholder - File operations not implemented
        Err(RuntimeError::NotImplemented("File operations not implemented".to_string()))
    }

    fn tool_get_env(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "tool:get-env".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        
        match &args[0] {
            Value::String(key) => {
                match std::env::var(key) {
                    Ok(value) => Ok(Value::String(value)),
                    Err(_) => Ok(Value::Nil),
                }
            },
            _ => Err(RuntimeError::TypeError {
                expected: "string".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "tool:get-env".to_string(),
            }),
        }
    }

    fn tool_http_fetch(_args: &[Value]) -> RuntimeResult<Value> {
        // Placeholder - HTTP operations not implemented
        Err(RuntimeError::NotImplemented("HTTP operations not implemented".to_string()))
    }

    fn tool_file_exists_p(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "tool:file-exists?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        
        match &args[0] {
            Value::String(path) => {
                Ok(Value::Boolean(std::path::Path::new(path).exists()))
            },
            _ => Err(RuntimeError::TypeError {
                expected: "string".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "tool:file-exists?".to_string(),
            }),
        }
    }

    // Agent functions
    fn discover_agents(_args: &[Value]) -> RuntimeResult<Value> {
        // Placeholder - Agent discovery not implemented
        Ok(Value::Vector(vec![]))
    }

    fn task_coordination(_args: &[Value]) -> RuntimeResult<Value> {
        // Placeholder - Task coordination not implemented
        Ok(Value::Map(HashMap::new()))
    }

    fn factorial(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "factorial".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        
        match &args[0] {
            Value::Integer(n) => {
                if *n < 0 {
                    return Err(RuntimeError::InvalidArgument(
                        "Factorial is not defined for negative numbers".to_string()
                    ));
                }
                let mut result = 1i64;
                for i in 1..=*n {
                    result *= i;
                }
                Ok(Value::Integer(result))
            },
            _ => Err(RuntimeError::TypeError {
                expected: "integer".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "factorial".to_string(),
            }),
        }
    }

    fn length_value(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "length".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        
        match &args[0] {
            Value::Vector(v) => Ok(Value::Integer(v.len() as i64)),
            Value::String(s) => Ok(Value::Integer(s.len() as i64)),
            Value::Map(m) => Ok(Value::Integer(m.len() as i64)),
            _ => Err(RuntimeError::TypeError {
                expected: "vector, string, or map".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "length".to_string(),
            }),
        }
    }

    fn discover_and_assess_agents(_args: &[Value]) -> RuntimeResult<Value> {
        // Placeholder - Agent discovery and assessment not implemented
        Ok(Value::Vector(vec![]))
    }

    fn establish_system_baseline(_args: &[Value]) -> RuntimeResult<Value> {
        // Placeholder - System baseline establishment not implemented
        Ok(Value::Map(HashMap::new()))
    }

    fn current_timestamp_ms(_args: &[Value]) -> RuntimeResult<Value> {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        Ok(Value::Integer(timestamp as i64))
    }    fn task_current_with_evaluator(
        _args: &[Expression], 
        evaluator: &crate::runtime::evaluator::Evaluator,
        _env: &mut Environment
    ) -> RuntimeResult<Value> {
        Ok(evaluator.get_task_context().unwrap_or(Value::Nil))
    }
}