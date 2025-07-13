# CCOS Security Framework

## Overview

This document outlines the comprehensive security framework for the CCOS Capability Architecture, ensuring safe execution of dangerous operations while maintaining system integrity.

## Analysis and Motivation

### Security Principles

1. **Principle of Least Privilege**: Grant only the minimum permissions necessary
2. **Defense in Depth**: Multiple security layers for comprehensive protection
3. **Explicit Security**: Security decisions must be explicit, not implicit
4. **Isolation**: Dangerous operations execute in isolated environments
5. **Auditability**: All security-relevant actions must be logged and auditable

### Threat Model

The security framework addresses these potential threats:

- **Code Injection**: Malicious code execution through capability calls
- **Resource Exhaustion**: Capabilities consuming excessive system resources
- **Data Exfiltration**: Unauthorized access to sensitive data
- **Privilege Escalation**: Capabilities gaining more permissions than granted
- **Side-Channel Attacks**: Information leakage through timing or resource usage

## Implementation Strategy

### 1. Security Context Framework

```rust
// src/runtime/security/context.rs
use std::collections::HashMap;
use std::time::{Duration, SystemTime};
use serde::{Deserialize, Serialize};

/// Security context for capability execution
#[derive(Debug, Clone)]
pub struct SecurityContext {
    /// Security level for this context
    pub level: SecurityLevel,
    /// Granted permissions
    pub permissions: PermissionSet,
    /// Resource limits
    pub resource_limits: ResourceLimits,
    /// Environment restrictions
    pub environment_restrictions: EnvironmentRestrictions,
    /// Audit configuration
    pub audit_config: AuditConfig,
    /// Context creation time
    pub created_at: SystemTime,
    /// Context expiration time
    pub expires_at: Option<SystemTime>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecurityLevel {
    /// Pure RTFS functions only
    Pure,
    /// Limited capabilities with explicit permissions
    Controlled,
    /// Full system access (for system administration)
    Full,
    /// Sandboxed execution (for untrusted code)
    Sandboxed,
}

/// Set of permissions granted to a security context
#[derive(Debug, Clone)]
pub struct PermissionSet {
    /// File system permissions
    pub filesystem: FileSystemPermissions,
    /// Network permissions
    pub network: NetworkPermissions,
    /// System permissions
    pub system: SystemPermissions,
    /// Environment permissions
    pub environment: EnvironmentPermissions,
    /// Inter-process communication permissions
    pub ipc: IPCPermissions,
}

#[derive(Debug, Clone)]
pub struct FileSystemPermissions {
    /// Allowed read paths (with wildcards)
    pub read_paths: Vec<PathPattern>,
    /// Allowed write paths (with wildcards)
    pub write_paths: Vec<PathPattern>,
    /// Allowed execute paths
    pub execute_paths: Vec<PathPattern>,
    /// Blocked paths (takes precedence)
    pub blocked_paths: Vec<PathPattern>,
}

#[derive(Debug, Clone)]
pub struct NetworkPermissions {
    /// Allowed outbound connections
    pub outbound: Vec<NetworkPattern>,
    /// Allowed inbound connections
    pub inbound: Vec<NetworkPattern>,
    /// Blocked networks (takes precedence)
    pub blocked: Vec<NetworkPattern>,
    /// Maximum concurrent connections
    pub max_connections: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct SystemPermissions {
    /// Allowed environment variables to read
    pub env_read: Vec<String>,
    /// Allowed environment variables to write
    pub env_write: Vec<String>,
    /// Allowed system commands
    pub commands: Vec<CommandPattern>,
    /// Process creation permissions
    pub process_creation: bool,
}

/// Resource limits for capability execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// Maximum memory usage in bytes
    pub max_memory: Option<u64>,
    /// Maximum CPU time
    pub max_cpu_time: Option<Duration>,
    /// Maximum wall clock time
    pub max_wall_time: Option<Duration>,
    /// Maximum disk space usage
    pub max_disk_space: Option<u64>,
    /// Maximum number of file descriptors
    pub max_file_descriptors: Option<u32>,
    /// Maximum number of threads
    pub max_threads: Option<u32>,
    /// Maximum network bandwidth (bytes per second)
    pub max_network_bandwidth: Option<u64>,
}

impl SecurityContext {
    /// Create a pure security context (no dangerous operations)
    pub fn pure() -> Self {
        Self {
            level: SecurityLevel::Pure,
            permissions: PermissionSet::none(),
            resource_limits: ResourceLimits::minimal(),
            environment_restrictions: EnvironmentRestrictions::strict(),
            audit_config: AuditConfig::minimal(),
            created_at: SystemTime::now(),
            expires_at: None,
        }
    }
    
    /// Create a controlled security context with specific permissions
    pub fn controlled(permissions: PermissionSet, limits: ResourceLimits) -> Self {
        Self {
            level: SecurityLevel::Controlled,
            permissions,
            resource_limits: limits,
            environment_restrictions: EnvironmentRestrictions::default(),
            audit_config: AuditConfig::standard(),
            created_at: SystemTime::now(),
            expires_at: Some(SystemTime::now() + Duration::from_hours(1)),
        }
    }
    
    /// Create a sandboxed security context for untrusted code
    pub fn sandboxed() -> Self {
        Self {
            level: SecurityLevel::Sandboxed,
            permissions: PermissionSet::sandboxed(),
            resource_limits: ResourceLimits::sandboxed(),
            environment_restrictions: EnvironmentRestrictions::sandboxed(),
            audit_config: AuditConfig::comprehensive(),
            created_at: SystemTime::now(),
            expires_at: Some(SystemTime::now() + Duration::from_minutes(10)),
        }
    }
    
    /// Check if a specific permission is granted
    pub fn check_permission(&self, permission: &Permission) -> Result<(), SecurityError> {
        if self.is_expired() {
            return Err(SecurityError::ContextExpired);
        }
        
        match permission {
            Permission::FileRead(path) => {
                self.permissions.filesystem.check_read_access(path)
            }
            Permission::FileWrite(path) => {
                self.permissions.filesystem.check_write_access(path)
            }
            Permission::NetworkAccess(target) => {
                self.permissions.network.check_outbound_access(target)
            }
            Permission::EnvironmentRead(var) => {
                self.permissions.environment.check_read_access(var)
            }
            Permission::SystemCommand(cmd) => {
                self.permissions.system.check_command_access(cmd)
            }
        }
    }
    
    /// Check if the context has expired
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            SystemTime::now() > expires_at
        } else {
            false
        }
    }
    
    /// Get remaining lifetime of the context
    pub fn remaining_lifetime(&self) -> Option<Duration> {
        self.expires_at.and_then(|expires_at| {
            expires_at.duration_since(SystemTime::now()).ok()
        })
    }
}
```

