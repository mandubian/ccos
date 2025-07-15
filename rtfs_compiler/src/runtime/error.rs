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

    SymbolNotFound(String),
    ModuleNotFound(String),

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
    
    Generic(String),

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
    
    /// Agent discovery and communication errors
    AgentDiscoveryError {
        message: String,
        registry_uri: String,
    },
    
    /// Agent communication errors
    AgentCommunicationError {
        message: String,
        agent_id: String,
        endpoint: String,
    },
    
    /// Agent profile parsing/validation errors
    AgentProfileError {
        message: String,
        profile_uri: Option<String>,
    },

    /// Custom application errors
    ApplicationError {
        error_type: Keyword,
        message: String,
        data: Option<Value>,
    },
    
    /// Unknown capability error
    UnknownCapability(String),
    
    /// Security violation error
    SecurityViolation {
        operation: String,
        capability: String,
        context: String,
    },
    
    /// Invalid program structure (for IR runtime)
    InvalidProgram(String),
    
    /// Not implemented functionality
    NotImplemented(String),
    
    /// Value is not callable
    NotCallable(String),

    /// Internal runtime error, e.g. for logic errors in the interpreter
    InternalError(String),
    
    /// Internal error for tail call optimization
    TailCall {
        function: Value,
        args: Vec<Value>,
    },
    
    /// Stack overflow error
    StackOverflow(String),

    InvalidTaskDefinition(String),

    InvalidParallelExpression,
}

impl RuntimeError {
    pub fn new(message: &str) -> RuntimeError {
        RuntimeError::Generic(message.to_string())
    }
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
            RuntimeError::SymbolNotFound(symbol) => {
                write!(f, "Symbol not found: {}", symbol)
            },
            RuntimeError::ModuleNotFound(module) => {
                write!(f, "Module not found: {}", module)
            },
            RuntimeError::ArityMismatch { function, expected, actual } => {
                write!(f, "Arity mismatch in {}: expected {}, got {}", function, expected, actual)
            },
            RuntimeError::DivisionByZero => {
                write!(f, "Division by zero")
            },
            RuntimeError::IndexOutOfBounds { index, length } => {
                write!(f, "Index out of bounds: index {} is not in range [0, {})", index, length)
            },
            RuntimeError::KeyNotFound { key } => write!(f, "Key not found: {}", key),
            RuntimeError::Generic(message) => write!(f, "Runtime error: {}", message),
            RuntimeError::ResourceError { resource_type, message } => {
                write!(f, "Resource error ({}): {}", resource_type, message)
            }
            RuntimeError::InvalidTaskDefinition(message) => {
                write!(f, "Invalid task definition: {}", message)
            },
            RuntimeError::InvalidParallelExpression => {
                write!(f, "Invalid parallel expression")
            },
            RuntimeError::IoError(msg) => {
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
            },
            RuntimeError::AgentDiscoveryError { message, registry_uri } => {
                write!(f, "Agent discovery error: {} (registry: {})", message, registry_uri)
            },
            RuntimeError::AgentCommunicationError { message, agent_id, endpoint } => {
                write!(f, "Agent communication error: {} (agent: {}, endpoint: {})", message, agent_id, endpoint)
            },
            RuntimeError::AgentProfileError { message, profile_uri } => {
                match profile_uri {
                    Some(uri) => write!(f, "Agent profile error: {} (profile: {})", message, uri),
                    None => write!(f, "Agent profile error: {}", message),
                }
            },

            RuntimeError::ApplicationError { error_type, message, .. } => {
                write!(f, "Application error ({}): {}", error_type.0, message)
            },
            RuntimeError::UnknownCapability(capability) => {
                write!(f, "Unknown capability: {}", capability)
            },
            RuntimeError::SecurityViolation { operation, capability, context } => {
                write!(f, "Security violation: {} operation on capability '{}' not allowed in context: {}", operation, capability, context)
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
            RuntimeError::TailCall { function, args } => {
                write!(f, "Tail call to function {:?} with args {:?}", function, args)
            },
            RuntimeError::StackOverflow(msg) => {
                write!(f, "Stack overflow: {}", msg)
            },
        }
    }
}

impl std::error::Error for RuntimeError {}

// TODO: Re-enable IR converter integration when ir_converter module is available
// impl From<crate::ir_converter::IrConversionError> for RuntimeError {
//     fn from(err: crate::ir_converter::IrConversionError) -> Self {
//         // Implementation will go here when ir_converter is available
//     }
// }

/// Convert runtime errors to RTFS error values
impl RuntimeError {    pub fn to_value(&self) -> Value {
        let message = match self {
            RuntimeError::TypeError { expected, actual, operation } => {
                format!("Type error in {}: expected {}, got {}", operation, expected, actual)
            },
            RuntimeError::UndefinedSymbol(symbol) => {
                format!("Undefined symbol: {}", symbol.0)
            },
            RuntimeError::SymbolNotFound(symbol) => {
                format!("Symbol not found: {}", symbol)
            },
            RuntimeError::ModuleNotFound(module) => {
                format!("Module not found: {}", module)
            },
            RuntimeError::ArityMismatch { function, expected, actual } => {
                format!("Arity mismatch in {}: expected {}, got {}", function, expected, actual)
            },
            RuntimeError::DivisionByZero => {
                "Division by zero".to_string()
            },
            RuntimeError::IndexOutOfBounds { index, length } => {
                format!("Index {} out of bounds for collection of length {}", index, length)
            },
            RuntimeError::KeyNotFound { key } => {
                format!("Key not found: {}", key)
            },
            _ => {
                self.to_string()
            },
        };
        
        Value::Error(crate::runtime::values::ErrorValue {
            message,
            stack_trace: None,
        })
    }
}

use crate::ir::converter::IrConversionError; // TODO: Re-enable when IR is integrated

/// Represents a location in the source code, pointing to a specific node in the AST.
/// This is crucial for providing meaningful error messages that can direct the user
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Location {
    // pub node_id: NodeId, // TODO: Re-enable when IR is integrated
    pub source_text: Option<String>,
}

impl Location {
    // pub fn new(node_id: NodeId) -> Self {
    //     Self { node_id, source_text: None }
    // }

    pub fn with_source_text(mut self, source_text: String) -> Self {
        self.source_text = Some(source_text);
        self
    }
}
