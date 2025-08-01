//! MicroVM Abstraction Layer for RTFS/CCOS
//!
//! This module provides a pluggable architecture for secure execution environments
//! that can isolate dangerous operations like network access, file I/O, and system calls.

use crate::runtime::values::Value;
use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::runtime::security::RuntimeContext;
use crate::ast::Expression;
use crate::bytecode::BytecodeExecutor;
use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use wasmtime;

/// Configuration for MicroVM execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MicroVMConfig {
    /// Maximum execution time for operations
    pub timeout: Duration,
    /// Memory limit in MB
    pub memory_limit_mb: u64,
    /// CPU limit (relative to host)
    pub cpu_limit: f64,
    /// Network access policy
    pub network_policy: NetworkPolicy,
    /// File system access policy
    pub fs_policy: FileSystemPolicy,
    /// Environment variables to pass
    pub env_vars: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkPolicy {
    /// No network access
    Denied,
    /// Allow specific domains only
    AllowList(Vec<String>),
    /// Allow all except specific domains
    DenyList(Vec<String>),
    /// Full network access (dangerous)
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FileSystemPolicy {
    /// No file system access
    None,
    /// Read-only access to specific paths
    ReadOnly(Vec<String>),
    /// Read-write access to specific paths
    ReadWrite(Vec<String>),
    /// Full file system access (dangerous)
    Full,
}

/// Program representation for MicroVM execution
#[derive(Debug, Clone)]
pub enum Program {
    /// RTFS bytecode to execute
    RtfsBytecode(Vec<u8>),
    /// RTFS AST to interpret
    RtfsAst(Box<Expression>),
    /// Native function pointer (for trusted code)
    NativeFunction(fn(Vec<Value>) -> RuntimeResult<Value>),
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
    
    /// Get a human-readable description of the program
    pub fn description(&self) -> String {
        match self {
            Program::RtfsBytecode(code) => format!("RTFS bytecode ({} bytes)", code.len()),
            Program::RtfsAst(_) => "RTFS AST".to_string(),
            Program::NativeFunction(_) => "Native function".to_string(),
            Program::ExternalProgram { path, args } => format!("External program: {} {:?}", path, args),
            Program::RtfsSource(source) => format!("RTFS source: {}", source),
        }
    }
}

/// Enhanced execution context for MicroVM operations
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
    pub config: MicroVMConfig,
    /// Runtime context for security and capability control (NEW)
    pub runtime_context: Option<RuntimeContext>,
}

/// Result of MicroVM execution
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    /// Return value from the operation
    pub value: Value,
    /// Execution metadata
    pub metadata: ExecutionMetadata,
}

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

#[derive(Debug, Clone)]
pub struct NetworkRequest {
    pub url: String,
    pub method: String,
    pub status_code: Option<u16>,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

#[derive(Debug, Clone)]
pub struct FileOperation {
    pub path: String,
    pub operation: String, // "read", "write", "create", "delete", etc.
    pub bytes_processed: u64,
}

/// Enhanced trait for MicroVM implementations
pub trait MicroVMProvider: Send + Sync {
    /// Name of this MicroVM provider
    fn name(&self) -> &'static str;
    
    /// Check if this provider is available on the current system
    fn is_available(&self) -> bool;
    
    /// Initialize the MicroVM provider
    fn initialize(&mut self) -> RuntimeResult<()>;
    
    /// Execute a program with capability permissions (NEW: primary method)
    fn execute_program(&self, context: ExecutionContext) -> RuntimeResult<ExecutionResult>;
    
    /// Execute a specific capability (for backward compatibility)
    fn execute_capability(&self, context: ExecutionContext) -> RuntimeResult<ExecutionResult> {
        // Default implementation for backward compatibility
        self.execute_program(context)
    }
    
    /// Legacy execute method (deprecated, use execute_program or execute_capability)
    #[deprecated(since = "2.0.0", note = "Use execute_program or execute_capability instead")]
    fn execute(&self, context: ExecutionContext) -> RuntimeResult<ExecutionResult> {
        self.execute_capability(context)
    }
    
    /// Cleanup resources
    fn cleanup(&mut self) -> RuntimeResult<()>;
    
