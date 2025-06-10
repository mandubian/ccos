// Runtime value system for RTFS
// Represents values during execution (different from AST which represents parsed code)

use std::collections::HashMap;
use crate::ast::{Symbol, Keyword, MapKey};

/// Runtime values in RTFS
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    // Primitive values
    Integer(i64),
    Float(f64),
    String(String),
    Boolean(bool),
    Keyword(Keyword),
    Symbol(Symbol),
    Nil,
    
    // Collection values
    Vector(Vec<Value>),
    Map(HashMap<MapKey, Value>),
    
    // Function values
    Function(Function),
    
    // Special values
    Resource(ResourceHandle),
    
    // Result types for tool operations
    Ok(Box<Value>),
    Error(ErrorValue),
}

/// Function representation at runtime
#[derive(Debug, Clone)]
pub enum Function {
    /// Built-in functions (implemented in Rust)
    Builtin {
        name: String,
        arity: Arity,
        func: fn(&[Value]) -> crate::runtime::RuntimeResult<Value>,
    },
    
    /// User-defined functions (defined in RTFS)
    UserDefined {
        params: Vec<crate::ast::ParamDef>,
        variadic_param: Option<crate::ast::ParamDef>,
        body: Vec<crate::ast::Expression>,
        closure: crate::runtime::Environment, // Captured environment
    },
}

/// Function arity specification
#[derive(Debug, Clone, PartialEq)]
pub enum Arity {
    Exact(usize),           // Exactly n arguments
    AtLeast(usize),         // At least n arguments (variadic)
    Range(usize, usize),    // Between min and max arguments
    Any,                    // Any number of arguments
}

/// Resource state tracking for lifecycle management
#[derive(Debug, Clone, PartialEq)]
pub enum ResourceState {
    Active,    // Resource is available for use
    Released,  // Resource has been cleaned up and cannot be used
}

/// Resource handle for external resources
#[derive(Debug, Clone, PartialEq)]
pub struct ResourceHandle {
    pub id: String,
    pub resource_type: String,
    pub metadata: HashMap<String, Value>,
    pub state: ResourceState,
}

/// Error value for runtime errors
#[derive(Debug, Clone, PartialEq)]
pub struct ErrorValue {
    pub error_type: Keyword,
    pub message: String,
    pub data: Option<HashMap<String, Value>>,
}

impl Value {
    /// Check if a value is truthy (everything except false and nil)
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Boolean(false) | Value::Nil => false,
            _ => true,
        }
    }
    
    /// Convert value to string representation
    pub fn to_string(&self) -> String {
        match self {
            Value::Integer(n) => n.to_string(),
            Value::Float(f) => f.to_string(),
            Value::String(s) => s.clone(),
            Value::Boolean(b) => b.to_string(),
            Value::Keyword(k) => format!(":{}", k.0),
            Value::Symbol(s) => s.0.clone(),
            Value::Nil => "nil".to_string(),
            Value::Vector(v) => {
                let elements: Vec<String> = v.iter().map(|x| x.to_string()).collect();
                format!("[{}]", elements.join(" "))
            },
            Value::Map(m) => {
                let entries: Vec<String> = m.iter().map(|(k, v)| {
                    let key_str = match k {
                        MapKey::Keyword(kw) => format!(":{}", kw.0),
                        MapKey::String(s) => format!("\"{}\"", s),
                        MapKey::Integer(i) => i.to_string(),
                    };
                    format!("{} {}", key_str, v.to_string())
                }).collect();
                format!("{{{}}}", entries.join(" "))
            },
            Value::Function(_) => "#<function>".to_string(),
            Value::Resource(h) => format!("#<resource:{}>", h.resource_type),
            Value::Ok(v) => format!("[:ok {}]", v.to_string()),
            Value::Error(e) => format!("[:error :{} \"{}\"]", e.error_type.0, e.message),
        }
    }
    
    /// Get the type name of a value
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Integer(_) => "int",
            Value::Float(_) => "float", 
            Value::String(_) => "string",
            Value::Boolean(_) => "bool",
            Value::Keyword(_) => "keyword",
            Value::Symbol(_) => "symbol",
            Value::Nil => "nil",
            Value::Vector(_) => "vector",
            Value::Map(_) => "map",
            Value::Function(_) => "function",
            Value::Resource(_) => "resource",
            Value::Ok(_) => "ok",
            Value::Error(_) => "error",
        }
    }
}

// Implement PartialEq for Function manually since function pointers don't implement it
impl PartialEq for Function {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Function::Builtin { name: n1, arity: a1, .. }, 
             Function::Builtin { name: n2, arity: a2, .. }) => {
                n1 == n2 && a1 == a2
            },
            (Function::UserDefined { params: p1, variadic_param: v1, body: b1, .. },
             Function::UserDefined { params: p2, variadic_param: v2, body: b2, .. }) => {
                p1 == p2 && v1 == v2 && b1 == b2
            },
            _ => false,
        }
    }
}
