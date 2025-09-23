# CCOS Specification 020: MicroVM Architecture

**Status:** Implemented
**Version:** 1.0
**Date:** 2025-01-21
**Related:**
- [SEP-000: System Architecture](./000-ccos-architecture.md)
- [SEP-005: Security and Context](./005-security-and-context.md)
- [SEP-015: Execution Contexts](./015-execution-contexts.md)

## 1. Abstract

The MicroVM Architecture provides secure, isolated execution environments for CCOS capabilities and programs. It implements a pluggable provider system with multiple isolation levels, from mock execution to full VM isolation, ensuring security and performance for different use cases.

## 2. Architecture Overview

### 2.1. Core Components

```
┌─────────────────────────────────────────────────────────────┐
│                    MicroVM System                          │
├─────────────────────────────────────────────────────────────┤
│  CapabilityRegistry ──┐                                     │
│                       │                                     │
│  MicroVMFactory ──────┼─── Provider Management              │
│                       │                                     │
│  SecurityAuthorizer ──┘                                     │
├─────────────────────────────────────────────────────────────┤
│                    Provider Layer                          │
├─────────────────────────────────────────────────────────────┤
│  Mock │ Process │ WASM │ Firecracker │ gVisor              │
│  (1x) │ (2-5x)  │(2-4x)│  (10-20x)  │ (3-8x)              │
└─────────────────────────────────────────────────────────────┘
```

### 2.2. Isolation Levels

| Provider | Isolation | Security | Performance | Use Case |
|----------|-----------|----------|-------------|----------|
| Mock | None | Minimal | 1x | Testing/Development |
| Process | Process | Low | 2-5x | Basic isolation |
| WASM | Sandbox | Medium | 2-4x | WebAssembly execution |
| Firecracker | VM | High | 10-20x | Production workloads |
| gVisor | Container | Medium | 3-8x | Container isolation |

## 3. Core Types

### 3.1. Program Representation

```rust
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
```

### 3.2. Execution Context

```rust
pub struct ExecutionContext {
    /// Unique identifier for this execution
    pub execution_id: String,
    /// Program to execute
    pub program: Option<Program>,
    /// Capability being executed
    pub capability_id: Option<String>,
    /// Capability permissions for program execution
    pub capability_permissions: Vec<String>,
    /// Arguments passed to the capability/program
    pub args: Vec<Value>,
    /// Configuration for this execution
    pub config: MicroVMConfig,
    /// Runtime context for security and capability control
    pub runtime_context: Option<RuntimeContext>,
}
```

## 4. Provider System

### 4.1. MicroVMProvider Trait

```rust
pub trait MicroVMProvider: Send + Sync {
    /// Provider name for identification
    fn name(&self) -> &str;
    
    /// Check if this provider is available on the current system
    fn is_available(&self) -> bool;
    
    /// Initialize the provider with configuration
    fn initialize(&mut self, config: MicroVMConfig) -> RuntimeResult<()>;
    
    /// Execute a program in the MicroVM
    fn execute_program(&self, context: ExecutionContext) -> RuntimeResult<ExecutionResult>;
    
    /// Execute a capability in the MicroVM
    fn execute_capability(&self, context: ExecutionContext) -> RuntimeResult<ExecutionResult>;
    
    /// Clean up resources
    fn cleanup(&mut self) -> RuntimeResult<()>;
    
    /// Get configuration schema for this provider
    fn get_config_schema(&self) -> serde_json::Value;
}
```

### 4.2. Provider Implementations

#### Mock Provider
- **Purpose**: Testing and development
- **Isolation**: None
- **Security**: Minimal boundary validation
- **Performance**: 1x baseline

#### Process Provider
- **Purpose**: Basic process isolation
- **Isolation**: Process-level
- **Security**: Capability-based access control
- **Performance**: 2-5x overhead

#### WASM Provider
- **Purpose**: WebAssembly sandbox execution
- **Isolation**: WASM sandbox
- **Security**: Medium security with sandboxing
- **Performance**: 2-4x overhead

#### Firecracker Provider
- **Purpose**: Production VM isolation
- **Isolation**: Full VM with jailer
- **Security**: High security with attestation
- **Performance**: 10-20x overhead

#### gVisor Provider
- **Purpose**: Container isolation
- **Isolation**: Container with gVisor
- **Security**: Medium security with container isolation
- **Performance**: 3-8x overhead

## 5. Security Architecture

### 5.1. Central Authorization System

The `SecurityAuthorizer` provides centralized security enforcement:

```rust
pub struct SecurityAuthorizer;

impl SecurityAuthorizer {
    /// Authorize a capability execution request
    pub fn authorize_capability(
        runtime_context: &RuntimeContext,
        capability_id: &str,
        args: &[Value],
    ) -> RuntimeResult<Vec<String>>;
    
    /// Authorize a program execution request
    pub fn authorize_program(
        runtime_context: &RuntimeContext,
        program: &Program,
        capability_id: Option<&str>,
    ) -> RuntimeResult<Vec<String>>;
    
    /// Validate execution context permissions
    pub fn validate_execution_context(
        required_permissions: &[String],
        execution_context: &ExecutionContext,
    ) -> RuntimeResult<()>;
}
```

### 5.2. Security Levels

#### Pure Security Level
- **Capabilities**: None allowed
- **MicroVM**: Not used
- **Use Case**: Maximum security for untrusted code

#### Controlled Security Level
- **Capabilities**: Explicit allowlist
- **MicroVM**: Required for dangerous operations
- **Use Case**: Standard production workloads

#### Full Security Level
- **Capabilities**: All allowed
- **MicroVM**: Optional
- **Use Case**: Trusted code execution

### 5.3. Capability Classification