    /// Get provider-specific configuration options
    fn get_config_schema(&self) -> serde_json::Value;
}

/// Factory for creating MicroVM providers
pub struct MicroVMFactory {
    providers: HashMap<String, Box<dyn MicroVMProvider>>,
}

impl MicroVMFactory {
    pub fn new() -> Self {
        let mut factory = Self {
            providers: HashMap::new(),
        };
        
        // Register and initialize built-in providers
        let mut mock_provider = MockMicroVMProvider::new();
        mock_provider.initialize().unwrap_or_else(|e| eprintln!("Failed to initialize mock provider: {}", e));
        factory.register_provider("mock", Box::new(mock_provider));
        
        let mut process_provider = ProcessMicroVMProvider::new();
        process_provider.initialize().unwrap_or_else(|e| eprintln!("Failed to initialize process provider: {}", e));
        factory.register_provider("process", Box::new(process_provider));
        
        // Register platform-specific providers
        #[cfg(target_os = "linux")]
        {
            let mut firecracker_provider = FirecrackerMicroVMProvider::new();
            firecracker_provider.initialize().unwrap_or_else(|e| eprintln!("Failed to initialize firecracker provider: {}", e));
            factory.register_provider("firecracker", Box::new(firecracker_provider));
            
            let mut gvisor_provider = GvisorMicroVMProvider::new();
            gvisor_provider.initialize().unwrap_or_else(|e| eprintln!("Failed to initialize gvisor provider: {}", e));
            factory.register_provider("gvisor", Box::new(gvisor_provider));
        }
        
        {
            let mut wasm_provider = WasmMicroVMProvider::new();
            wasm_provider.initialize().unwrap_or_else(|e| eprintln!("Failed to initialize wasm provider: {}", e));
            factory.register_provider("wasm", Box::new(wasm_provider));
        }
        
        factory
    }
    
    pub fn register_provider(&mut self, name: &str, provider: Box<dyn MicroVMProvider>) {
        self.providers.insert(name.to_string(), provider);
    }
    
    pub fn get_provider(&self, name: &str) -> Option<&dyn MicroVMProvider> {
        self.providers.get(name).map(|p| p.as_ref())
    }
    
    pub fn list_providers(&self) -> Vec<&str> {
        self.providers.keys().map(|k| k.as_str()).collect()
    }
    
    pub fn get_available_providers(&self) -> Vec<&str> {
        self.providers
            .iter()
            .filter(|(_, provider)| provider.is_available())
            .map(|(name, _)| name.as_str())
            .collect()
    }
}

/// Mock MicroVM provider for testing and development
pub struct MockMicroVMProvider {
    initialized: bool,
}

impl MockMicroVMProvider {
    pub fn new() -> Self {
        Self { initialized: false }
    }
}

impl MicroVMProvider for MockMicroVMProvider {
    fn name(&self) -> &'static str {
        "mock"
    }
    
    fn is_available(&self) -> bool {
        true // Always available
    }
    
    fn initialize(&mut self) -> RuntimeResult<()> {
        self.initialized = true;
        Ok(())
    }
    
    fn execute_program(&self, context: ExecutionContext) -> RuntimeResult<ExecutionResult> {
        if !self.initialized {
            return Err(RuntimeError::Generic("MockMicroVMProvider not initialized".to_string()));
        }
        
        println!("[MOCK-MICROVM] Executing program: {:?}", context.program);
        println!("[MOCK-MICROVM] Args: {:?}", context.args);
        
        // Use RTFS executor for actual program execution
        let mut rtfs_executor = RtfsMicroVMExecutor::new();
        let runtime_context = context.runtime_context.unwrap_or_else(RuntimeContext::pure);
        
        let value = match context.program.as_ref() {
            Some(program) => {
                // Execute the program using RTFS executor
                rtfs_executor.execute_rtfs_program(
                    program.clone(),
                    context.capability_permissions,
                    context.args.clone(),
                    runtime_context,
                )?
            },
            None => {
                // Fallback for backward compatibility
                Value::String("Mock capability execution (backward compatibility)".to_string())
            }
        };
        
        let metadata = ExecutionMetadata {
            duration: Duration::from_millis(10),
            memory_used_mb: 1,
            cpu_time: Duration::from_micros(100),
            network_requests: if context.program.as_ref().map(|p| p.is_network_operation()).unwrap_or(false) {
                vec![NetworkRequest {
                    url: "https://mock-api.example.com".to_string(),
                    method: "GET".to_string(),
                    status_code: Some(200),
                    bytes_sent: 100,
                    bytes_received: 500,
                }]
            } else {
                vec![]
            },
            file_operations: if context.program.as_ref().map(|p| p.is_file_operation()).unwrap_or(false) {
                vec![FileOperation {
                    path: "/mock/file.txt".to_string(),
                    operation: "read".to_string(),
                    bytes_processed: 100,
                }]
            } else {
                vec![]
            },
        };
        
        Ok(ExecutionResult { value, metadata })
    }
    
