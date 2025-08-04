# MicroVM Providers Guide

## Overview

The RTFS MicroVM system provides a pluggable architecture for secure execution environments with multiple isolation providers. This guide covers the implementation, configuration, and usage of all available MicroVM providers.

## Architecture

### Core Components

- **MicroVMProvider Trait**: Common interface for all isolation providers
- **ExecutionContext**: Execution context with program, security, and configuration
- **ExecutionResult**: Results with metadata and performance information
- **MicroVMFactory**: Central registry and management of providers

### Security Model

Each provider implements different levels of isolation:

1. **Mock Provider**: No isolation (testing only)
2. **Process Provider**: OS-level process isolation
3. **Firecracker Provider**: Full VM isolation (strongest)
4. **gVisor Provider**: Container-like isolation
5. **WASM Provider**: WebAssembly sandbox isolation

## Provider Implementations

### 1. MockMicroVMProvider

**Purpose**: Development and testing
**Isolation Level**: None (simulated)
**Availability**: Always available

#### Features
- Simulates execution with realistic metadata
- Handles different capability types
- Provides execution timing and resource usage simulation
- Always available for testing

#### Configuration
```rust
let config = MicroVMConfig {
    timeout: Duration::from_secs(30),
    memory_limit_mb: 512,
    cpu_limit: 1.0,
    network_policy: NetworkPolicy::AllowList(vec!["api.github.com".to_string()]),
    fs_policy: FileSystemPolicy::ReadOnly,
    env_vars: HashMap::new(),
};
```

#### Usage Example
```rust
let mut factory = MicroVMFactory::new();
factory.initialize_provider("mock")?;

let provider = factory.get_provider("mock")?;
let context = ExecutionContext {
    execution_id: "test-execution".to_string(),
    program: Some(Program::RtfsSource("(+ 1 2)".to_string())),
    capability_id: None,
    capability_permissions: vec![],
    args: vec![],
    config: MicroVMConfig::default(),
    runtime_context: Some(RuntimeContext::unrestricted()),
};

let result = provider.execute_program(context)?;
println!("Result: {:?}", result);
```

### 2. ProcessMicroVMProvider

**Purpose**: Lightweight OS-level isolation
**Isolation Level**: Process-level
**Availability**: All platforms

#### Features
- Spawns separate processes with restricted permissions
- OS-level isolation using process boundaries
- Supports external program execution
- Cross-platform compatibility

#### Configuration
```rust
let config = MicroVMConfig {
    timeout: Duration::from_secs(60),
    memory_limit_mb: 1024,
    cpu_limit: 0.5,
    network_policy: NetworkPolicy::Denied,
    fs_policy: FileSystemPolicy::ReadOnly,
    env_vars: HashMap::new(),
};
```

#### Usage Example
```rust
let mut factory = MicroVMFactory::new();
factory.initialize_provider("process")?;

let provider = factory.get_provider("process")?;
let context = ExecutionContext {
    execution_id: "process-execution".to_string(),
    program: Some(Program::ExternalProgram {
        path: "/usr/bin/echo".to_string(),
        args: vec!["Hello, World!".to_string()],
    }),
    capability_id: None,
    capability_permissions: vec![],
    args: vec![],
    config: MicroVMConfig::default(),
    runtime_context: Some(RuntimeContext::unrestricted()),
};

let result = provider.execute_program(context)?;
```

### 3. FirecrackerMicroVMProvider

**Purpose**: Full VM isolation (strongest security)
**Isolation Level**: Virtual Machine
**Availability**: Linux only (requires Firecracker binary)

#### Features
- Full VM isolation using Firecracker
- Unix domain socket communication
- VM lifecycle management (start, stop, cleanup)
- Simulated RTFS runtime deployment

#### Prerequisites
```bash
# Install Firecracker
curl -Lo firecracker https://github.com/firecracker-microvm/firecracker/releases/download/v1.4.0/firecracker-v1.4.0
chmod +x firecracker
sudo mv firecracker /usr/local/bin/
```

#### Configuration
```rust
let config = MicroVMConfig {
    timeout: Duration::from_secs(120),
    memory_limit_mb: 2048,
    cpu_limit: 1.0,
    network_policy: NetworkPolicy::AllowList(vec!["api.github.com".to_string()]),
    fs_policy: FileSystemPolicy::ReadOnly,
    env_vars: HashMap::new(),
};
```

