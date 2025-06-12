// Error handling for the RTFS runtime

use std::fmt;
use crate::ast::{Symbol, Keyword};
use crate::runtime::Value;

pub type RuntimeResult<T> = Result<T, RuntimeError>;

/// Runtime errors that can occur during RTFS execution
#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeError {
    /// Type errors (wrong type for operation)
    TypeError {
        expected: String,
        actual: String,
        operation: String,
    },
    
    /// Undefined symbol/variable
    UndefinedSymbol(Symbol),
    
    /// Arity mismatch (wrong number of arguments)
    ArityMismatch {
        function: String,
        expected: String,
        actual: usize,
    },
    
    /// Division by zero
    DivisionByZero,
    
    /// Index out of bounds
    IndexOutOfBounds {
        index: i64,
        length: usize,
    },
    
    /// Key not found in map
    KeyNotFound {
        key: String,
    },
    
    /// Resource errors
    ResourceError {
        resource_type: String,
        message: String,
    },
      /// I/O errors
    IoError(String),
    
    /// Module loading/execution errors
    ModuleError(String),
    
    /// Invalid argument errors
    InvalidArgument(String),
    
    /// Network errors
    NetworkError(String),
    
    /// JSON parsing errors
    JsonError(String),
    
    /// Pattern matching errors
    MatchError(String),
      /// Custom application errors
    ApplicationError {
        error_type: Keyword,
        message: String,
        data: Option<Value>,
    },
    
    /// Invalid program structure (for IR runtime)
    InvalidProgram(String),
    
    /// Not implemented functionality
    NotImplemented(String),
    
    /// Value is not callable
    NotCallable(String),
    
    /// Internal runtime errors (should not normally occur)
    InternalError(String),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuntimeError::TypeError { expected, actual, operation } => {
                write!(f, "Type error in {}: expected {}, got {}", operation, expected, actual)
            },
            RuntimeError::UndefinedSymbol(symbol) => {
                write!(f, "Undefined symbol: {}", symbol.0)
            },
            RuntimeError::ArityMismatch { function, expected, actual } => {
                write!(f, "Arity mismatch in {}: expected {}, got {}", function, expected, actual)
            },
            RuntimeError::DivisionByZero => {
                write!(f, "Division by zero")
            },
            RuntimeError::IndexOutOfBounds { index, length } => {
                write!(f, "Index {} out of bounds for collection of length {}", index, length)
            },
            RuntimeError::KeyNotFound { key } => {
                write!(f, "Key not found: {}", key)
            },
            RuntimeError::ResourceError { resource_type, message } => {
                write!(f, "Resource error ({}): {}", resource_type, message)
            },            RuntimeError::IoError(msg) => {
                write!(f, "I/O error: {}", msg)
            },
            RuntimeError::ModuleError(msg) => {
                write!(f, "Module error: {}", msg)
            },
            RuntimeError::InvalidArgument(msg) => {
                write!(f, "Invalid argument: {}", msg)
            },
            RuntimeError::NetworkError(msg) => {
                write!(f, "Network error: {}", msg)
            },
            RuntimeError::JsonError(msg) => {
                write!(f, "JSON error: {}", msg)
            },
            RuntimeError::MatchError(msg) => {
                write!(f, "Match error: {}", msg)
            },            RuntimeError::ApplicationError { error_type, message, .. } => {
                write!(f, "Application error ({}): {}", error_type.0, message)
            },
            RuntimeError::InvalidProgram(msg) => {
                write!(f, "Invalid program: {}", msg)
            },
            RuntimeError::NotImplemented(msg) => {
                write!(f, "Not implemented: {}", msg)
            },
            RuntimeError::NotCallable(msg) => {
                write!(f, "Not callable: {}", msg)
            },
            RuntimeError::InternalError(msg) => {
                write!(f, "Internal error: {}", msg)
            },
        }
    }
}

impl std::error::Error for RuntimeError {}

/// Convert runtime errors to RTFS error values
impl RuntimeError {
    pub fn to_value(&self) -> Value {
        use std::collections::HashMap;
        
        let (error_type, message, data) = match self {
            RuntimeError::TypeError { expected, actual, operation } => (
                Keyword("error/type".to_string()),
                format!("Type error in {}: expected {}, got {}", operation, expected, actual),
                Some({
                    let mut map = HashMap::new();
                    map.insert("expected".to_string(), Value::String(expected.clone()));
                    map.insert("actual".to_string(), Value::String(actual.clone()));
                    map.insert("operation".to_string(), Value::String(operation.clone()));
                    Value::Map(map.into_iter().map(|(k, v)| (crate::ast::MapKey::String(k), v)).collect())
                })
            ),
            RuntimeError::UndefinedSymbol(symbol) => (
                Keyword("error/undefined-symbol".to_string()),
                format!("Undefined symbol: {}", symbol.0),
                Some({
                    let mut map = HashMap::new();
                    map.insert("symbol".to_string(), Value::String(symbol.0.clone()));
                    Value::Map(map.into_iter().map(|(k, v)| (crate::ast::MapKey::String(k), v)).collect())
                })
            ),
            RuntimeError::ArityMismatch { function, expected, actual } => (
                Keyword("error/arity-mismatch".to_string()),
                format!("Arity mismatch in {}: expected {}, got {}", function, expected, actual),
                Some({
                    let mut map = HashMap::new();
                    map.insert("function".to_string(), Value::String(function.clone()));
                    map.insert("expected".to_string(), Value::String(expected.clone()));
                    map.insert("actual".to_string(), Value::Integer(*actual as i64));
                    Value::Map(map.into_iter().map(|(k, v)| (crate::ast::MapKey::String(k), v)).collect())
                })
            ),
            RuntimeError::DivisionByZero => (
                Keyword("error/arithmetic".to_string()),
                "Division by zero".to_string(),
                None
            ),
            RuntimeError::IndexOutOfBounds { index, length } => (
                Keyword("error/index-out-of-bounds".to_string()),
                format!("Index {} out of bounds for collection of length {}", index, length),
                Some({
                    let mut map = HashMap::new();
                    map.insert("index".to_string(), Value::Integer(*index));
                    map.insert("length".to_string(), Value::Integer(*length as i64));
                    Value::Map(map.into_iter().map(|(k, v)| (crate::ast::MapKey::String(k), v)).collect())
                })
            ),
            RuntimeError::KeyNotFound { key } => (
                Keyword("error/key-not-found".to_string()),
                format!("Key not found: {}", key),
                Some({
                    let mut map = HashMap::new();
                    map.insert("key".to_string(), Value::String(key.clone()));
                    Value::Map(map.into_iter().map(|(k, v)| (crate::ast::MapKey::String(k), v)).collect())
                })
            ),
            RuntimeError::ApplicationError { error_type, message, data } => (
                error_type.clone(),
                message.clone(),
                data.clone()
            ),
            _ => (
                Keyword("error/runtime".to_string()),
                self.to_string(),
                None
            ),
        };
        
        Value::Error(crate::runtime::values::ErrorValue {
            error_type,
            message,
            data: data.map(|v| match v {
                Value::Map(m) => m.into_iter().map(|(k, v)| {
                    let key = match k {
                        crate::ast::MapKey::String(s) => s,
                        crate::ast::MapKey::Keyword(kw) => kw.0,
                        crate::ast::MapKey::Integer(i) => i.to_string(),
                    };
                    (key, v)
                }).collect(),
                _ => HashMap::new(),
            }),
        })
    }
}
