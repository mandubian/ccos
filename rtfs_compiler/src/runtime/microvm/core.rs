//! Core MicroVM Types

use crate::runtime::values::Value;
use crate::runtime::security::RuntimeContext;
use crate::ast::Expression;
use std::time::Duration;

/// Program representation for MicroVM execution
#[derive(Debug, Clone)]
pub enum Program {
    /// RTFS bytecode to execute
    RtfsBytecode(Vec<u8>),
    /// RTFS AST to interpret
    RtfsAst(Box<Expression>),
    /// Native function pointer (for trusted code)
    NativeFunction(fn(Vec<Value>) -> crate::runtime::error::RuntimeResult<Value>),
    /// External program (for process-based isolation)
    ExternalProgram {
        path: String,
        args: Vec<String>,
    },
    /// RTFS source code to parse and execute
    RtfsSource(String),
}

impl Program {
    /// Check if this program performs network operations
    pub fn is_network_operation(&self) -> bool {
        match self {
            Program::RtfsSource(source) => source.contains("http") || source.contains("network"),
            Program::RtfsAst(ast) => format!("{:?}", ast).contains("http") || format!("{:?}", ast).contains("network"),
            Program::ExternalProgram { path, args } => {
                path.contains("curl") || path.contains("wget") || 
                args.iter().any(|arg| arg.contains("http") || arg.contains("network"))
            },
            _ => false,
        }
    }
    
    /// Check if this program performs file operations
    pub fn is_file_operation(&self) -> bool {
        match self {
            Program::RtfsSource(source) => source.contains("file") || source.contains("io"),
            Program::RtfsAst(ast) => format!("{:?}", ast).contains("file") || format!("{:?}", ast).contains("io"),
            Program::ExternalProgram { path, args } => {
                path.contains("cat") || path.contains("ls") || path.contains("cp") ||
                args.iter().any(|arg| arg.contains("file") || arg.contains("io"))
            },
            _ => false,
        }
    }

    /// Get a human-readable description of this program
    pub fn description(&self) -> String {
        match self {
            Program::RtfsSource(source) => format!("RTFS source: {}", source),
            Program::RtfsAst(_) => "RTFS AST".to_string(),
            Program::RtfsBytecode(bytes) => format!("RTFS bytecode ({} bytes)", bytes.len()),
            Program::NativeFunction(_) => "Native function".to_string(),
            Program::ExternalProgram { path, args } => {
                format!("External program: {} {:?}", path, args)
            },
        }
    }
}

/// Execution context for MicroVM operations
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    /// Unique identifier for this execution
    pub execution_id: String,
    /// Program to execute (NEW: supports arbitrary programs)
    pub program: Option<Program>,
    /// Capability being executed (for backward compatibility)
    pub capability_id: Option<String>,
    /// Capability permissions for program execution (NEW)
    pub capability_permissions: Vec<String>,
    /// Arguments passed to the capability/program
    pub args: Vec<Value>,
    /// Configuration for this execution
    pub config: crate::runtime::microvm::config::MicroVMConfig,
    /// Runtime context for security and capability control (NEW)
    pub runtime_context: Option<RuntimeContext>,
}

/// Result of a MicroVM execution
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    /// Return value from the operation
    pub value: Value,
    /// Execution metadata
    pub metadata: ExecutionMetadata,
}

/// Metadata about execution performance and operations
#[derive(Debug, Clone)]
pub struct ExecutionMetadata {
    /// Actual execution time
    pub duration: Duration,
    /// Memory used during execution
    pub memory_used_mb: u64,
    /// CPU time used
    pub cpu_time: Duration,
    /// Network requests made (if any)
    pub network_requests: Vec<NetworkRequest>,
    /// File operations performed (if any)
    pub file_operations: Vec<FileOperation>,
}

/// Network request metadata
#[derive(Debug, Clone)]
pub struct NetworkRequest {
    pub url: String,
    pub method: String,
    pub status_code: Option<u16>,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

/// File operation metadata
#[derive(Debug, Clone)]
pub struct FileOperation {
    pub path: String,
    pub operation: String, // "read", "write", "create", "delete", etc.
    pub bytes_processed: u64,
}
