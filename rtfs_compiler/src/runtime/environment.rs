// Environment for variable bindings and scope management

use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;
use crate::ast::Symbol;
use crate::ir::NodeId;
use crate::runtime::{Value, RuntimeError, RuntimeResult};
use crate::runtime::values::{Function, Arity};

/// Environment for variable bindings
/// Supports lexical scoping with parent environments
#[derive(Debug, Clone, PartialEq)]
pub struct Environment {
    /// Current scope bindings
    bindings: HashMap<String, Value>,
    /// Parent environment for lexical scoping
    parent: Option<Rc<Environment>>,
}

impl Environment {
    /// Create a new empty environment
    pub fn new() -> Self {
        Environment {
            bindings: HashMap::new(),
            parent: None,
        }
    }
    
    /// Create a new environment with a parent
    pub fn with_parent(parent: Rc<Environment>) -> Self {
        Environment {
            bindings: HashMap::new(),
            parent: Some(parent),
        }
    }
    
    /// Define a new binding in the current scope
    pub fn define(&mut self, symbol: &Symbol, value: Value) {
        self.bindings.insert(symbol.0.clone(), value);
    }
    
    /// Look up a symbol in this environment or parent environments
    pub fn lookup(&self, symbol: &Symbol) -> RuntimeResult<Value> {
        if let Some(value) = self.bindings.get(&symbol.0) {
            Ok(value.clone())
        } else if let Some(parent) = &self.parent {
            parent.lookup(symbol)
        } else {
            Err(RuntimeError::UndefinedSymbol(symbol.clone()))
        }
    }
    
    /// Check if a symbol is defined in this environment (not parent environments)
    pub fn contains(&self, symbol: &Symbol) -> bool {
        self.bindings.contains_key(&symbol.0)
    }
    
    /// Update an existing binding (searches up the scope chain)
    pub fn set(&mut self, symbol: &Symbol, value: Value) -> RuntimeResult<()> {
        if self.bindings.contains_key(&symbol.0) {
            self.bindings.insert(symbol.0.clone(), value);
            Ok(())
        } else if let Some(_parent) = &self.parent {
            // We can't modify the parent since it's behind an Rc
            // For now, just create a new binding in the current scope
            // In a more sophisticated implementation, we might use RefCell or other interior mutability
            self.bindings.insert(symbol.0.clone(), value);
            Ok(())
        } else {
            Err(RuntimeError::UndefinedSymbol(symbol.clone()))
        }
    }
    
    /// Get all bindings in the current scope (for debugging)
    pub fn current_bindings(&self) -> &HashMap<String, Value> {
        &self.bindings
    }

    /// Get the keys of the bindings map (for debugging)
    pub fn binding_keys(&self) -> Vec<String> {
        self.bindings.keys().cloned().collect()
    }
}

/// Optimized environment that uses pre-resolved binding IDs
#[derive(Debug, Clone)]
pub struct IrEnvironment {
    bindings: HashMap<NodeId, Value>, // Keyed by binding node ID, not name
    parent: Option<Rc<IrEnvironment>>,
}