### 2. Permission System

```rust
// src/runtime/security/permissions.rs

/// Represents a specific permission request
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Permission {
    FileRead(PathBuf),
    FileWrite(PathBuf),
    FileExecute(PathBuf),
    NetworkAccess(String),
    EnvironmentRead(String),
    EnvironmentWrite(String),
    SystemCommand(String),
    ProcessCreate,
    IPCConnect(String),
}

/// Pattern for matching file system paths
#[derive(Debug, Clone)]
pub struct PathPattern {
    pattern: String,
    is_wildcard: bool,
    is_recursive: bool,
}

impl PathPattern {
    pub fn new(pattern: &str) -> Self {
        let is_wildcard = pattern.contains('*') || pattern.contains('?');
        let is_recursive = pattern.contains("**");
        
        Self {
            pattern: pattern.to_string(),
            is_wildcard,
            is_recursive,
        }
    }
    
    pub fn matches(&self, path: &Path) -> bool {
        if !self.is_wildcard {
            return path.starts_with(&self.pattern);
        }
        
        // Use glob matching for wildcard patterns
        match glob::Pattern::new(&self.pattern) {
            Ok(pattern) => pattern.matches_path(path),
            Err(_) => false,
        }
    }
}

/// Pattern for matching network addresses
#[derive(Debug, Clone)]
pub struct NetworkPattern {
    pattern: String,
    is_wildcard: bool,
}

impl NetworkPattern {
    pub fn new(pattern: &str) -> Self {
        let is_wildcard = pattern.contains('*');
        
        Self {
            pattern: pattern.to_string(),
            is_wildcard,
        }
    }
    
    pub fn matches(&self, target: &str) -> bool {
        if !self.is_wildcard {
            return target == self.pattern;
        }
        
        // Simple wildcard matching for domains
        if self.pattern.starts_with("*.") {
            let domain = self.pattern.strip_prefix("*.").unwrap();
            return target == domain || target.ends_with(&format!(".{}", domain));
        }
        
        false
    }
}

impl FileSystemPermissions {
    pub fn check_read_access(&self, path: &Path) -> Result<(), SecurityError> {
        // Check blocked paths first
        for blocked in &self.blocked_paths {
            if blocked.matches(path) {
                return Err(SecurityError::PermissionDenied(format!(
                    "Read access blocked for path: {:?}", path
                )));
            }
        }
        
        // Check allowed read paths
        for allowed in &self.read_paths {
            if allowed.matches(path) {
                return Ok(());
            }
        }
        
        Err(SecurityError::PermissionDenied(format!(
            "Read access not granted for path: {:?}", path
        )))
    }
    
    pub fn check_write_access(&self, path: &Path) -> Result<(), SecurityError> {
        // Check blocked paths first
        for blocked in &self.blocked_paths {
            if blocked.matches(path) {
                return Err(SecurityError::PermissionDenied(format!(
                    "Write access blocked for path: {:?}", path
                )));
            }
        }
        
        // Check allowed write paths
        for allowed in &self.write_paths {
            if allowed.matches(path) {
                return Ok(());
            }
        }
        
        Err(SecurityError::PermissionDenied(format!(
            "Write access not granted for path: {:?}", path
        )))
    }
}

impl NetworkPermissions {
    pub fn check_outbound_access(&self, target: &str) -> Result<(), SecurityError> {
        // Check blocked networks first
        for blocked in &self.blocked {
            if blocked.matches(target) {
                return Err(SecurityError::PermissionDenied(format!(
                    "Network access blocked for target: {}", target
                )));
            }
        }
        
        // Check allowed outbound patterns
        for allowed in &self.outbound {
            if allowed.matches(target) {
                return Ok(());
            }
        }
        
        Err(SecurityError::PermissionDenied(format!(
            "Network access not granted for target: {}", target
        )))
    }
}
```