#### Usage Example
```rust
let mut factory = MicroVMFactory::new();
factory.initialize_provider("firecracker")?;

let provider = factory.get_provider("firecracker")?;
let context = ExecutionContext {
    execution_id: "firecracker-execution".to_string(),
    program: Some(Program::RtfsSource("(println \"Hello from Firecracker VM\")".to_string())),
    capability_id: None,
    capability_permissions: vec![],
    args: vec![],
    config: MicroVMConfig::default(),
    runtime_context: Some(RuntimeContext::unrestricted()),
};

let result = provider.execute_program(context)?;
```

### 4. GvisorMicroVMProvider

**Purpose**: Container-like isolation with faster startup
**Isolation Level**: Container
**Availability**: Linux only (requires runsc binary)

#### Features
- Container-based isolation using gVisor
- User-space kernel for security
- Resource limits and security options
- RTFS runtime deployment within containers

#### Prerequisites
```bash
# Install gVisor
wget https://storage.googleapis.com/gvisor/releases/nightly/latest/runsc
chmod +x runsc
sudo mv runsc /usr/local/bin/
```

#### Configuration
```rust
let config = MicroVMConfig {
    timeout: Duration::from_secs(90),
    memory_limit_mb: 1536,
    cpu_limit: 0.8,
    network_policy: NetworkPolicy::AllowList(vec!["api.github.com".to_string()]),
    fs_policy: FileSystemPolicy::ReadOnly,
    env_vars: HashMap::new(),
};
```

#### Usage Example
```rust
let mut factory = MicroVMFactory::new();
factory.initialize_provider("gvisor")?;

let provider = factory.get_provider("gvisor")?;
let context = ExecutionContext {
    execution_id: "gvisor-execution".to_string(),
    program: Some(Program::RtfsSource("(println \"Hello from gVisor container\")".to_string())),
    capability_id: None,
    capability_permissions: vec![],
    args: vec![],
    config: MicroVMConfig::default(),
    runtime_context: Some(RuntimeContext::unrestricted()),
};

let result = provider.execute_program(context)?;
```

### 5. WasmMicroVMProvider

**Purpose**: WebAssembly-based isolation
**Isolation Level**: WASM sandbox
**Availability**: All platforms (requires wasm feature flag)

#### Features
- WebAssembly sandbox isolation
- Cross-platform compatibility
- Fast startup and execution
- Memory and resource limits

#### Configuration
```rust
let config = MicroVMConfig {
    timeout: Duration::from_secs(60),
    memory_limit_mb: 512,
    cpu_limit: 0.5,
    network_policy: NetworkPolicy::Denied,
    fs_policy: FileSystemPolicy::None,
    env_vars: HashMap::new(),
};
```

#### Usage Example
```rust
let mut factory = MicroVMFactory::new();
factory.initialize_provider("wasm")?;

let provider = factory.get_provider("wasm")?;
let context = ExecutionContext {
    execution_id: "wasm-execution".to_string(),
    program: Some(Program::RtfsSource("(+ 1 2)".to_string())),
    capability_id: None,
    capability_permissions: vec![],
    args: vec![],
    config: MicroVMConfig::default(),
    runtime_context: Some(RuntimeContext::unrestricted()),
};

let result = provider.execute_program(context)?;
```

## Security Policies

### NetworkPolicy

```rust
pub enum NetworkPolicy {
    Denied,                                    // No network access
    AllowList(Vec<String>),                   // Only specified hosts
    DenyList(Vec<String>),                    // All except specified hosts
    Full,                                      // Full network access
}
```

### FileSystemPolicy

```rust
pub enum FileSystemPolicy {
    None,                                      // No file system access
    ReadOnly,                                  // Read-only access
    ReadWrite,                                 // Read-write access
    Full,                                      // Full file system access
}
```

## Configuration Best Practices

### Security-First Configuration

```rust
// High-security configuration for untrusted code
let secure_config = MicroVMConfig {
    timeout: Duration::from_secs(30),
    memory_limit_mb: 256,
    cpu_limit: 0.25,
    network_policy: NetworkPolicy::Denied,
    fs_policy: FileSystemPolicy::None,
    env_vars: HashMap::new(),
};
```

### Performance Configuration

```rust
// Performance-optimized configuration for trusted code
let perf_config = MicroVMConfig {
    timeout: Duration::from_secs(300),
    memory_limit_mb: 4096,
    cpu_limit: 2.0,
    network_policy: NetworkPolicy::Full,
    fs_policy: FileSystemPolicy::ReadWrite,
    env_vars: HashMap::new(),
};
```

