//! MicroVM Abstraction Layer for RTFS/CCOS
//!
//! This module provides a pluggable architecture for secure execution environments
//! that can isolate dangerous operations like network access, file I/O, and system calls.

use crate::runtime::values::Value;
use crate::runtime::error::{RuntimeError, RuntimeResult};
use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use std::time::Duration;

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

/// Execution context for MicroVM operations
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    /// Unique identifier for this execution
    pub execution_id: String,
    /// Capability being executed
    pub capability_id: String,
    /// Arguments passed to the capability
    pub args: Vec<Value>,
    /// Configuration for this execution
    pub config: MicroVMConfig,
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

/// Trait for MicroVM implementations
pub trait MicroVMProvider: Send + Sync {
    /// Name of this MicroVM provider
    fn name(&self) -> &'static str;
    
    /// Check if this provider is available on the current system
    fn is_available(&self) -> bool;
    
    /// Initialize the MicroVM provider
    fn initialize(&mut self) -> RuntimeResult<()>;
    
    /// Execute a capability in the MicroVM
    fn execute(&self, context: ExecutionContext) -> RuntimeResult<ExecutionResult>;
    
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
        
        #[cfg(feature = "wasm")]
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
    
    fn execute(&self, context: ExecutionContext) -> RuntimeResult<ExecutionResult> {
        if !self.initialized {
            return Err(RuntimeError::Generic("MockMicroVMProvider not initialized".to_string()));
        }
        
        println!("[MOCK-MICROVM] Executing capability: {}", context.capability_id);
        println!("[MOCK-MICROVM] Args: {:?}", context.args);
        
        // Simulate execution based on capability type
        let value = match context.capability_id.as_str() {
            "ccos.network.http-fetch" => {
                // HTTP operations should be handled by the marketplace's execute_http_capability
                // For now, return a placeholder that indicates this should be routed through the marketplace
                Value::String("HTTP operations should be executed through the marketplace, not directly in MicroVM".to_string())
            },
            "ccos.io.open-file" => {
                Value::String("mock-file-handle".to_string())
            },
            "ccos.io.read-line" => {
                Value::String("mock file content line".to_string())
            },
            _ => Value::String("mock-result".to_string()),
        };
        
        let metadata = ExecutionMetadata {
            duration: Duration::from_millis(10),
            memory_used_mb: 1,
            cpu_time: Duration::from_micros(100),
            network_requests: if context.capability_id.contains("network") {
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
            file_operations: if context.capability_id.contains("io") {
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
    
    fn execute(&self, _context: ExecutionContext) -> RuntimeResult<ExecutionResult> {
        if !self.initialized {
            return Err(RuntimeError::Generic("ProcessMicroVMProvider not initialized".to_string()));
        }
        
        // TODO: Implement actual process-based execution
        // This would spawn a separate process with restricted permissions
        // and execute the capability there
        Err(RuntimeError::Generic("ProcessMicroVMProvider not yet implemented".to_string()))
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
                }
            }
        })
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
    
    fn execute(&self, _context: ExecutionContext) -> RuntimeResult<ExecutionResult> {
        if !self.initialized {
            return Err(RuntimeError::Generic("FirecrackerMicroVMProvider not initialized".to_string()));
        }
        
        // TODO: Implement Firecracker integration
        Err(RuntimeError::Generic("FirecrackerMicroVMProvider not yet implemented".to_string()))
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
    
    fn execute(&self, _context: ExecutionContext) -> RuntimeResult<ExecutionResult> {
        if !self.initialized {
            return Err(RuntimeError::Generic("GvisorMicroVMProvider not initialized".to_string()));
        }
        
        // TODO: Implement gVisor integration
        Err(RuntimeError::Generic("GvisorMicroVMProvider not yet implemented".to_string()))
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
#[cfg(feature = "wasm")]
pub struct WasmMicroVMProvider {
    initialized: bool,
}

#[cfg(feature = "wasm")]
impl WasmMicroVMProvider {
    pub fn new() -> Self {
        Self { initialized: false }
    }
}

#[cfg(feature = "wasm")]
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
    
    fn execute(&self, _context: ExecutionContext) -> RuntimeResult<ExecutionResult> {
        if !self.initialized {
            return Err(RuntimeError::Generic("WasmMicroVMProvider not initialized".to_string()));
        }
        
        // TODO: Implement WASM-based execution
        Err(RuntimeError::Generic("WasmMicroVMProvider not yet implemented".to_string()))
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