### 3. Security Validator

```rust
// src/runtime/security/validator.rs
use std::sync::Arc;
use tokio::sync::Mutex;

/// Validates security requirements for capability execution
pub struct SecurityValidator {
    /// Policy engine for complex security decisions
    policy_engine: Arc<PolicyEngine>,
    /// Resource monitor for tracking usage
    resource_monitor: Arc<Mutex<ResourceMonitor>>,
    /// Audit logger for security events
    audit_logger: Arc<AuditLogger>,
}

impl SecurityValidator {
    pub fn new() -> Self {
        Self {
            policy_engine: Arc::new(PolicyEngine::new()),
            resource_monitor: Arc::new(Mutex::new(ResourceMonitor::new())),
            audit_logger: Arc::new(AuditLogger::new()),
        }
    }
    
    /// Validate that a capability provider meets security requirements
    pub fn validate_provider(&self, provider: &dyn CapabilityProvider) -> Result<(), String> {
        let metadata = provider.metadata();
        
        // Check provider signature/authenticity
        self.validate_provider_signature(&metadata)?;
        
        // Validate all capabilities offered by the provider
        for capability in provider.list_capabilities() {
            self.validate_capability_descriptor(&capability)?;
        }
        
        // Audit provider registration
        self.audit_logger.log_provider_registration(&metadata);
        
        Ok(())
    }
    
    /// Validate capability execution request
    pub async fn validate_execution(
        &self,
        capability: &CapabilityDescriptor,
        context: &ExecutionContext,
    ) -> Result<(), SecurityError> {
        // Check if context is still valid
        if context.security_context.is_expired() {
            return Err(SecurityError::ContextExpired);
        }
        
        // Validate permissions
        for permission in &capability.security_requirements.permissions {
            context.security_context.check_permission(permission)?;
        }
        
        // Check resource limits
        self.validate_resource_limits(capability, context).await?;
        
        // Apply security policies
        self.policy_engine.evaluate_execution_policy(capability, context)?;
        
        // Audit execution request
        self.audit_logger.log_execution_request(capability, context);
        
        Ok(())
    }
    
    async fn validate_resource_limits(
        &self,
        capability: &CapabilityDescriptor,
        context: &ExecutionContext,
    ) -> Result<(), SecurityError> {
        let mut monitor = self.resource_monitor.lock().await;
        
        let required = &capability.security_requirements.resource_limits;
        let available = &context.security_context.resource_limits;
        
        // Check memory limits
        if let (Some(required_mem), Some(available_mem)) = (required.max_memory, available.max_memory) {
            if required_mem > available_mem {
                return Err(SecurityError::ResourceLimitExceeded(format!(
                    "Memory requirement {} exceeds limit {}", required_mem, available_mem
                )));
            }
            
            // Check current memory usage
            let current_usage = monitor.get_memory_usage();
            if current_usage + required_mem > available_mem {
                return Err(SecurityError::ResourceLimitExceeded(format!(
                    "Memory usage would exceed limit: {} + {} > {}", 
                    current_usage, required_mem, available_mem
                )));
            }
        }
        
        // Check CPU time limits
        if let (Some(required_cpu), Some(available_cpu)) = (required.max_cpu_time, available.max_cpu_time) {
            if required_cpu > available_cpu {
                return Err(SecurityError::ResourceLimitExceeded(format!(
                    "CPU time requirement {:?} exceeds limit {:?}", required_cpu, available_cpu
                )));
            }
        }
        
        // Reserve resources for this execution
        monitor.reserve_resources(required)?;
        
        Ok(())
    }
    
    fn validate_provider_signature(&self, metadata: &ProviderMetadata) -> Result<(), String> {
        // In a production system, this would verify cryptographic signatures
        // For now, we'll do basic validation
        
        if metadata.name.is_empty() {
            return Err("Provider name cannot be empty".to_string());
        }
        
        if metadata.version.is_empty() {
            return Err("Provider version cannot be empty".to_string());
        }
        
        // Check for known malicious providers (would be from a database)
        if self.is_known_malicious_provider(metadata) {
            return Err(format!("Provider {} is on the malicious provider list", metadata.name));
        }
        
        Ok(())
    }
    
    fn validate_capability_descriptor(&self, capability: &CapabilityDescriptor) -> Result<(), String> {
        // Validate capability ID format
        if !capability.id.contains('.') {
            return Err(format!("Capability ID must be namespaced: {}", capability.id));
        }
        
        // Check for dangerous patterns
        if capability.id.contains("..") || capability.id.contains("/") {
            return Err(format!("Invalid capability ID format: {}", capability.id));
        }
        
        // Validate security requirements are reasonable
        let reqs = &capability.security_requirements;
        
        // Check if memory limits are reasonable
        if let Some(memory) = reqs.resource_limits.max_memory {
            if memory > 1_073_741_824 { // 1GB
                return Err(format!(
                    "Capability {} requests excessive memory: {} bytes", 
                    capability.id, memory
                ));
            }
        }
        
        // Check if CPU time limits are reasonable
        if let Some(cpu_time) = reqs.resource_limits.max_cpu_time {
            if cpu_time > Duration::from_minutes(5) {
                return Err(format!(
                    "Capability {} requests excessive CPU time: {:?}", 
                    capability.id, cpu_time
                ));
            }
        }
        
        Ok(())
    }
    
    fn is_known_malicious_provider(&self, metadata: &ProviderMetadata) -> bool {
        // In production, this would check against a database of known malicious providers
        // For now, we'll check against a hardcoded list
        const MALICIOUS_PROVIDERS: &[&str] = &[
            "malicious-provider",
            "evil-plugin",
            "backdoor-capability",
        ];
        
        MALICIOUS_PROVIDERS.contains(&metadata.name.as_str())
    }
}

/// Errors that can occur during security validation
#[derive(Debug, thiserror::Error)]
pub enum SecurityError {
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    
    #[error("Security context has expired")]
    ContextExpired,
    
    #[error("Resource limit exceeded: {0}")]
    ResourceLimitExceeded(String),
    
    #[error("Policy violation: {0}")]
    PolicyViolation(String),
    
    #[error("Invalid security configuration: {0}")]
    InvalidConfiguration(String),
    
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),
}
```