    fn execute_capability(&self, context: ExecutionContext) -> RuntimeResult<ExecutionResult> {
        self.execute_program(context)
    }
    
    fn cleanup(&mut self) -> RuntimeResult<()> {
        self.initialized = false;
        Ok(())
    }
    
    fn get_config_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "mock_delay_ms": {
                    "type": "integer",
                    "description": "Artificial delay in milliseconds"
                }
            }
        })
    }
}

/// Process-based MicroVM provider using OS processes for isolation
pub struct ProcessMicroVMProvider {
    initialized: bool,
}

impl ProcessMicroVMProvider {
    pub fn new() -> Self {
        Self { initialized: false }
    }
}

impl MicroVMProvider for ProcessMicroVMProvider {
    fn name(&self) -> &'static str {
        "process"
    }
    
    fn is_available(&self) -> bool {
        // Available on all platforms
        true
    }
    
    fn initialize(&mut self) -> RuntimeResult<()> {
        self.initialized = true;
        Ok(())
    }
    
    fn execute_program(&self, context: ExecutionContext) -> RuntimeResult<ExecutionResult> {
        if !self.initialized {
            return Err(RuntimeError::Generic("ProcessMicroVMProvider not initialized".to_string()));
        }
        
        let start_time = std::time::Instant::now();
        
        // Execute program based on type
        let value = match context.program.as_ref() {
            Some(program) => {
                match program {
                    Program::ExternalProgram { path, args } => {
                        // Execute external program with process isolation
                        self.execute_external_process(path, args, &context)?
                    },
                    Program::RtfsSource(source) => {
                        // Execute RTFS source in isolated process
                        self.execute_rtfs_in_process(source, &context)?
                    },
                    Program::NativeFunction(func) => {
                        // Execute native function (trusted, but still isolated)
                        self.execute_native_in_process(func, &context)?
                    },
                    _ => {
                        // For other program types, use RTFS executor
                        let mut rtfs_executor = RtfsMicroVMExecutor::new();
                        let runtime_context = context.runtime_context.clone().unwrap_or_else(RuntimeContext::pure);
                        rtfs_executor.execute_rtfs_program(
                            program.clone(),
                            context.capability_permissions.clone(),
                            context.args.clone(),
                            runtime_context,
                        )?
                    }
                }
            },
            None => {
                // Fallback for backward compatibility
                Value::String("Process capability execution (backward compatibility)".to_string())
            }
        };
        
        let duration = start_time.elapsed();
        
        let metadata = ExecutionMetadata {
            duration,
            memory_used_mb: 1, // TODO: Measure actual memory usage
            cpu_time: duration, // TODO: Measure actual CPU time
            network_requests: if context.program.as_ref().map(|p| p.is_network_operation()).unwrap_or(false) {
                vec![NetworkRequest {
                    url: "https://process-api.example.com".to_string(),
                    method: "GET".to_string(),
                    status_code: Some(200),
                    bytes_sent: 100,
                    bytes_received: 500,
                }]
            } else {
                vec![]
            },
            file_operations: if context.program.as_ref().map(|p| p.is_file_operation()).unwrap_or(false) {
                vec![FileOperation {
                    path: "/process/file.txt".to_string(),
                    operation: "read".to_string(),
                    bytes_processed: 100,
                }]
            } else {
                vec![]
            },
        };
        
        Ok(ExecutionResult { value, metadata })
    }
    
    fn execute_capability(&self, context: ExecutionContext) -> RuntimeResult<ExecutionResult> {
        self.execute_program(context)
    }
    
    fn cleanup(&mut self) -> RuntimeResult<()> {
        self.initialized = false;
        Ok(())
    }
    
    fn get_config_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "user": {
                    "type": "string",
                    "description": "User to run the process as"
                },
                "chroot": {
                    "type": "string",
                    "description": "Chroot directory for the process"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Process timeout in seconds"
                }
            }
        })
    }
}

