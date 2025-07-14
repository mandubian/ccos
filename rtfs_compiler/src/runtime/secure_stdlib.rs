//! RTFS Secure Standard Library
//!
//! This module contains only pure, deterministic functions that are safe to execute
//! in any context without security concerns. All dangerous operations (file I/O,
//! network access, system calls) are moved to CCOS capabilities.

use crate::ast::{Keyword, MapKey};
use crate::runtime::values::Value;
use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::runtime::values::{BuiltinFunction, Arity, Function, BuiltinFunctionWithContext};
use crate::runtime::environment::Environment;
use crate::runtime::evaluator::Evaluator;
use crate::ast::Symbol;
use std::collections::HashMap;
use std::rc::Rc;

/// Secure Standard Library - contains only pure, safe functions
pub struct SecureStandardLibrary;

impl SecureStandardLibrary {
    /// Create a secure global environment with only safe functions
    pub fn create_secure_environment() -> Environment {
        let mut env = Environment::new();
        
        // Load only safe functions
        Self::load_arithmetic_functions(&mut env);
        Self::load_comparison_functions(&mut env);
        Self::load_boolean_functions(&mut env);
        Self::load_string_functions(&mut env);
        Self::load_collection_functions(&mut env);
        Self::load_type_predicate_functions(&mut env);
        
        env
    }
    
