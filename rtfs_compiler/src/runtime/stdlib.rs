// Standard library implementation for RTFS
// Contains all built-in functions and tool interfaces

use std::collections::HashMap;
use crate::ast::{Symbol, Keyword, MapKey};
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
        
        env.define(&Symbol("map".to_string()), Value::Function(Function::Builtin {
            name: "map".to_string(),
            arity: Arity::Any,
            func: Self::map,
        }));
        
        env.define(&Symbol("map-fn".to_string()), Value::Function(Function::Builtin {
            name: "map-fn".to_string(),
            arity: Arity::AtLeast(2),
            func: Self::map_function,
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
            func: Self::bool_p,
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
            arity: Arity::Exact(1),
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
        }));
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
    
    fn map(args: &[Value]) -> RuntimeResult<Value> {
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
    
    fn conj(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() < 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "conj".to_string(),
                expected: "at least 2".to_string(),
                actual: args.len(),
            });
        }
        
        match &args[0] {
            Value::Vector(vec) => {
                let mut new_vec = vec.clone();
                
                // Add all new elements
                for value in &args[1..] {
                    new_vec.push(value.clone());
                }
                
                Ok(Value::Vector(new_vec))
            },
            Value::Map(map) => {
                let mut new_map = map.clone();
                
                // Add key-value pairs (expected as vectors [key value])
                for pair in &args[1..] {
                    match pair {
                        Value::Vector(pair_vec) if pair_vec.len() == 2 => {
                            let key = Self::value_to_map_key(&pair_vec[0])?;
                            let value = pair_vec[1].clone();
                            new_map.insert(key, value);
                        },
                        _ => return Err(RuntimeError::TypeError {
                            expected: "vector of length 2 [key value]".to_string(),
                            actual: format!("{} of length {}", 
                                pair.type_name(), 
                                if let Value::Vector(v) = pair { v.len() } else { 0 }
                            ),
                            operation: "conj".to_string(),
                        }),
                    }
                }
                
                Ok(Value::Map(new_map))
            },
            _ => Err(RuntimeError::TypeError {
                expected: "vector or map".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "conj".to_string(),
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
    
    // Tool functions (placeholder implementations)
    fn tool_log(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "tool:log".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        
        println!("LOG: {}", args[0].to_string());
        Ok(Value::Nil)
    }
    
    fn tool_print(args: &[Value]) -> RuntimeResult<Value> {
        let output = args.iter()
            .map(|v| v.to_string())
            .collect::<Vec<String>>()
            .join(" ");
        println!("{}", output);
        Ok(Value::Nil)
    }
    
    fn tool_current_time(_args: &[Value]) -> RuntimeResult<Value> {
        // Simple placeholder - would use actual system time in real implementation
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| RuntimeError::InternalError(format!("Time error: {}", e)))?;
        
        // Return timestamp as string
        Ok(Value::String(format!("{}", now.as_secs())))
    }
    
    fn tool_parse_json(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "tool:parse-json".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        
        let json_str = match &args[0] {
            Value::String(s) => s,
            _ => return Err(RuntimeError::TypeError {
                expected: "string".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "tool:parse-json".to_string(),
            }),
        };
        
        // Simple JSON parser (placeholder - would use proper JSON library)
        // For now, just handle basic cases
        match json_str.trim() {
            "null" => Ok(Value::Ok(Box::new(Value::Nil))),
            "true" => Ok(Value::Ok(Box::new(Value::Boolean(true)))),
            "false" => Ok(Value::Ok(Box::new(Value::Boolean(false)))),
            s if s.starts_with('"') && s.ends_with('"') => {
                let content = &s[1..s.len()-1];
                Ok(Value::Ok(Box::new(Value::String(content.to_string()))))
            },
            s if s.parse::<i64>().is_ok() => {
                let num = s.parse::<i64>().unwrap();
                Ok(Value::Ok(Box::new(Value::Integer(num))))
            },
            s if s.parse::<f64>().is_ok() => {
                let num = s.parse::<f64>().unwrap();
                Ok(Value::Ok(Box::new(Value::Float(num))))
            },
            _ => Ok(Value::Error(crate::runtime::values::ErrorValue {
                error_type: crate::ast::Keyword("error/json".to_string()),
                message: "Invalid JSON".to_string(),
                data: None,
            })),
        }
    }
    
    fn tool_serialize_json(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "tool:serialize-json".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        
        // Simple JSON serializer (placeholder)
        let json_str = match &args[0] {
            Value::Nil => "null".to_string(),
            Value::Boolean(b) => b.to_string(),
            Value::Integer(n) => n.to_string(),
            Value::Float(f) => f.to_string(),
            Value::String(s) => format!("\"{}\"", s.replace("\"", "\\\"")),
            Value::Vector(v) => {
                let elements: Vec<String> = v.iter()
                    .map(|val| match Self::tool_serialize_json(&[val.clone()]) {
                        Ok(Value::Ok(boxed_val)) => match *boxed_val {
                            Value::String(s) => s,
                            _ => "null".to_string(),
                        },
                        _ => "null".to_string(),
                    })
                    .collect();
                format!("[{}]", elements.join(","))
            },
            _ => return Ok(Value::Error(crate::runtime::values::ErrorValue {
                error_type: crate::ast::Keyword("error/json".to_string()),
                message: "Value not serializable to JSON".to_string(),
                data: None,
            })),
        };
        
        Ok(Value::Ok(Box::new(Value::String(json_str))))
    }
    
    // Enhanced tool functions for resource management
    fn tool_open_file(args: &[Value]) -> RuntimeResult<Value> {
        if args.is_empty() || args.len() > 3 {
            return Err(RuntimeError::ArityMismatch {
                function: "tool:open-file".to_string(),
                expected: "1-3".to_string(),
                actual: args.len(),
            });
        }
        
        let filename = match &args[0] {
            Value::String(s) => s,
            _ => return Err(RuntimeError::TypeError {
                expected: "string".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "tool:open-file filename".to_string(),
            }),
        };
        
        // Parse optional mode parameter
        let _mode = if args.len() > 1 {
            match &args[1] {
                Value::Keyword(k) => k.0.as_str(),
                _ => "read",
            }
        } else {
            "read"
        };
        
        // Create a resource handle for the file
        let mut metadata = HashMap::new();
        metadata.insert("filename".to_string(), Value::String(filename.clone()));
        metadata.insert("mode".to_string(), Value::String(_mode.to_string()));
        
        let resource = crate::runtime::values::ResourceHandle {
            id: format!("file_{}", filename),
            resource_type: "FileHandle".to_string(),
            metadata,
            state: crate::runtime::values::ResourceState::Active,
        };
        
        Ok(Value::Resource(resource))
    }
    
    fn tool_read_line(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "tool:read-line".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        
        match &args[0] {
            Value::Resource(handle) => {
                // Check if resource is still active
                if handle.state != crate::runtime::values::ResourceState::Active {
                    return Err(RuntimeError::ResourceError {
                        resource_type: handle.resource_type.clone(),
                        message: "Attempted to read from released file handle".to_string(),
                    });
                }
                
                // Simulate reading a line (in real implementation, would use actual file I/O)
                let filename = handle.metadata.get("filename")
                    .and_then(|v| match v {
                        Value::String(s) => Some(s.clone()),
                        _ => None,
                    })
                    .unwrap_or_else(|| "unknown".to_string());
                
                // Return simulated content
                Ok(Value::Ok(Box::new(Value::String(format!("Content from {}", filename)))))
            },
            _ => Err(RuntimeError::TypeError {
                expected: "file handle".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "tool:read-line".to_string(),
            }),
        }
    }
    
    fn tool_write_line(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "tool:write-line".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }
        
        let handle = match &args[0] {
            Value::Resource(h) => h,
            _ => return Err(RuntimeError::TypeError {
                expected: "file handle".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "tool:write-line".to_string(),
            }),
        };
        
        // Check if resource is still active
        if handle.state != crate::runtime::values::ResourceState::Active {
            return Err(RuntimeError::ResourceError {
                resource_type: handle.resource_type.clone(),
                message: "Attempted to write to released file handle".to_string(),
            });
        }
        
        let content = match &args[1] {
            Value::String(s) => s,
            _ => return Err(RuntimeError::TypeError {
                expected: "string".to_string(),
                actual: args[1].type_name().to_string(),
                operation: "tool:write-line content".to_string(),
            }),
        };
        
        // Simulate writing (in real implementation, would use actual file I/O)
        println!("Writing to {}: {}", handle.id, content);
        
        Ok(Value::Ok(Box::new(Value::String(format!("Wrote {} chars", content.len())))))
    }
    
    fn tool_close_file(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "tool:close-file".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        
        match &args[0] {
            Value::Resource(handle) => {
                // In a real implementation, this would perform actual file closing
                println!("Closing file handle: {}", handle.id);
                Ok(Value::Ok(Box::new(Value::Nil)))
            },
            _ => Err(RuntimeError::TypeError {
                expected: "file handle".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "tool:close-file".to_string(),
            }),
        }
    }
    
    fn tool_get_env(args: &[Value]) -> RuntimeResult<Value> {
        if args.is_empty() || args.len() > 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "tool:get-env".to_string(),
                expected: "1-2".to_string(),
                actual: args.len(),
            });
        }
        
        let var_name = match &args[0] {
            Value::String(s) => s,
            _ => return Err(RuntimeError::TypeError {
                expected: "string".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "tool:get-env variable name".to_string(),
            }),
        };
        
        let default_value = if args.len() > 1 {
            Some(args[1].clone())
        } else {
            None
        };
        
        // Get environment variable
        match std::env::var(var_name) {
            Ok(value) => Ok(Value::Ok(Box::new(Value::String(value)))),
            Err(_) => {
                if let Some(default) = default_value {
                    Ok(Value::Ok(Box::new(default)))
                } else {
                    Ok(Value::Error(crate::runtime::values::ErrorValue {
                        error_type: crate::ast::Keyword("error/env-not-found".to_string()),
                        message: format!("Environment variable '{}' not found", var_name),
                        data: None,
                    }))
                }
            }
        }
    }
    
    fn tool_http_fetch(args: &[Value]) -> RuntimeResult<Value> {
        if args.is_empty() || args.len() > 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "tool:http-fetch".to_string(),
                expected: "1-2".to_string(),
                actual: args.len(),
            });
        }
        
        let url = match &args[0] {
            Value::String(s) => s,
            _ => return Err(RuntimeError::TypeError {
                expected: "string".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "tool:http-fetch URL".to_string(),
            }),
        };
        
        // In a real implementation, this would make actual HTTP requests
        // For now, simulate different responses based on URL
        if url.contains("example.com") {
            Ok(Value::Ok(Box::new(Value::String(format!("Fetched content from {}", url)))))
        } else if url.contains("error") {
            Ok(Value::Error(crate::runtime::values::ErrorValue {
                error_type: crate::ast::Keyword("error/network".to_string()),
                message: "Network error during fetch".to_string(),
                data: Some({
                    let mut data = HashMap::new();
                    data.insert("url".to_string(), Value::String(url.clone()));
                    data
                }),
            }))
        } else {
            Ok(Value::Ok(Box::new(Value::String(format!("Mock response from {}", url)))))
        }
    }
    
    // Helper functions
    fn value_to_map_key(value: &Value) -> RuntimeResult<MapKey> {
        match value {
            Value::Keyword(k) => Ok(MapKey::Keyword(k.clone())),
            Value::String(s) => Ok(MapKey::String(s.clone())),
            Value::Integer(i) => Ok(MapKey::Integer(*i)),
            _ => Err(RuntimeError::TypeError {
                expected: "keyword, string, or integer".to_string(),
                actual: value.type_name().to_string(),
                operation: "map key conversion".to_string(),
            }),
        }
    }
    
    fn map_function(args: &[Value]) -> RuntimeResult<Value> {
        if args.len() < 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "map-fn".to_string(),
                expected: "at least 2".to_string(),
                actual: args.len(),
            });
        }
        
        let function = &args[0];
        let collections = &args[1..];
        
        // For now, support single collection mapping
        if collections.len() != 1 {
            return Err(RuntimeError::InternalError(
                "map-fn currently supports only single collection".to_string()
            ));
        }
        
        match &collections[0] {
            Value::Vector(vec) => {
                let mut results = Vec::new();
                for item in vec {
                    // This would require a way to call the function value
                    // For now, simulate with a simple operation
                    match function {
                        Value::Function(_) => {
                            // In a real implementation, we'd call the function here
                            // For demonstration, just return the item unchanged
                            results.push(item.clone());
                        },
                        _ => return Err(RuntimeError::TypeError {
                            expected: "function".to_string(),
                            actual: function.type_name().to_string(),
                            operation: "map-fn".to_string(),
                        }),
                    }
                }
                Ok(Value::Vector(results))
            },
            _ => Err(RuntimeError::TypeError {
                expected: "vector".to_string(),
                actual: collections[0].type_name().to_string(),
                operation: "map-fn".to_string(),
            }),
        }
    }
}
