//! Core MicroVM Types

use crate::ast::Expression;
use crate::runtime::security::RuntimeContext;
use crate::runtime::values::Value;
use std::time::Duration;

/// Supported script languages for sandboxed execution
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptLanguage {
    /// Python (2.x or 3.x)
    Python,
    /// JavaScript (Node.js)
    JavaScript,
    /// Bash/Shell script
    Shell,
    /// Ruby
    Ruby,
    /// Lua
    Lua,
    /// RTFS language
    Rtfs,
    /// WebAssembly
    Wasm,
    /// Custom language with interpreter path
    Custom { interpreter: String, file_ext: String },
}

impl ScriptLanguage {
    /// Get the interpreter command for this language
    pub fn interpreter(&self) -> &str {
        match self {
            ScriptLanguage::Python => "python",
            ScriptLanguage::JavaScript => "node",
            ScriptLanguage::Shell => "sh",
            ScriptLanguage::Ruby => "ruby",
            ScriptLanguage::Lua => "lua",
            ScriptLanguage::Rtfs => "rtfs",
            ScriptLanguage::Wasm => "wasmtime",
            ScriptLanguage::Custom { interpreter, .. } => interpreter,
        }
    }

    /// Get the file extension for this language
    pub fn file_extension(&self) -> &str {
        match self {
            ScriptLanguage::Python => "py",
            ScriptLanguage::JavaScript => "js",
            ScriptLanguage::Shell => "sh",
            ScriptLanguage::Ruby => "rb",
            ScriptLanguage::Lua => "lua",
            ScriptLanguage::Rtfs => "rtfs",
            ScriptLanguage::Wasm => "wasm",
            ScriptLanguage::Custom { file_ext, .. } => file_ext,
        }
    }

    /// Get the flag to execute a string of code
    pub fn execute_flag(&self) -> &str {
        match self {
            ScriptLanguage::Python => "-c",
            ScriptLanguage::JavaScript => "-e",
            ScriptLanguage::Shell => "-c",
            ScriptLanguage::Ruby => "-e",
            ScriptLanguage::Lua => "-e",
            ScriptLanguage::Rtfs => "-c",
            ScriptLanguage::Wasm => "", // wasmtime doesn't use -c for binary
            ScriptLanguage::Custom { .. } => "-c",
        }
    }

    /// Alternative interpreter paths to try in order
    pub fn interpreter_alternatives(&self) -> Vec<&str> {
        match self {
            ScriptLanguage::Python => vec!["/usr/bin/python", "/usr/bin/python3", "/usr/bin/python2"],
            ScriptLanguage::JavaScript => vec!["/usr/bin/node", "/usr/local/bin/node"],
            ScriptLanguage::Shell => vec!["/bin/sh", "/bin/bash"],
            ScriptLanguage::Ruby => vec!["/usr/bin/ruby"],
            ScriptLanguage::Lua => vec!["/usr/bin/lua", "/usr/bin/lua5.4", "/usr/bin/lua5.3"],
            ScriptLanguage::Rtfs => vec![],
            ScriptLanguage::Wasm => vec!["/usr/bin/wasmtime", "/usr/local/bin/wasmtime"],
            ScriptLanguage::Custom { interpreter, .. } => vec![interpreter.as_str()],
        }
    }

    /// Detect language from source code heuristics
    pub fn detect_from_source(source: &str) -> Option<ScriptLanguage> {
        let trimmed = source.trim();
        
        // Check shebang first
        if let Some(first_line) = trimmed.lines().next() {
            if first_line.starts_with("#!") {
                if first_line.contains("python") {
                    return Some(ScriptLanguage::Python);
                } else if first_line.contains("node") || first_line.contains("javascript") {
                    return Some(ScriptLanguage::JavaScript);
                } else if first_line.contains("ruby") {
                    return Some(ScriptLanguage::Ruby);
                } else if first_line.contains("lua") {
                    return Some(ScriptLanguage::Lua);
                } else if first_line.contains("sh") || first_line.contains("bash") {
                    return Some(ScriptLanguage::Shell);
                }
            }
        }
        
        // Heuristic detection based on syntax patterns
        // Python
        if source.contains("import ") || source.contains("def ") || source.contains("print(") {
            return Some(ScriptLanguage::Python);
        }
        
        // JavaScript
        if source.contains("const ") || source.contains("let ") || source.contains("function ") 
           || source.contains("console.log") || source.contains("require(") || source.contains("import {") {
            return Some(ScriptLanguage::JavaScript);
        }
        
        // Ruby
        if source.contains("puts ") || source.contains("def ") && source.contains("end") {
            return Some(ScriptLanguage::Ruby);
        }
        
        None
    }
}

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
    ExternalProgram { path: String, args: Vec<String> },
    /// RTFS source code to parse and execute (legacy, prefer ScriptSource)
    RtfsSource(String),
    /// Script source with explicit language tag for sandboxed execution
    ScriptSource { language: ScriptLanguage, source: String },
    /// Binary source (e.g. WASM) with explicit language tag
    Binary { language: ScriptLanguage, source: Vec<u8> },
}

impl Program {
    /// Check if this program performs network operations
    pub fn is_network_operation(&self) -> bool {
        match self {
            Program::RtfsSource(source) => source.contains("http") || source.contains("network"),
            Program::RtfsAst(ast) => {
                format!("{:?}", ast).contains("http") || format!("{:?}", ast).contains("network")
            }
            Program::ExternalProgram { path, args } => {
                path.contains("curl")
                    || path.contains("wget")
                    || args
                        .iter()
                        .any(|arg| arg.contains("http") || arg.contains("network"))
            }
            _ => false,
        }
    }

    /// Check if this program performs file operations
    pub fn is_file_operation(&self) -> bool {
        match self {
            Program::RtfsSource(source) => source.contains("file") || source.contains("io"),
            Program::RtfsAst(ast) => {
                format!("{:?}", ast).contains("file") || format!("{:?}", ast).contains("io")
            }
            Program::ExternalProgram { path, args } => {
                path.contains("cat")
                    || path.contains("ls")
                    || path.contains("cp")
                    || args
                        .iter()
                        .any(|arg| arg.contains("file") || arg.contains("io"))
            }
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
            }
            Program::ScriptSource { language, source } => {
                format!("{:?} script: {} bytes", language, source.len())
            }
            Program::Binary { language, source } => {
                format!("{:?} binary: {} bytes", language, source.len())
            }
        }
    }

    /// Try to detect the language and convert RtfsSource to ScriptSource
    pub fn with_detected_language(self) -> Self {
        match self {
            Program::RtfsSource(source) => {
                if let Some(lang) = ScriptLanguage::detect_from_source(&source) {
                    Program::ScriptSource { language: lang, source }
                } else {
                    Program::RtfsSource(source)
                }
            }
            other => other,
        }
    }

    /// Create a Python script program
    pub fn python(source: impl Into<String>) -> Self {
        Program::ScriptSource {
            language: ScriptLanguage::Python,
            source: source.into(),
        }
    }

    /// Create a JavaScript script program
    pub fn javascript(source: impl Into<String>) -> Self {
        Program::ScriptSource {
            language: ScriptLanguage::JavaScript,
            source: source.into(),
        }
    }

    /// Create a Shell script program
    pub fn shell(source: impl Into<String>) -> Self {
        Program::ScriptSource {
            language: ScriptLanguage::Shell,
            source: source.into(),
        }
    }

    /// Create a WASM binary program
    pub fn wasm(source: Vec<u8>) -> Self {
        Program::Binary {
            language: ScriptLanguage::Wasm,
            source,
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