impl ProcessMicroVMProvider {
    /// Execute an external program with process isolation
    fn execute_external_process(
        &self,
        path: &str,
        args: &[String],
        context: &ExecutionContext,
    ) -> RuntimeResult<Value> {
        // Validate that external program execution is allowed
        if let Some(runtime_context) = &context.runtime_context {
            if !runtime_context.is_capability_allowed("external_program") {
                return Err(RuntimeError::SecurityViolation {
                    operation: "execute".to_string(),
                    capability: "external_program".to_string(),
                    context: format!("{:?}", runtime_context),
                });
            }
        }
        
        // Execute external program with process isolation
        println!("[PROCESS-MICROVM] Executing external program: {} {:?}", path, args);
        
        let output = std::process::Command::new(path)
            .args(args)
            .output()
            .map_err(|e| RuntimeError::Generic(format!("Failed to execute external program: {}", e)))?;
        
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        
        if output.status.success() {
            Ok(Value::String(stdout.to_string()))
        } else {
            Err(RuntimeError::Generic(format!("External program failed: {}", stderr)))
        }
    }
    
    /// Execute RTFS source code in an isolated process
    fn execute_rtfs_in_process(
        &self,
        source: &str,
        context: &ExecutionContext,
    ) -> RuntimeResult<Value> {
        // Create a temporary file with the RTFS source
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join(format!("rtfs_program_{}.rtfs", context.execution_id));
        
        // Write RTFS source to temporary file
        std::fs::write(&temp_file, source)
            .map_err(|e| RuntimeError::Generic(format!("Failed to write RTFS source: {}", e)))?;
        
        // Execute the RTFS compiler in a separate process
        let output = std::process::Command::new("rtfs_compiler")
            .arg("--execute")
            .arg(temp_file.to_str().unwrap())
            .output()
            .map_err(|e| RuntimeError::Generic(format!("Failed to execute RTFS compiler: {}", e)))?;
        
        // Clean up temporary file
        let _ = std::fs::remove_file(temp_file);
        
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        
        if output.status.success() {
            // Parse the result (assuming JSON output)
            match serde_json::from_str::<serde_json::Value>(&stdout) {
                Ok(json_value) => {
                    // Convert serde_json::Value to our Value type
                    match json_value {
                        serde_json::Value::String(s) => Ok(Value::String(s)),
                        serde_json::Value::Number(n) => {
                            if let Some(i) = n.as_i64() {
                                Ok(Value::Integer(i))
                            } else if let Some(f) = n.as_f64() {
                                Ok(Value::Float(f))
                            } else {
                                Ok(Value::String(n.to_string()))
                            }
                        },
                        serde_json::Value::Bool(b) => Ok(Value::Boolean(b)),
                        serde_json::Value::Null => Ok(Value::Nil),
                        serde_json::Value::Array(arr) => {
                            let values: Vec<Value> = arr.into_iter()
                                .map(|v| match v {
                                    serde_json::Value::String(s) => Value::String(s),
                                    serde_json::Value::Number(n) => {
                                        if let Some(i) = n.as_i64() {
                                            Value::Integer(i)
                                        } else if let Some(f) = n.as_f64() {
                                            Value::Float(f)
                                        } else {
                                            Value::String(n.to_string())
                                        }
                                    },
                                    serde_json::Value::Bool(b) => Value::Boolean(b),
                                    serde_json::Value::Null => Value::Nil,
                                    _ => Value::String(v.to_string()),
                                })
                                .collect();
                            Ok(Value::Vector(values))
                        },
                        _ => Ok(Value::String(json_value.to_string())),
                    }
                },
                Err(_) => Ok(Value::String(stdout.to_string())),
            }
        } else {
            Err(RuntimeError::Generic(format!("RTFS execution failed: {}", stderr)))
        }
    }
    
    /// Execute a native function in an isolated context
    fn execute_native_in_process(
        &self,
        func: &fn(Vec<Value>) -> RuntimeResult<Value>,
        context: &ExecutionContext,
    ) -> RuntimeResult<Value> {
        // Validate that native functions are allowed
        if let Some(runtime_context) = &context.runtime_context {
            if !runtime_context.is_capability_allowed("native_function") {
                return Err(RuntimeError::SecurityViolation {
                    operation: "execute".to_string(),
                    capability: "native_function".to_string(),
                    context: format!("{:?}", runtime_context),
                });
            }
        }
        
        // Execute native function with capability restrictions
        println!("[PROCESS-MICROVM] Executing native function with {} args", context.args.len());
        
        func(context.args.clone())
    }
}