#### Safe Capabilities (No MicroVM Required)
- `ccos.system.current-time`
- `ccos.math.add`
- `ccos.string.concat`

#### Dangerous Capabilities (MicroVM Required)
- `ccos.io.open-file`
- `ccos.io.read-line`
- `ccos.io.write-line`
- `ccos.network.http-fetch`
- `ccos.system.get-env`

## 6. Enhanced Firecracker Provider

### 6.1. Security Features

```rust
pub struct SecurityFeatures {
    pub seccomp_enabled: bool,
    pub jailer_enabled: bool,
    pub jailer_gid: Option<u32>,
    pub jailer_uid: Option<u32>,
    pub jailer_chroot_base: Option<PathBuf>,
    pub jailer_netns: Option<String>,
    pub seccomp_filter_path: Option<PathBuf>,
    pub enable_balloon: bool,
    pub enable_entropy: bool,
}
```

### 6.2. Resource Monitoring

```rust
pub struct ResourceLimits {
    pub max_cpu_time: Duration,
    pub max_memory_mb: u32,
    pub max_disk_io_mb: u32,
    pub max_network_io_mb: u32,
    pub max_processes: u32,
    pub max_open_files: u32,
}
```

### 6.3. Performance Optimization

```rust
pub struct PerformanceTuning {
    pub cpu_pinning: bool,
    pub memory_hugepages: bool,
    pub io_scheduler: String,
    pub network_optimization: bool,
    pub cache_prefetch: bool,
}
```

### 6.4. Attestation Support

```rust
pub struct AttestationConfig {
    pub enabled: bool,
    pub expected_kernel_hash: Option<String>,
    pub expected_rootfs_hash: Option<String>,
    pub tpm_enabled: bool,
    pub measured_boot: bool,
}
```

## 7. Configuration System

### 7.1. MicroVM Configuration

```rust
pub struct MicroVMConfig {
    /// Default provider to use
    pub default_provider: String,
    /// Provider-specific configuration
    pub provider_config: HashMap<String, serde_json::Value>,
    /// Security settings
    pub security: SecurityConfig,
    /// Resource limits
    pub resource_limits: ResourceLimits,
    /// Performance tuning
    pub performance: PerformanceConfig,
}
```

### 7.2. RTFS Configuration Integration

The system supports parsing RTFS `agent.config` expressions:

```clojure
(agent.config 
  :version "0.1"
  :agent-id "agent.test"
  :profile :microvm
  :microvm {
    :kernel {:image "kernels/vmlinuz-min" :cmdline "console=none"}
    :rootfs {:image "images/agent-rootfs.img" :ro true}
    :resources {:vcpus 1 :mem_mb 256}
    :devices {:nic {:enabled true :proxy_ns "egress-proxy-ns"}}
    :attestation {:enabled true :expect_rootfs_hash "sha256:..."}
  })
```

## 8. Integration with CCOS

### 8.1. Capability Registry Integration

```rust
impl CapabilityRegistry {
    /// Execute capability with MicroVM isolation
    pub fn execute_capability_with_microvm(
        &self,
        capability_id: &str,
        args: Vec<Value>,
        runtime_context: Option<&RuntimeContext>,
    ) -> RuntimeResult<Value>;
}
```

### 8.2. Step Special Form Integration

MicroVM execution integrates with RTFS step special forms:

```clojure
(step "Secure Network Call"
  (call :ccos.network.http-fetch ["https://api.example.com/data"]))
```

## 9. Testing and Quality Assurance

### 9.1. Test Categories

1. **Provider Lifecycle Tests**: Initialization, execution, cleanup
2. **Security Tests**: Authorization, boundary validation, policy enforcement
3. **Performance Tests**: Benchmarking, resource monitoring
4. **Configuration Tests**: RTFS parsing, validation, integration
5. **Integration Tests**: End-to-end capability execution

### 9.2. Test Coverage

- **35 Total Tests**: Comprehensive coverage of all features
- **Security Validation**: All security features tested
- **Provider Coverage**: All 5 providers tested
- **Configuration Testing**: RTFS integration validated

## 10. Performance Characteristics

### 10.1. Startup Times

| Provider | Startup Time | Memory Overhead | Security Level |
|----------|--------------|-----------------|----------------|
| Mock | <1ms | 0MB | Minimal |
| Process | 10-50ms | 5-20MB | Low |
| WASM | 20-100ms | 10-50MB | Medium |
| Firecracker | 200-1000ms | 50-200MB | High |
| gVisor | 50-200ms | 20-100MB | Medium |

### 10.2. Resource Usage

- **CPU**: Provider-dependent overhead (1x to 20x)
- **Memory**: 0MB to 200MB base overhead
- **Network**: Minimal overhead for network operations
- **Storage**: Temporary storage for VM images and runtime

## 11. Future Enhancements

### 11.1. Planned Features

1. **Advanced Attestation**: TPM integration and measured boot
2. **Network Isolation**: Advanced networking policies and proxy integration
3. **Resource Scheduling**: Dynamic resource allocation and scheduling
4. **Monitoring Integration**: Prometheus metrics and health checks
5. **Orchestration Integration**: Full CCOS orchestrator integration

### 11.2. Performance Optimizations

1. **VM Pooling**: Reuse VM instances for faster startup
2. **Image Caching**: Cache VM images and configurations
3. **Parallel Execution**: Support for parallel MicroVM execution
4. **Resource Sharing**: Optimize resource usage across VMs

## 12. References

- [SEP-000: System Architecture](./000-ccos-architecture.md)
- [SEP-005: Security and Context](./005-security-and-context.md)
- [SEP-015: Execution Contexts](./015-execution-contexts.md)
- [SEP-004: Capabilities and Marketplace](./004-capabilities-and-marketplace.md)