    pub(crate) fn load_arithmetic_functions(env: &mut Environment) {
        // Arithmetic functions (safe, pure)
        env.define(
            &Symbol("+".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "+".to_string(),
                arity: Arity::Variadic(1),
                func: Rc::new(Self::add),
            })),
        );
        
        env.define(
            &Symbol("-".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "-".to_string(),
                arity: Arity::Variadic(1),
                func: Rc::new(Self::subtract),
            })),
        );
        
        env.define(
            &Symbol("*".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "*".to_string(),
                arity: Arity::Variadic(1),
                func: Rc::new(Self::multiply),
            })),
        );
        
        env.define(
            &Symbol("/".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "/".to_string(),
                arity: Arity::Variadic(1),
                func: Rc::new(Self::divide),
            })),
        );

        // Max
        env.define(
            &Symbol("max".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "max".to_string(),
                arity: Arity::Variadic(1),
                func: Rc::new(Self::max_value),
            })),
        );

        // Min
        env.define(
            &Symbol("min".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "min".to_string(),
                arity: Arity::Variadic(1),
                func: Rc::new(Self::min_value),
            })),
        );

        // Increment function
        env.define(
            &Symbol("inc".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "inc".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(|args| {
                    if args.len() != 1 {
                        return Err(RuntimeError::Generic(
                            "inc expects exactly 1 argument".to_string(),
                        ));
                    }
                    match &args[0] {
                        Value::Integer(n) => Ok(Value::Integer(n + 1)),
                        Value::Float(f) => Ok(Value::Float(f + 1.0)),
                        _ => Err(RuntimeError::Generic("inc expects a number".to_string())),
                    }
                }),
            })),
        );

        // Decrement function
        env.define(
            &Symbol("dec".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "dec".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(|args| {
                    if args.len() != 1 {
                        return Err(RuntimeError::Generic(
                            "dec expects exactly 1 argument".to_string(),
                        ));
                    }
                    match &args[0] {
                        Value::Integer(n) => Ok(Value::Integer(n - 1)),
                        Value::Float(f) => Ok(Value::Float(f - 1.0)),
                        _ => Err(RuntimeError::Generic("dec expects a number".to_string())),
                    }
                }),
            })),
        );
    }
    
    pub(crate) fn load_comparison_functions(env: &mut Environment) {
        // Comparison functions (safe, pure)
        env.define(
            &Symbol("=".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "=".to_string(),
                arity: Arity::Variadic(1),
                func: Rc::new(Self::equal),
            })),
        );
        
        env.define(
            &Symbol("!=".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "!=".to_string(),
                arity: Arity::Fixed(2),
                func: Rc::new(Self::not_equal),
            })),
        );
        
        env.define(
            &Symbol(">".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: ">".to_string(),
                arity: Arity::Fixed(2),
                func: Rc::new(Self::greater_than),
            })),
        );

        env.define(
            &Symbol("<".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "<".to_string(),
                arity: Arity::Fixed(2),
                func: Rc::new(Self::less_than),
            })),
        );

        env.define(
            &Symbol(">=".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: ">=".to_string(),
                arity: Arity::Fixed(2),
                func: Rc::new(Self::greater_equal),
            })),
        );

        env.define(
            &Symbol("<=".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "<=".to_string(),
                arity: Arity::Fixed(2),
                func: Rc::new(Self::less_equal),
            })),
        );
    }
    
    pub(crate) fn load_boolean_functions(env: &mut Environment) {
        // Boolean logic functions (safe, pure)
        env.define(
            &Symbol("and".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "and".to_string(),
                arity: Arity::Variadic(0),
                func: Rc::new(|args| Self::and(args)),
            })),
        );
        
        env.define(
            &Symbol("or".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "or".to_string(),
                arity: Arity::Variadic(0),
                func: Rc::new(|args| Self::or(args)),
            })),
        );
        
        env.define(
            &Symbol("not".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "not".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::not),
            })),
        );
    }
    
    pub(crate) fn load_string_functions(env: &mut Environment) {
        // String functions (safe, pure)
        env.define(
            &Symbol("str".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "str".to_string(),
                arity: Arity::Variadic(0),
                func: Rc::new(Self::str),
            })),
        );
        
        env.define(
            &Symbol("string-length".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "string-length".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::string_length),
            })),
        );

        env.define(
            &Symbol("substring".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "substring".to_string(),
                arity: Arity::Variadic(2),
                func: Rc::new(Self::substring),
            })),
        );
        
        env.define(
            &Symbol("string-contains".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "string-contains".to_string(),
                arity: Arity::Fixed(2),
                func: Rc::new(Self::string_contains),
            })),
        );
    }
    
    pub(crate) fn load_collection_functions(env: &mut Environment) {
        // Collection functions (safe, pure)
        // Map function - now supports user-defined functions with evaluator context
        env.define(
            &Symbol("map".to_string()),
            Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
                name: "map".to_string(),
                arity: Arity::Fixed(2),
                func: Rc::new(Self::map_with_context),
            })),
        );

        // Filter function - now supports user-defined functions with evaluator context
        env.define(
            &Symbol("filter".to_string()),
            Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
                name: "filter".to_string(),
                arity: Arity::Fixed(2),
                func: Rc::new(Self::filter_with_context),
            })),
        );

        // Reduce function - now supports user-defined functions with evaluator context
        env.define(
            &Symbol("reduce".to_string()),
            Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
                name: "reduce".to_string(),
                arity: Arity::Range(2, 3),
                func: Rc::new(Self::reduce_with_context),
            })),
        );

        // Empty predicate
        env.define(
            &Symbol("empty?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "empty?".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::empty_p),
            })),
        );

        // Cons function
        env.define(
            &Symbol("cons".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "cons".to_string(),
                arity: Arity::Fixed(2),
                func: Rc::new(Self::cons),
            })),
        );
        
        env.define(
            &Symbol("first".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "first".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::first),
            })),
        );
        
        env.define(
            &Symbol("rest".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "rest".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::rest),
            })),
        );

        // Get-in function
        env.define(
            &Symbol("get-in".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "get-in".to_string(),
                arity: Arity::Variadic(2),
                func: Rc::new(Self::get_in),
            })),
        );

        // Partition function
        env.define(
            &Symbol("partition".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "partition".to_string(),
                arity: Arity::Fixed(2),
                func: Rc::new(Self::partition),
            })),
        );

        // Conj function
        env.define(
            &Symbol("conj".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "conj".to_string(),
                arity: Arity::Variadic(1),
                func: Rc::new(Self::conj),
            })),
        );
        
        env.define(
            &Symbol("get".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "get".to_string(),
                arity: Arity::Variadic(2),
                func: Rc::new(Self::get),
            })),
        );

        // Assoc function
        env.define(
            &Symbol("assoc".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "assoc".to_string(),
                arity: Arity::Variadic(3),
                func: Rc::new(Self::assoc),
            })),
        );

        // Dissoc function
        env.define(
            &Symbol("dissoc".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "dissoc".to_string(),
                arity: Arity::Variadic(2),
                func: Rc::new(Self::dissoc),
            })),
        );
        
        env.define(
            &Symbol("count".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "count".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::count),
            })),
        );
        
        env.define(
            &Symbol("vector".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "vector".to_string(),
                arity: Arity::Variadic(0),
                func: Rc::new(Self::vector),
            })),
        );
        
        env.define(
            &Symbol("hash-map".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "hash-map".to_string(),
                arity: Arity::Variadic(0),
                func: Rc::new(Self::hash_map),
            })),
        );

        env.define(
            &Symbol("range".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "range".to_string(),
                arity: Arity::Fixed(2),
                func: Rc::new(Self::range),
            })),
        );
    }
    
    pub(crate) fn load_type_predicate_functions(env: &mut Environment) {
        // Type predicate functions (safe, pure)
        env.define(
            &Symbol("int?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "int?".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::int_p),
            })),
        );
        
        env.define(
            &Symbol("float?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "float?".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::float_p),
            })),
        );

        env.define(
            &Symbol("number?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "number?".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::number_p),
            })),
        );

        env.define(
            &Symbol("string?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "string?".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::string_p),
            })),
        );

        env.define(
            &Symbol("string-p".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "string-p".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::string_p),
            })),
        );

        env.define(
            &Symbol("bool?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "bool?".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::nil_p),
            })),
        );

        env.define(
            &Symbol("nil?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "nil?".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::nil_p),
            })),
        );

        env.define(
            &Symbol("map?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "map?".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::map_p),
            })),
        );

        env.define(
            &Symbol("vector?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "vector?".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::vector_p),
            })),
        );

        env.define(
            &Symbol("keyword?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "keyword?".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::keyword_p),
            })),
        );

        env.define(
            &Symbol("symbol?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "symbol?".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::symbol_p),
            })),
        );

        env.define(
            &Symbol("fn?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "fn?".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::fn_p),
            })),
        );
    }
    
    // Implementation of pure functions (copied from StandardLibrary)
    fn add(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
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
                }
                Value::Float(f) => {
                    let current = result_float.unwrap_or(result_int.unwrap_or(0) as f64);
                    result_float = Some(current + f);
                    result_int = None;
                }
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "number".to_string(),
                        actual: arg.type_name().to_string(),
                        operation: "+".to_string(),
                    })
                }
            }
        }

        if let Some(f) = result_float {
            Ok(Value::Float(f))
        } else {
            Ok(Value::Integer(result_int.unwrap_or(0)))
        }
    }
    
    fn subtract(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
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
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "number".to_string(),
                        actual: args[0].type_name().to_string(),
                        operation: "-".to_string(),
                    })
                }
            };

            for arg in &args[1..] {
                match arg {
                    Value::Integer(n) => {
                        result.0 -= *n as f64;
                    }
                    Value::Float(f) => {
                        result.0 -= f;
                        result.1 = true;
                    }
                    _ => {
                        return Err(RuntimeError::TypeError {
                            expected: "number".to_string(),
                            actual: arg.type_name().to_string(),
                            operation: "-".to_string(),
                        })
                    }
                }
            }

            if result.1 {
                Ok(Value::Float(result.0))
            } else {
                Ok(Value::Integer(result.0 as i64))
            }
        }
    }
    
    fn multiply(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
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
                }
                Value::Float(f) => {
                    let current = result_float.unwrap_or(result_int.unwrap_or(1) as f64);
                    result_float = Some(current * f);
                    result_int = None;
                }
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "number".to_string(),
                        actual: arg.type_name().to_string(),
                        operation: "*".to_string(),
                    })
                }
            }
        }

        if let Some(f) = result_float {
            Ok(Value::Float(f))
        } else {
            Ok(Value::Integer(result_int.unwrap_or(1)))
        }
    }
    
    fn divide(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
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
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "number".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "/".to_string(),
                })
            }
        };

        for arg in &args[1..] {
            let divisor = match arg {
                Value::Integer(n) => *n as f64,
                Value::Float(f) => *f,
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "number".to_string(),
                        actual: arg.type_name().to_string(),
                        operation: "/".to_string(),
                    })
                }
            };

            if divisor == 0.0 {
                return Err(RuntimeError::DivisionByZero);
            }

            result /= divisor;
        }

        // Check if the result is a whole number and return integer if so
        if result.fract() == 0.0 {
            Ok(Value::Integer(result as i64))
        } else {
            Ok(Value::Float(result))
        }
    }
    
    fn equal(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
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
    
    fn not_equal(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "!=".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(args[0] != args[1]))
    }

    fn greater_than(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: ">".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        Self::compare_values(&args[0], &args[1], ">", |a, b| a > b)
    }

    fn less_than(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "<".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        Self::compare_values(&args[0], &args[1], "<", |a, b| a < b)
    }

    fn greater_equal(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: ">=".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        Self::compare_values(&args[0], &args[1], ">=", |a, b| a >= b)
    }

    fn less_equal(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
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
        cmp: fn(f64, f64) -> bool,
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
            }
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "comparable types".to_string(),
                    actual: format!("{} and {}", a.type_name(), b.type_name()),
                    operation: op.to_string(),
                })
            }
        };

        Ok(Value::Boolean(cmp(a_val, b_val)))
    }
    
    fn and(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        for arg in args {
            if !arg.is_truthy() {
                return Ok(arg.clone());
            }
        }
        Ok(args.last().cloned().unwrap_or(Value::Boolean(true)))
    }
    
    fn or(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        for arg in args {
            if arg.is_truthy() {
                return Ok(arg.clone());
            }
        }
        Ok(args.last().cloned().unwrap_or(Value::Nil))
    }
    
    fn not(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "not".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(!args[0].is_truthy()))
    }
    
    fn str(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        let result = args
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<String>>()
            .join("");
        Ok(Value::String(result))
    }
    
    fn substring(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() < 2 || args.len() > 3 {
            return Err(RuntimeError::ArityMismatch {
                function: "substring".to_string(),
                expected: "2 or 3".to_string(),
                actual: args.len(),
            });
        }

        let string = match &args[0] {
            Value::String(s) => s,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "string".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "substring".to_string(),
                })
            }
        };

        let start = match &args[1] {
            Value::Integer(n) => *n as usize,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "integer".to_string(),
                    actual: args[1].type_name().to_string(),
                    operation: "substring".to_string(),
                })
            }
        };

        let end = if args.len() == 3 {
            match &args[2] {
                Value::Integer(n) => Some(*n as usize),
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "integer".to_string(),
                        actual: args[2].type_name().to_string(),
                        operation: "substring".to_string(),
                    })
                }
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
    
    fn string_length(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
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
    
    fn string_contains(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "string-contains".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        let haystack = match &args[0] {
            Value::String(s) => s,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "string".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "string-contains".to_string(),
                })
            }
        };

        let needle = match &args[1] {
            Value::String(s) => s,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "string".to_string(),
                    actual: args[1].type_name().to_string(),
                    operation: "string-contains".to_string(),
                })
            }
        };

        Ok(Value::Boolean(haystack.contains(needle)))
    }
    
    fn vector(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        Ok(Value::Vector(args.to_vec()))
    }
    
    fn hash_map(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
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
    
    fn get(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
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
            }
            (Value::Vector(vec), Value::Integer(index)) => {
                let idx = *index as usize;
                Ok(vec.get(idx).cloned().unwrap_or(default))
            }
            _ => Err(RuntimeError::TypeError {
                expected: "map or vector with appropriate key/index".to_string(),
                actual: format!("{} with {}", args[0].type_name(), args[1].type_name()),
                operation: "get".to_string(),
            }),
        }
    }
    
    fn count(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
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
    
    fn first(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
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
    
    fn rest(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
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
            }
            _ => Err(RuntimeError::TypeError {
                expected: "vector".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "rest".to_string(),
            }),
        }
    }
    
    fn int_p(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "int?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Integer(_))))
    }

    fn float_p(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "float?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Float(_))))
    }

    fn number_p(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "number?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(
            args[0],
            Value::Integer(_) | Value::Float(_)
        )))
    }
    
    fn string_p(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "string?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::String(_))))
    }

    fn bool_p(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "bool?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Boolean(_))))
    }

    fn nil_p(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "nil?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Nil)))
    }

    fn map_p(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "map?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Map(_))))
    }

    fn vector_p(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "vector?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Vector(_))))
    }

    fn keyword_p(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "keyword?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Keyword(_))))
    }

    fn symbol_p(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "symbol?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Symbol(_))))
    }

    fn fn_p(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "fn?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Function(_))))
    }
    
    fn map_with_context(
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
        let collection_vec = match collection {
            Value::Vector(v) => v.clone(),
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "vector".to_string(),
                    actual: collection.type_name().to_string(),
                    operation: "map".to_string(),
                })
            }
        };
        let mut result = Vec::new();
        for item in collection_vec {
            match function {
                Value::Function(Function::Builtin(builtin_func)) => {
                    // Fast path for builtin functions
                    let func_args = vec![item];
                    let mapped_value = (builtin_func.func)(func_args)?;
                    result.push(mapped_value);
                }
                Value::Function(Function::BuiltinWithContext(builtin_func)) => {
                    // Handle builtin functions with context
                    let func_args = vec![item];
                    let mapped_value = (builtin_func.func)(func_args, evaluator, env)?;
                    result.push(mapped_value);
                }
                Value::Function(Function::Closure(closure)) => {
                    // Handle user-defined functions with full evaluator access
                    let mut func_env = Environment::with_parent(closure.env.clone());
                    func_env.define(&closure.params[0], item);
                    let mapped_value = evaluator.eval_expr(&closure.body, &mut func_env)?;
                    result.push(mapped_value);
                }
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "function".to_string(),
                        actual: function.type_name().to_string(),
                        operation: "map".to_string(),
                    });
                }
            }
        }
        Ok(Value::Vector(result))
    }

    fn filter_with_context(
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
        let function = &args[0];
        let collection = &args[1];
        let collection_vec = match collection {
            Value::Vector(v) => v.clone(),
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "vector".to_string(),
                    actual: collection.type_name().to_string(),
                    operation: "filter".to_string(),
                })
            }
        };
        let mut result = Vec::new();
        for item in collection_vec {
            let keep = match function {
                Value::Function(Function::Builtin(builtin_func)) => {
                    let func_args = vec![item.clone()];
                    let v = (builtin_func.func)(func_args)?;
                    v.is_truthy()
                }
                Value::Function(Function::BuiltinWithContext(builtin_func)) => {
                    let func_args = vec![item.clone()];
                    let v = (builtin_func.func)(func_args, evaluator, env)?;
                    v.is_truthy()
                }
                Value::Function(Function::Closure(closure)) => {
                    let mut func_env = Environment::with_parent(closure.env.clone());
                    func_env.define(&closure.params[0], item.clone());
                    let v = evaluator.eval_expr(&closure.body, &mut func_env)?;
                    v.is_truthy()
                }
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "function".to_string(),
                        actual: function.type_name().to_string(),
                        operation: "filter".to_string(),
                    });
                }
            };
            if keep {
                result.push(item);
            }
        }
        Ok(Value::Vector(result))
    }

    fn reduce_with_context(
        args: Vec<Value>,
        evaluator: &Evaluator,
        env: &mut Environment,
    ) -> RuntimeResult<Value> {
        if args.len() < 2 || args.len() > 3 {
            return Err(RuntimeError::new("reduce requires 2 or 3 arguments"));
        }
        let function = &args[0];
        let collection_arg_index = args.len() - 1;
        let collection_val = &args[collection_arg_index];
        let collection = match collection_val {
            Value::Vector(v) => v.clone(),
            _ => {
                return Err(RuntimeError::new(
                    "reduce expects a vector as its last argument",
                ))
            }
        };
        if collection.is_empty() {
            return if args.len() == 3 {
                Ok(args[1].clone()) // initial value
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
            let func_args = vec![accumulator.clone(), value.clone()];
            accumulator = match function {
                Value::Function(Function::Builtin(builtin_func)) => (builtin_func.func)(func_args)?,
                Value::Function(Function::BuiltinWithContext(builtin_func)) => {
                    (builtin_func.func)(func_args, evaluator, env)?
                }
                Value::Function(Function::Closure(closure)) => {
                    let mut func_env = Environment::with_parent(closure.env.clone());
                    func_env.define(&closure.params[0], accumulator.clone());
                    func_env.define(&closure.params[1], value.clone());
                    evaluator.eval_expr(&closure.body, &mut func_env)?
                }
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "function".to_string(),
                        actual: function.type_name().to_string(),
                        operation: "reduce".to_string(),
                    });
                }
            };
        }
        Ok(accumulator)
    }

    fn max_value(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
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

    fn min_value(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
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

    fn conj(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
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
            }
            Value::Map(map) => {
                if args.len() != 3 {
                    return Err(RuntimeError::ArityMismatch {
                        function: "conj".to_string(),
                        expected: "3 (for map)".to_string(),
                        actual: args.len(),
                    });
                }
                let key = Self::value_to_map_key(&args[1])?;
                map.insert(key, args[2].clone());
                Ok(collection)
            }
            _ => Err(RuntimeError::TypeError {
                expected: "vector or map".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "conj".to_string(),
            }),
        }
    }

    fn empty_p(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
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

    fn cons(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
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
            }
            _ => Err(RuntimeError::TypeError {
                expected: "vector".to_string(),
                actual: args[1].type_name().to_string(),
                operation: "cons".to_string(),
            }),
        }
    }

    fn get_in(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if !(2..=3).contains(&args.len()) {
            return Err(RuntimeError::ArityMismatch {
                function: "get-in".to_string(),
                expected: "2 or 3".to_string(),
                actual: args.len(),
            });
        }

        let collection = &args[0];
        let path = &args[1];
        let default = if args.len() == 3 {
            Some(args[2].clone())
        } else {
            None
        };

        let path_vec = match path {
            Value::Vector(v) => v,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "vector".to_string(),
                    actual: path.type_name().to_string(),
                    operation: "get-in path".to_string(),
                })
            }
        };

        let mut current = collection.clone();
        for key in path_vec {
            match (&current, key) {
                (Value::Map(m), Value::Keyword(k)) => {
                    if let Some(val) = m.get(&MapKey::Keyword(k.clone())) {
                        current = val.clone();
                    } else {
                        return Ok(default.unwrap_or(Value::Nil));
                    }
                }
                (Value::Map(m), Value::String(s)) => {
                    if let Some(val) = m.get(&MapKey::String(s.clone())) {
                        current = val.clone();
                    } else {
                        return Ok(default.unwrap_or(Value::Nil));
                    }
                }
                (Value::Vector(v), Value::Integer(i)) => {
                    if let Some(val) = v.get(*i as usize) {
                        current = val.clone();
                    } else {
                        return Ok(default.unwrap_or(Value::Nil));
                    }
                }
                _ => return Ok(default.unwrap_or(Value::Nil)),
            }
        }

        Ok(current)
    }

    fn partition(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "partition".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        let size = match &args[0] {
            Value::Integer(i) if *i > 0 => *i as usize,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "positive integer".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "partition size".to_string(),
                })
            }
        };

        let collection = match &args[1] {
            Value::Vector(v) => v.clone(),
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "vector".to_string(),
                    actual: args[1].type_name().to_string(),
                    operation: "partition collection".to_string(),
                })
            }
        };

        let mut result = Vec::new();
        for chunk in collection.chunks(size) {
            result.push(Value::Vector(chunk.to_vec()));
        }
        Ok(Value::Vector(result))
    }

    fn assoc(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
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
            }
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
                    _ => {
                        return Err(RuntimeError::TypeError {
                            expected: "integer".to_string(),
                            actual: args[1].type_name().to_string(),
                            operation: "assoc".to_string(),
                        })
                    }
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
            }
            _ => Err(RuntimeError::TypeError {
                expected: "map or vector".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "assoc".to_string(),
            }),
        }
    }

    fn dissoc(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
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
            }
            _ => Err(RuntimeError::TypeError {
                expected: "map".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "dissoc".to_string(),
            }),
        }
    }

    fn factorial(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
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
                        "Factorial is not defined for negative numbers".to_string(),
                    ));
                }
                let mut result = 1i64;
                for i in 1..=*n {
                    result *= i;
                }
                Ok(Value::Integer(result))
            }
            _ => Err(RuntimeError::TypeError {
                expected: "integer".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "factorial".to_string(),
            }),
        }
    }

    fn length_value(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
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

    fn range(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "range".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }
        let start = match &args[0] {
            Value::Integer(i) => *i,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "Integer".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "range".to_string(),
                })
            }
        };
        let end = match &args[1] {
            Value::Integer(i) => *i,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "Integer".to_string(),
                    actual: args[1].type_name().to_string(),
                    operation: "range".to_string(),
                })
            }
        };
        if end < start {
            return Ok(Value::Vector(vec![]));
        }
        let vec = (start..end).map(Value::Integer).collect();
        Ok(Value::Vector(vec))
    }

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
}