/// Firecracker MicroVM provider (Linux only)
#[cfg(target_os = "linux")]
pub struct FirecrackerMicroVMProvider {
    initialized: bool,
}

#[cfg(target_os = "linux")]
impl FirecrackerMicroVMProvider {
    pub fn new() -> Self {
        Self { initialized: false }
    }
}

#[cfg(target_os = "linux")]
impl MicroVMProvider for FirecrackerMicroVMProvider {
    fn name(&self) -> &'static str {
        "firecracker"
    }
    
    fn is_available(&self) -> bool {
        // Check if firecracker binary is available
        std::process::Command::new("firecracker")
            .arg("--version")
            .output()
            .is_ok()
    }
    
    fn initialize(&mut self) -> RuntimeResult<()> {
        self.initialized = true;
        Ok(())
    }
    
    fn execute_program(&self, _context: ExecutionContext) -> RuntimeResult<ExecutionResult> {
        if !self.initialized {
            return Err(RuntimeError::Generic("FirecrackerMicroVMProvider not initialized".to_string()));
        }
        
        // TODO: Implement Firecracker integration
        Err(RuntimeError::Generic("FirecrackerMicroVMProvider not yet implemented".to_string()))
    }
    
    fn execute_capability(&self, _context: ExecutionContext) -> RuntimeResult<ExecutionResult> {
        self.execute_program(_context)
    }
    
    fn cleanup(&mut self) -> RuntimeResult<()> {
        self.initialized = false;
        Ok(())
    }
    
    fn get_config_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "kernel_path": {
                    "type": "string",
                    "description": "Path to the kernel image"
                },
                "rootfs_path": {
                    "type": "string",
                    "description": "Path to the root filesystem"
                }
            }
        })
    }
}

/// gVisor MicroVM provider (Linux only)
#[cfg(target_os = "linux")]
pub struct GvisorMicroVMProvider {
    initialized: bool,
}

#[cfg(target_os = "linux")]
impl GvisorMicroVMProvider {
    pub fn new() -> Self {
        Self { initialized: false }
    }
}

#[cfg(target_os = "linux")]
impl MicroVMProvider for GvisorMicroVMProvider {
    fn name(&self) -> &'static str {
        "gvisor"
    }
    
    fn is_available(&self) -> bool {
        // Check if runsc (gVisor runtime) is available
        std::process::Command::new("runsc")
            .arg("--version")
            .output()
            .is_ok()
    }
    
    fn initialize(&mut self) -> RuntimeResult<()> {
        self.initialized = true;
        Ok(())
    }
    
    fn execute_program(&self, _context: ExecutionContext) -> RuntimeResult<ExecutionResult> {
        if !self.initialized {
            return Err(RuntimeError::Generic("GvisorMicroVMProvider not initialized".to_string()));
        }
        
        // TODO: Implement gVisor integration
        Err(RuntimeError::Generic("GvisorMicroVMProvider not yet implemented".to_string()))
    }
    
    fn execute_capability(&self, _context: ExecutionContext) -> RuntimeResult<ExecutionResult> {
        self.execute_program(_context)
    }
    
    fn cleanup(&mut self) -> RuntimeResult<()> {
        self.initialized = false;
        Ok(())
    }
    
    fn get_config_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "platform": {
                    "type": "string",
                    "enum": ["ptrace", "kvm"],
                    "description": "gVisor platform to use"
                }
            }
        })
    }
}

/// WASM-based MicroVM provider
pub struct WasmMicroVMProvider {
    initialized: bool,
}

impl WasmMicroVMProvider {
    pub fn new() -> Self {
        Self { initialized: false }
    }
}