### 4. Execution Isolation

```rust
// src/runtime/security/isolation.rs
use std::process::Command;
use tokio::process::Command as AsyncCommand;

/// Provides execution isolation for dangerous capabilities
pub enum ExecutionIsolation {
    /// No isolation (for safe operations)
    None,
    /// Process isolation with restricted permissions
    Process(ProcessIsolation),
    /// MicroVM isolation for maximum security
    MicroVM(MicroVMIsolation),
    /// WASM sandbox isolation
    WASM(WASMIsolation),
    /// Container isolation
    Container(ContainerIsolation),
}

#[derive(Debug)]
pub struct ProcessIsolation {
    /// Working directory for the isolated process
    pub working_directory: PathBuf,
    /// Environment variables to pass
    pub environment: HashMap<String, String>,
    /// User ID to run as (Unix only)
    pub uid: Option<u32>,
    /// Group ID to run as (Unix only)
    pub gid: Option<u32>,
    /// Network namespace isolation
    pub network_isolation: bool,
    /// File system isolation using chroot/jail
    pub filesystem_isolation: Option<PathBuf>,
}

impl ProcessIsolation {
    pub async fn execute_capability(
        &self,
        capability_id: &str,
        inputs: &Value,
        timeout: Duration,
    ) -> Result<Value, ExecutionError> {
        let mut cmd = AsyncCommand::new("rtfs-capability-runner");
        
        // Set working directory
        cmd.current_dir(&self.working_directory);
        
        // Set environment variables
        cmd.envs(&self.environment);
        
        // Apply user/group restrictions (Unix only)
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            
            if let Some(uid) = self.uid {
                cmd.uid(uid);
            }
            
            if let Some(gid) = self.gid {
                cmd.gid(gid);
            }
        }
        
        // Set up arguments
        cmd.arg("--capability-id").arg(capability_id);
        cmd.arg("--inputs").arg(serde_json::to_string(inputs)?);
        
        // Execute with timeout
        let output = tokio::time::timeout(timeout, cmd.output()).await
            .map_err(|_| ExecutionError::Timeout)?
            .map_err(ExecutionError::ProcessError)?;
        
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ExecutionError::CapabilityFailed(stderr.to_string()));
        }
        
        // Parse result
        let stdout = String::from_utf8_lossy(&output.stdout);
        let result: Value = serde_json::from_str(&stdout)
            .map_err(|e| ExecutionError::InvalidResult(e.to_string()))?;
        
        Ok(result)
    }
}

#[derive(Debug)]
pub struct MicroVMIsolation {
    /// Firecracker VMM configuration
    pub vm_config: FirecrackerConfig,
    /// Resource limits for the VM
    pub resource_limits: VMResourceLimits,
    /// Network configuration
    pub network_config: Option<VMNetworkConfig>,
}

#[derive(Debug)]
pub struct WASMIsolation {
    /// WASM runtime engine
    pub engine: wasmtime::Engine,
    /// Resource limits for WASM execution
    pub resource_limits: WASMResourceLimits,
    /// Host function allowlist
    pub allowed_host_functions: Vec<String>,
}

impl WASMIsolation {
    pub async fn execute_capability(
        &self,
        module_bytes: &[u8],
        capability_id: &str,
        inputs: &Value,
        timeout: Duration,
    ) -> Result<Value, ExecutionError> {
        let module = wasmtime::Module::new(&self.engine, module_bytes)
            .map_err(|e| ExecutionError::InvalidModule(e.to_string()))?;
        
        let mut store = wasmtime::Store::new(&self.engine, ());
        
        // Apply resource limits
        store.limiter(|_| &mut self.resource_limits);
        
        // Create instance with limited host functions
        let mut linker = wasmtime::Linker::new(&self.engine);
        self.setup_host_functions(&mut linker)?;
        
        let instance = linker.instantiate(&mut store, &module)
            .map_err(|e| ExecutionError::InstantiationFailed(e.to_string()))?;
        
        // Find and call the capability function
        let capability_func = instance
            .get_typed_func::<(i32, i32), i32>(&mut store, "execute_capability")
            .map_err(|e| ExecutionError::FunctionNotFound(e.to_string()))?;
        
        // Serialize inputs and write to WASM memory
        let inputs_json = serde_json::to_string(inputs)?;
        let inputs_ptr = self.write_string_to_memory(&mut store, &instance, &inputs_json)?;
        let capability_id_ptr = self.write_string_to_memory(&mut store, &instance, capability_id)?;
        
        // Execute with timeout
        let result_ptr = tokio::time::timeout(
            timeout,
            tokio::task::spawn_blocking(move || {
                capability_func.call(&mut store, (capability_id_ptr, inputs_ptr))
            })
        ).await
        .map_err(|_| ExecutionError::Timeout)?
        .map_err(|e| ExecutionError::ExecutionFailed(e.to_string()))?
        .map_err(|e| ExecutionError::WASMTrap(e.to_string()))?;
        
        // Read result from WASM memory
        let result_json = self.read_string_from_memory(&mut store, &instance, result_ptr)?;
        let result: Value = serde_json::from_str(&result_json)
            .map_err(|e| ExecutionError::InvalidResult(e.to_string()))?;
        
        Ok(result)
    }
    
    fn setup_host_functions(&self, linker: &mut wasmtime::Linker<()>) -> Result<(), ExecutionError> {
        // Only provide explicitly allowed host functions
        
        if self.allowed_host_functions.contains(&"log".to_string()) {
            linker.func_wrap("env", "log", |caller: wasmtime::Caller<'_, ()>, ptr: i32, len: i32| {
                // Implementation for safe logging
            }).map_err(|e| ExecutionError::HostFunctionSetupFailed(e.to_string()))?;
        }
        
        if self.allowed_host_functions.contains(&"get_time".to_string()) {
            linker.func_wrap("env", "get_time", || {
                SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
            }).map_err(|e| ExecutionError::HostFunctionSetupFailed(e.to_string()))?;
        }
        
        // Add other allowed host functions...
        
        Ok(())
    }
}
```

This comprehensive security framework provides:

1. **Multi-layered security** with contexts, permissions, validation, and isolation
2. **Fine-grained permissions** for file system, network, and system access
3. **Resource limits enforcement** preventing resource exhaustion attacks
4. **Multiple isolation mechanisms** (process, microVM, WASM, container)
5. **Comprehensive audit logging** for security monitoring
6. **Policy engine** for complex security decisions
7. **Provider validation** ensuring only trusted capabilities are loaded

The framework is designed to be both secure and usable, providing clear security boundaries while maintaining performance and functionality.