### Development Configuration

```rust
// Development-friendly configuration
let dev_config = MicroVMConfig {
    timeout: Duration::from_secs(60),
    memory_limit_mb: 1024,
    cpu_limit: 1.0,
    network_policy: NetworkPolicy::AllowList(vec!["localhost".to_string()]),
    fs_policy: FileSystemPolicy::ReadOnly,
    env_vars: HashMap::new(),
};
```

## Testing and Validation

### Comprehensive Test Suite

The MicroVM system includes comprehensive tests covering:

1. **Performance Tests**: Execution time comparison across providers
2. **Security Tests**: Capability restriction enforcement
3. **Resource Tests**: Memory and CPU limit enforcement
4. **Error Handling Tests**: Graceful failure handling
5. **Concurrent Tests**: Multi-threaded execution validation
6. **Configuration Tests**: Configuration validation and schema
7. **Lifecycle Tests**: Provider initialization and cleanup
8. **Integration Tests**: Capability system integration

### Running Tests

```bash
# Run all MicroVM tests
cargo test --lib microvm_tests

# Run specific provider tests
cargo test --lib test_mock_provider_program_execution
cargo test --lib test_process_provider_rtfs_execution
cargo test --lib test_firecracker_provider_availability
cargo test --lib test_gvisor_provider_availability

# Run performance tests
cargo test --lib test_microvm_provider_performance_comparison

# Run security tests
cargo test --lib test_microvm_provider_security_isolation
```

## Troubleshooting

### Common Issues

1. **Provider Not Available**
   - Check if required binaries are installed (firecracker, runsc)
   - Verify platform compatibility
   - Check feature flags for WASM provider

2. **Execution Timeouts**
   - Increase timeout in MicroVMConfig
   - Check resource limits
   - Verify program complexity

3. **Permission Denied**
   - Check capability permissions in RuntimeContext
   - Verify network and file system policies
   - Ensure proper security configuration

4. **Resource Limits Exceeded**
   - Increase memory_limit_mb or cpu_limit
   - Optimize program resource usage
   - Consider using a different provider

### Debugging

Enable debug logging:

```rust
use log::{debug, info, warn, error};

// In your code
debug!("Provider execution started: {:?}", context);
info!("Provider execution completed: {:?}", result);
warn!("Resource usage high: {:?}", metadata);
error!("Provider execution failed: {:?}", error);
```

## Performance Considerations

### Provider Selection Guide

| Use Case | Recommended Provider | Reasoning |
|----------|---------------------|-----------|
| Development/Testing | Mock | Fast, no isolation overhead |
| Lightweight Isolation | Process | Good balance of security and performance |
| High Security | Firecracker | Strongest isolation, higher overhead |
| Container Environment | gVisor | Fast startup, good isolation |
| Cross-platform | WASM | Platform-independent, sandboxed |

### Performance Benchmarks

Typical execution times (relative):

- **Mock Provider**: 1x (baseline)
- **Process Provider**: 2-5x
- **gVisor Provider**: 3-8x
- **Firecracker Provider**: 10-20x
- **WASM Provider**: 2-4x

### Optimization Tips

1. **Use appropriate providers** for your security requirements
2. **Configure resource limits** based on actual needs
3. **Reuse providers** when possible to avoid initialization overhead
4. **Monitor performance** and adjust configuration accordingly
5. **Consider caching** for frequently executed programs

## Future Enhancements

### Planned Features

1. **Provider Pooling**: Reuse provider instances for better performance
2. **Dynamic Configuration**: Runtime configuration updates
3. **Advanced Monitoring**: Detailed performance and security metrics
4. **Provider Chaining**: Combine multiple providers for layered security
5. **Custom Providers**: Framework for implementing custom isolation providers

### Integration Roadmap

1. **CCOS Integration**: Full integration with CCOS orchestration
2. **Agent Isolation**: Agent-specific isolation requirements
3. **Marketplace Integration**: Provider selection based on capability requirements
4. **Security Auditing**: Comprehensive security audit trails

## Conclusion

The MicroVM system provides a flexible, secure, and performant foundation for isolated program execution. By choosing the appropriate provider and configuration for your use case, you can achieve the right balance of security, performance, and functionality.

For more information, see the [MicroVM Architecture Documentation](../../ccos/specs/000-ccos-architecture.md) and the [RTFS-CCOS Integration Guide](../13-rtfs-ccos-integration-guide.md). 