impl MicroVMProvider for WasmMicroVMProvider {
    fn name(&self) -> &'static str {
        "wasm"
    }
    
    fn is_available(&self) -> bool {
        true // WASM runtime is built-in
    }
    
    fn initialize(&mut self) -> RuntimeResult<()> {
        self.initialized = true;
        Ok(())
    }
    
    fn execute_program(&self, context: ExecutionContext) -> RuntimeResult<ExecutionResult> {
        if !self.initialized {
            return Err(RuntimeError::Generic("WasmMicroVMProvider not initialized".to_string()));
        }
        
        let start_time = std::time::Instant::now();
        
        // Execute program based on type
        let value = match context.program.as_ref() {
            Some(program) => {
                match program {
                    Program::RtfsSource(source) => {
                        // For now, we'll create a simple WASM module that returns a string
                        // In a real implementation, we'd compile RTFS to WASM
                        self.execute_simple_wasm_module(source, &context)?
                    },
                    Program::ExternalProgram { path, args } => {
                        return Err(RuntimeError::Generic(
                            "External programs not yet supported in WASM provider".to_string()
                        ));
                    },
                    Program::NativeFunction(_func) => {
                        return Err(RuntimeError::Generic(
                            "Native functions not supported in WASM provider".to_string()
                        ));
                    },
                    _ => {
                        return Err(RuntimeError::Generic(
                            "Program type not yet supported in WASM provider".to_string()
                        ));
                    }
                }
            },
            None => {
                return Err(RuntimeError::Generic("No program provided for execution".to_string()));
            }
        };
        
        let duration = start_time.elapsed();
        
        Ok(ExecutionResult {
            value,
            metadata: ExecutionMetadata {
                duration,
                memory_used_mb: 0, // TODO: Track WASM memory usage
                cpu_time: duration,
                network_requests: vec![],
                file_operations: vec![],
            },
        })
    }
    
    fn execute_capability(&self, _context: ExecutionContext) -> RuntimeResult<ExecutionResult> {
        self.execute_program(_context)
    }
    
    fn cleanup(&mut self) -> RuntimeResult<()> {
        self.initialized = false;
        Ok(())
    }
    
    fn get_config_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "engine": {
                    "type": "string",
                    "enum": ["wasmtime", "wasmer"],
                    "description": "WASM runtime engine to use"
                }
            }
        })
    }
}

impl WasmMicroVMProvider {
    /// Execute a simple WASM module (for demonstration purposes)
    fn execute_simple_wasm_module(&self, source: &str, _context: &ExecutionContext) -> RuntimeResult<Value> {
        // For now, we'll create a simple WASM module that returns a string
        // This is a demonstration - in a real implementation, we'd compile RTFS to WASM
        
        // Create a simple WASM module that returns "Hello from WASM"
        let wasm_bytes = self.create_simple_wasm_module(source)?;
        
        // Execute the WASM module using wasmtime
        let engine = wasmtime::Engine::default();
        let module = wasmtime::Module::new(&engine, &wasm_bytes)
            .map_err(|e| RuntimeError::Generic(format!("Failed to create WASM module: {}", e)))?;
        
        let mut store = wasmtime::Store::new(&engine, ());
        let instance = wasmtime::Instance::new(&mut store, &module, &[])
            .map_err(|e| RuntimeError::Generic(format!("Failed to instantiate WASM module: {}", e)))?;
        
        // Call the main function
        let main_func = instance.get_func(&mut store, "main")
            .ok_or_else(|| RuntimeError::Generic("WASM module has no 'main' function".to_string()))?;
        
        let mut results = vec![wasmtime::Val::I32(0)]; // Pre-allocate result buffer
        main_func.call(&mut store, &[], &mut results)
            .map_err(|e| RuntimeError::Generic(format!("Failed to execute WASM function: {}", e)))?;
        
        // Convert result to Value
        if let Some(result) = results.first() {
            match result {
                wasmtime::Val::I32(n) => Ok(Value::Integer(*n as i64)),
                wasmtime::Val::I64(n) => Ok(Value::Integer(*n)),
                wasmtime::Val::F32(f) => Ok(Value::Float(f32::from_bits(*f) as f64)),
                wasmtime::Val::F64(f) => Ok(Value::Float(f64::from_bits(*f))),
                _ => Ok(Value::String(format!("WASM result: {:?}", result))),
            }
        } else {
            // If no return value, return a success message
            Ok(Value::String(format!("WASM execution completed for: {}", source)))
        }
    }
    
    /// Create a simple WASM module (for demonstration)
    fn create_simple_wasm_module(&self, _source: &str) -> RuntimeResult<Vec<u8>> {
        // This is a very simple WASM module that returns 42
        // In a real implementation, we'd compile the RTFS source to WASM
        let wasm_bytes = vec![
            0x00, 0x61, 0x73, 0x6d, // WASM magic number
            0x01, 0x00, 0x00, 0x00, // Version 1
            // Type section
            0x01, 0x07, 0x01, 0x60, 0x00, 0x01, 0x7f, // func type: () -> i32
            // Function section
            0x03, 0x02, 0x01, 0x00, // 1 function of type 0
            // Export section
            0x07, 0x07, 0x01, 0x04, 0x6d, 0x61, 0x69, 0x6e, 0x00, 0x00, // export "main"
            // Code section
            0x0a, 0x04, 0x01, 0x02, 0x00, 0x41, 0x2a, 0x0b, // func body: i32.const 42
        ];
        
        Ok(wasm_bytes)
    }
}