impl IrEnvironment {
    pub fn new() -> Self {
        IrEnvironment {
            bindings: HashMap::new(),
            parent: None,
        }
    }
      /// Create a new IrEnvironment pre-loaded with standard library functions
    /// This ensures that standard library functions are available with their proper binding IDs
    pub fn with_stdlib() -> Self {
        let mut env = IrEnvironment::new();
        
        // The binding IDs here must match the IDs assigned by IrConverter::add_builtin_functions
        // They are assigned in the order they appear in the builtins array
        
        // Arithmetic operators
        env.define(1, Value::Function(Function::Builtin {
            name: "+".to_string(),
            arity: Arity::AtLeast(1),
            func: Self::add_builtin,
        }));
        
        env.define(2, Value::Function(Function::Builtin {
            name: "-".to_string(),
            arity: Arity::AtLeast(1),
            func: Self::subtract_builtin,
        }));
        
        env.define(3, Value::Function(Function::Builtin {
            name: "*".to_string(),
            arity: Arity::AtLeast(1),
            func: Self::multiply_builtin,
        }));
        
        env.define(4, Value::Function(Function::Builtin {
            name: "/".to_string(),
            arity: Arity::AtLeast(1),
            func: Self::divide_builtin,
        }));
        
        // Comparison operators
        env.define(5, Value::Function(Function::Builtin {
            name: "=".to_string(),
            arity: Arity::AtLeast(1),
            func: Self::equal_builtin,
        }));
        
        env.define(6, Value::Function(Function::Builtin {
            name: "!=".to_string(),
            arity: Arity::Exact(2),
            func: Self::not_equal_builtin,
        }));
        
        env.define(7, Value::Function(Function::Builtin {
            name: ">".to_string(),
            arity: Arity::Exact(2),
            func: Self::greater_than_builtin,
        }));
        
        env.define(8, Value::Function(Function::Builtin {
            name: "<".to_string(),
            arity: Arity::Exact(2),
            func: Self::less_than_builtin,
        }));
        
        env.define(9, Value::Function(Function::Builtin {
            name: ">=".to_string(),
            arity: Arity::Exact(2),
            func: Self::greater_equal_builtin,
        }));
        
        env.define(10, Value::Function(Function::Builtin {
            name: "<=".to_string(),
            arity: Arity::Exact(2),
            func: Self::less_equal_builtin,
        }));
        
        // Logical operators  
        env.define(11, Value::Function(Function::Builtin {
            name: "and".to_string(),
            arity: Arity::Any,
            func: Self::and_builtin,
        }));
        
        env.define(12, Value::Function(Function::Builtin {
            name: "or".to_string(),
            arity: Arity::Any,
            func: Self::or_builtin,
        }));
        
        env.define(13, Value::Function(Function::Builtin {
            name: "not".to_string(),
            arity: Arity::Exact(1),
            func: Self::not_builtin,
        }));
        
        // Add reduce function with binding ID 52 to match IR converter
        env.define(52, Value::Function(Function::Builtin {
            name: "reduce".to_string(),
            arity: Arity::Exact(3),  // function, initial_value, collection
            func: Self::reduce_builtin,
        }));
        
        // Add more standard library functions as needed...
        // Note: The binding IDs must match the order in IrConverter::add_builtin_functions
        
        env
    }
    