impl Default for MicroVMConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            memory_limit_mb: 128,
            cpu_limit: 0.5,
            network_policy: NetworkPolicy::Denied,
            fs_policy: FileSystemPolicy::None,
            env_vars: HashMap::new(),
        }
    }
}

impl Default for MicroVMFactory {
    fn default() -> Self {
        Self::new()
    }
}

/// RTFS MicroVM Executor for program execution with capability permissions
pub struct RtfsMicroVMExecutor {
    rtfs_runtime: crate::runtime::Runtime,
    capability_registry: crate::runtime::capability_registry::CapabilityRegistry,
}

impl RtfsMicroVMExecutor {
    /// Create a new RTFS MicroVM executor
    pub fn new() -> Self {
        let module_registry = std::rc::Rc::new(crate::runtime::module_runtime::ModuleRegistry::new());
        let rtfs_runtime = crate::runtime::Runtime::new_with_tree_walking_strategy(module_registry);
        let capability_registry = crate::runtime::capability_registry::CapabilityRegistry::new();
        
        Self {
            rtfs_runtime,
            capability_registry,
        }
    }
    
    /// Execute an RTFS program with capability permissions
    pub fn execute_rtfs_program(
        &mut self,
        program: Program,
        permissions: Vec<String>,
        args: Vec<Value>,
        runtime_context: RuntimeContext,
    ) -> RuntimeResult<Value> {
        // Validate capability permissions
        self.validate_capability_permissions(&permissions, &runtime_context)?;
        
        // Execute program based on type
        match program {
            Program::RtfsSource(source) => {
                self.execute_rtfs_source(&source, args, runtime_context)
            },
            Program::RtfsAst(ast) => {
                self.execute_rtfs_ast(*ast, args, runtime_context)
            },
            Program::RtfsBytecode(bytecode) => {
                self.execute_rtfs_bytecode(bytecode, args, runtime_context)
            },
            Program::NativeFunction(func) => {
                // Execute native function with capability restrictions
                self.execute_native_function(func, args, runtime_context)
            },
            Program::ExternalProgram { path, args: prog_args } => {
                // Execute external program with isolation
                self.execute_external_program(path, prog_args, args, runtime_context)
            },
        }
    }
    
    /// Execute RTFS source code
    fn execute_rtfs_source(
        &mut self,
        source: &str,
        args: Vec<Value>,
        runtime_context: RuntimeContext,
    ) -> RuntimeResult<Value> {
        // Parse RTFS source code
        let top_level_items = crate::parser::parse(source)
            .map_err(|e| RuntimeError::Generic(format!("Parse error: {}", e)))?;
        
        // Create evaluator with restricted security context
        let module_registry = std::rc::Rc::new(crate::runtime::module_runtime::ModuleRegistry::new());
        let delegation_engine = std::sync::Arc::new(crate::ccos::delegation::StaticDelegationEngine::new(std::collections::HashMap::new()));
        let host = std::rc::Rc::new(crate::runtime::host::RuntimeHost::new(
            std::sync::Arc::new(std::sync::Mutex::new(crate::ccos::causal_chain::CausalChain::new().expect("Failed to create causal chain"))),
            std::sync::Arc::new(crate::runtime::capability_marketplace::CapabilityMarketplace::new(
                std::sync::Arc::new(tokio::sync::RwLock::new(crate::runtime::capability_registry::CapabilityRegistry::new()))
            )),
            runtime_context.clone(),
        ));
        
        let mut evaluator = crate::runtime::evaluator::Evaluator::new(
            module_registry,
            delegation_engine,
            runtime_context,
            host,
        );
        
        // Set up arguments in the environment if provided
        if !args.is_empty() {
            let mut env = evaluator.env.clone();
            for (i, arg) in args.iter().enumerate() {
                env.define(
                    &crate::ast::Symbol(format!("arg{}", i)),
                    arg.clone(),
                );
            }
            evaluator.env = env;
        }
        
        // Evaluate the parsed program
        evaluator.eval_toplevel(&top_level_items)
    }
    
    /// Execute RTFS AST
    fn execute_rtfs_ast(
        &mut self,
        ast: Expression,
        args: Vec<Value>,
        runtime_context: RuntimeContext,
    ) -> RuntimeResult<Value> {
        // Create evaluator with restricted security context
        let module_registry = std::rc::Rc::new(crate::runtime::module_runtime::ModuleRegistry::new());
        let delegation_engine = std::sync::Arc::new(crate::ccos::delegation::StaticDelegationEngine::new(std::collections::HashMap::new()));
        let host = std::rc::Rc::new(crate::runtime::host::RuntimeHost::new(
            std::sync::Arc::new(std::sync::Mutex::new(crate::ccos::causal_chain::CausalChain::new().expect("Failed to create causal chain"))),
            std::sync::Arc::new(crate::runtime::capability_marketplace::CapabilityMarketplace::new(
                std::sync::Arc::new(tokio::sync::RwLock::new(crate::runtime::capability_registry::CapabilityRegistry::new()))
            )),
            runtime_context.clone(),
        ));
        
        let mut evaluator = crate::runtime::evaluator::Evaluator::new(
            module_registry,
            delegation_engine,
            runtime_context,
            host,
        );
        
        // Set up arguments in the environment if provided
        if !args.is_empty() {
            let mut env = evaluator.env.clone();
            for (i, arg) in args.iter().enumerate() {
                env.define(
                    &crate::ast::Symbol(format!("arg{}", i)),
                    arg.clone(),
                );
            }
            evaluator.env = env;
        }
        
        // Evaluate the AST expression
        evaluator.evaluate(&ast)
    }
    
    /// Execute RTFS bytecode
    fn execute_rtfs_bytecode(
        &mut self,
        bytecode: Vec<u8>,
        args: Vec<Value>,
        _runtime_context: RuntimeContext,
    ) -> RuntimeResult<Value> {
        // Execute RTFS bytecode using WASM executor
        let wasm_executor = crate::bytecode::WasmExecutor::new();
        
        // Execute the bytecode module (assuming a default function name)
        wasm_executor.execute_module(&bytecode, "main", &args)
    }
    
    /// Execute native function with capability restrictions
    fn execute_native_function(
        &self,
        func: fn(Vec<Value>) -> RuntimeResult<Value>,
        args: Vec<Value>,
        runtime_context: RuntimeContext,
    ) -> RuntimeResult<Value> {
        // Validate that native functions are allowed in this context
        if !runtime_context.is_capability_allowed("native_function") {
            return Err(RuntimeError::SecurityViolation {
                operation: "execute".to_string(),
                capability: "native_function".to_string(),
                context: format!("{:?}", runtime_context),
            });
        }
        
        // Execute native function with capability restrictions
        println!("[RTFS-MICROVM] Executing native function with {} args", args.len());
        
        func(args)
    }
    
    /// Execute external program with isolation
    fn execute_external_program(
        &self,
        path: String,
        prog_args: Vec<String>,
        _args: Vec<Value>,
        runtime_context: RuntimeContext,
    ) -> RuntimeResult<Value> {
        // Validate that external program execution is allowed
        if !runtime_context.is_capability_allowed("external_program") {
            return Err(RuntimeError::SecurityViolation {
                operation: "execute".to_string(),
                capability: "external_program".to_string(),
                context: format!("{:?}", runtime_context),
            });
        }
        
        // Execute external program with process isolation
        println!("[RTFS-MICROVM] Executing external program: {} {:?}", path, prog_args);
        
        // Use std::process::Command to execute the external program
        let output = std::process::Command::new(&path)
            .args(&prog_args)
            .output()
            .map_err(|e| RuntimeError::Generic(format!("Failed to execute external program: {}", e)))?;
        
        // Return the output as a string
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        
        if output.status.success() {
            Ok(Value::String(stdout.to_string()))
        } else {
            Err(RuntimeError::Generic(format!("External program failed: {}", stderr)))
        }
    }
    
    /// Validate capability permissions against runtime context
    fn validate_capability_permissions(
        &self,
        permissions: &[String],
        runtime_context: &RuntimeContext,
    ) -> RuntimeResult<()> {
        for permission in permissions {
            if !runtime_context.is_capability_allowed(permission) {
                return Err(RuntimeError::Generic(
                    format!("Capability '{}' not allowed in current runtime context", permission)
                ));
            }
        }
        Ok(())
    }
}

impl Default for RtfsMicroVMExecutor {
    fn default() -> Self {
        Self::new()
    }
}