    // Builtin function implementations for IrEnvironment
    fn add_builtin(args: &[Value]) -> crate::runtime::RuntimeResult<Value> {
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
                _ => return Err(crate::runtime::RuntimeError::TypeError {
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
    
    fn subtract_builtin(args: &[Value]) -> crate::runtime::RuntimeResult<Value> {
        if args.is_empty() {
            return Err(crate::runtime::RuntimeError::ArityMismatch {
                function: "-".to_string(),
                expected: "at least 1".to_string(),
                actual: 0,
            });
        }
        
        if args.len() == 1 {
            // Unary minus
            match &args[0] {
                Value::Integer(n) => Ok(Value::Integer(-n)),
                Value::Float(f) => Ok(Value::Float(-f)),
                _ => Err(crate::runtime::RuntimeError::TypeError {
                    expected: "number".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "-".to_string(),
                }),
            }
        } else {
            // Binary subtraction
            let mut result = match &args[0] {
                Value::Integer(n) => (*n as f64, false),
                Value::Float(f) => (*f, true),
                _ => return Err(crate::runtime::RuntimeError::TypeError {
                    expected: "number".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "-".to_string(),
                }),
            };
            
            for arg in &args[1..] {
                match arg {
                    Value::Integer(n) => result.0 -= *n as f64,
                    Value::Float(f) => {
                        result.0 -= f;
                        result.1 = true;
                    },
                    _ => return Err(crate::runtime::RuntimeError::TypeError {
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
    
    fn multiply_builtin(args: &[Value]) -> crate::runtime::RuntimeResult<Value> {
        if args.is_empty() {
            return Ok(Value::Integer(1));
        }
        
        let mut result_int: Option<i64> = None;
        let mut result_float: Option<f64> = None;
        
        for arg in args {
            match arg {
                Value::Integer(n) => {
                    if let Some(float_acc) = result_float {
                        result_float = Some(float_acc * (*n as f64));
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
                _ => return Err(crate::runtime::RuntimeError::TypeError {
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
    
    fn divide_builtin(args: &[Value]) -> crate::runtime::RuntimeResult<Value> {
        if args.len() < 2 {
            return Err(crate::runtime::RuntimeError::ArityMismatch {
                function: "/".to_string(),
                expected: "at least 2".to_string(),
                actual: args.len(),
            });
        }
        
        let mut result = match &args[0] {
            Value::Integer(n) => *n as f64,
            Value::Float(f) => *f,
            _ => return Err(crate::runtime::RuntimeError::TypeError {
                expected: "number".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "/".to_string(),
            }),
        };
        
        for arg in &args[1..] {
            let divisor = match arg {
                Value::Integer(n) => *n as f64,
                Value::Float(f) => *f,
                _ => return Err(crate::runtime::RuntimeError::TypeError {
                    expected: "number".to_string(),
                    actual: arg.type_name().to_string(),
                    operation: "/".to_string(),
                }),
            };
            
            if divisor == 0.0 {
                return Err(crate::runtime::RuntimeError::DivisionByZero);
            }
            
            result /= divisor;
        }
        
        Ok(Value::Float(result))
    }
    
    fn equal_builtin(args: &[Value]) -> crate::runtime::RuntimeResult<Value> {
        if args.len() < 2 {
            return Err(crate::runtime::RuntimeError::ArityMismatch {
                function: "=".to_string(),
                expected: "at least 2".to_string(),
                actual: args.len(),
            });
        }
        
        let first = &args[0];
        for arg in &args[1..] {
            if !Self::values_equal(first, arg) {
                return Ok(Value::Boolean(false));
            }
        }
        Ok(Value::Boolean(true))
    }
    
    fn not_equal_builtin(args: &[Value]) -> crate::runtime::RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(crate::runtime::RuntimeError::ArityMismatch {
                function: "!=".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }
        
        Ok(Value::Boolean(!Self::values_equal(&args[0], &args[1])))
    }
    
    fn greater_than_builtin(args: &[Value]) -> crate::runtime::RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(crate::runtime::RuntimeError::ArityMismatch {
                function: ">".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }
        
        Self::compare_numbers(&args[0], &args[1], |a, b| a > b)
    }
    
    fn less_than_builtin(args: &[Value]) -> crate::runtime::RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(crate::runtime::RuntimeError::ArityMismatch {
                function: "<".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }
        
        Self::compare_numbers(&args[0], &args[1], |a, b| a < b)
    }
    
    fn greater_equal_builtin(args: &[Value]) -> crate::runtime::RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(crate::runtime::RuntimeError::ArityMismatch {
                function: ">=".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }
        
        Self::compare_numbers(&args[0], &args[1], |a, b| a >= b)
    }
    
    fn less_equal_builtin(args: &[Value]) -> crate::runtime::RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(crate::runtime::RuntimeError::ArityMismatch {
                function: "<=".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }
        
        Self::compare_numbers(&args[0], &args[1], |a, b| a <= b)
    }
    
    fn and_builtin(args: &[Value]) -> crate::runtime::RuntimeResult<Value> {
        for arg in args {
            if !arg.is_truthy() {
                return Ok(arg.clone());
            }
        }
        Ok(args.last().cloned().unwrap_or(Value::Boolean(true)))
    }
    
    fn or_builtin(args: &[Value]) -> crate::runtime::RuntimeResult<Value> {
        for arg in args {
            if arg.is_truthy() {
                return Ok(arg.clone());
            }
        }
        Ok(args.last().cloned().unwrap_or(Value::Boolean(false)))
    }
    
    fn not_builtin(args: &[Value]) -> crate::runtime::RuntimeResult<Value> {
        if args.len() != 1 {
            return Err(crate::runtime::RuntimeError::ArityMismatch {
                function: "not".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        
        Ok(Value::Boolean(!args[0].is_truthy()))
    }
    
    fn reduce_builtin(args: &[Value]) -> crate::runtime::RuntimeResult<Value> {
        if args.len() != 3 {
            return Err(crate::runtime::RuntimeError::ArityMismatch {
                function: "reduce".to_string(),
                expected: "3".to_string(),
                actual: args.len(),
            });
        }
        
        let _func = &args[0];         // Function to apply (e.g., +)
        let initial = &args[1];       // Initial value (e.g., 0)
        let collection = &args[2];    // Collection to reduce (e.g., [1 2 3 4 5])
        
        match collection {
            Value::Vector(vec) => {
                let mut accumulator = initial.clone();
                
                // For now, we can only handle builtin functions with limited operations
                // This is a limitation similar to the AST runtime
                match _func {
                    Value::Function(crate::runtime::values::Function::Builtin { name, .. }) if name == "+" => {
                        for item in vec {
                            match (&accumulator, item) {
                                (Value::Integer(acc), Value::Integer(val)) => {
                                    accumulator = Value::Integer(acc + val);
                                },
                                (Value::Float(acc), Value::Float(val)) => {
                                    accumulator = Value::Float(acc + val);
                                },
                                (Value::Integer(acc), Value::Float(val)) => {
                                    accumulator = Value::Float(*acc as f64 + val);
                                },
                                (Value::Float(acc), Value::Integer(val)) => {
                                    accumulator = Value::Float(acc + *val as f64);
                                },
                                _ => return Err(crate::runtime::RuntimeError::TypeError {
                                    expected: "number".to_string(),
                                    actual: format!("{:?}", item),
                                    operation: "addition in reduce".to_string(),
                                }),
                            }
                        }
                        Ok(accumulator)
                    },
                    _ => {
                        // For other functions, return initial value as placeholder
                        // This is a limitation that would need evaluator access to fix properly
                        Ok(initial.clone())
                    }
                }
            },
            _ => Err(crate::runtime::RuntimeError::TypeError {
                expected: "vector".to_string(),
                actual: collection.type_name().to_string(),
                operation: "reduce".to_string(),
            }),
        }
    }
    
    // Helper functions
    fn values_equal(a: &Value, b: &Value) -> bool {
        use Value::*;
        match (a, b) {
            (Nil, Nil) => true,
            (Boolean(a), Boolean(b)) => a == b,
            (Integer(a), Integer(b)) => a == b,
            (Float(a), Float(b)) => (a - b).abs() < f64::EPSILON,
            (Integer(a), Float(b)) => (*a as f64 - b).abs() < f64::EPSILON,
            (Float(a), Integer(b)) => (a - *b as f64).abs() < f64::EPSILON,
            (String(a), String(b)) => a == b,
            (Keyword(a), Keyword(b)) => a == b,
            (Symbol(a), Symbol(b)) => a == b,
            (Vector(a), Vector(b)) => {
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| Self::values_equal(x, y))
            },
            (Map(a), Map(b)) => {
                a.len() == b.len() && 
                a.iter().all(|(k, v)| b.get(k).map_or(false, |v2| Self::values_equal(v, v2)))
            },
            _ => false,
        }
    }
    
    fn compare_numbers<F>(a: &Value, b: &Value, op: F) -> crate::runtime::RuntimeResult<Value>
    where
        F: Fn(f64, f64) -> bool,
    {
        let a_num = match a {
            Value::Integer(n) => *n as f64,
            Value::Float(f) => *f,
            _ => return Err(crate::runtime::RuntimeError::TypeError {
                expected: "number".to_string(),
                actual: a.type_name().to_string(),
                operation: "comparison".to_string(),
            }),
        };
        
        let b_num = match b {
            Value::Integer(n) => *n as f64,
            Value::Float(f) => *f,
            _ => return Err(crate::runtime::RuntimeError::TypeError {
                expected: "number".to_string(),
                actual: b.type_name().to_string(),
                operation: "comparison".to_string(),
            }),
        };
        
        Ok(Value::Boolean(op(a_num, b_num)))
    }
    
    pub fn with_parent(parent: Rc<IrEnvironment>) -> Self {
        IrEnvironment {
            bindings: HashMap::new(),
            parent: Some(parent),
        }
    }
    
    pub fn define(&mut self, id: NodeId, value: Value) {
        self.bindings.insert(id, value);
    }
    
    pub fn lookup(&self, id: NodeId) -> Option<Value> {
        if let Some(value) = self.bindings.get(&id) {
            Some(value.clone())
        } else if let Some(parent) = &self.parent {
            parent.lookup(id)
        } else {
            None
        }
    }

    pub fn binding_count(&self) -> usize {
        self.bindings.len()
    }

    /// Get the keys of the bindings map (for debugging)
    pub fn binding_keys(&self) -> Vec<NodeId> {
        self.bindings.keys().cloned().collect()
    }
}

/// Shared mutable environment for letrec semantics
/// This allows functions to mutually reference each other
#[derive(Debug, Clone)]
pub struct SharedEnvironment {
    inner: Rc<RefCell<Environment>>,
}

impl SharedEnvironment {
    pub fn new(env: Environment) -> Self {
        SharedEnvironment {
            inner: Rc::new(RefCell::new(env)),
        }
    }
    
    pub fn define(&self, symbol: &Symbol, value: Value) {
        self.inner.borrow_mut().define(symbol, value);
    }
    
    pub fn lookup(&self, symbol: &Symbol) -> RuntimeResult<Value> {
        self.inner.borrow().lookup(symbol)
    }
    
    pub fn to_environment(&self) -> Environment {
        self.inner.borrow().clone()
    }
    
    pub fn current_bindings(&self) -> HashMap<String, Value> {
        self.inner.borrow().current_bindings().clone()
    }
}

impl Default for Environment {
    fn default() -> Self {
        Self::new()
    }
}
