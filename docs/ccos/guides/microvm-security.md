# MicroVM Security Worktree

This worktree implements **per-step MicroVM security controls** for the CCOS (Cognitive Computing Operating System) RTFS runtime. It provides fine-grained security profiles that are automatically derived and enforced for each step in a plan execution.

## Overview

The MicroVM security system enhances CCOS with:

- **Per-step profile derivation**: Automatically analyzes each step's operations to determine required security controls
- **MicroVM enforcement**: Runs dangerous operations in isolated MicroVM environments
- **Network ACLs**: Fine-grained network access control per step
- **Filesystem policies**: Path-based file system access control
- **Determinism flags**: Tracks whether operations are deterministic/reproducible
- **Resource limits**: Per-step resource constraints (CPU, memory, time, I/O)

## Safety Model

### Core Principles

1. **Principle of Least Privilege**: Each step gets exactly the permissions it needs, nothing more
2. **Automatic Profile Derivation**: Security profiles are derived from step expressions, not manually configured
3. **Layered Isolation**: Multiple isolation levels (Inherit → Isolated → Sandboxed) based on operation risk
4. **Immutable Auditing**: All security decisions are logged to the Causal Chain
5. **Runtime Context Awareness**: Profiles are adjusted based on the current runtime security context

### Isolation Levels

- **Inherit**: Step runs with same privileges as parent context (for safe operations)
- **Isolated**: Step runs in separate MicroVM with controlled resource limits
- **Sandboxed**: Step runs in highly restricted environment with syscall filtering

### Operation Classification

The system automatically classifies operations to determine security requirements:

**Dangerous Operations** (Sandboxed):
- System calls (`exec`, `shell`, `process`)
- External program execution
- Raw system access

**High-Risk Operations** (Isolated):
- Network access (`http-fetch`, `socket`, `fetch`)
- File system operations (`open`, `read`, `write`)
- System environment access

**Low-Risk Operations** (Inherit):
- Pure functions
- Data transformations
- Math operations
- Local data processing

## Usage

### Basic Plan Execution with Security

```clojure
; This plan will automatically get appropriate security profiles
(step "Process user data"
  (call :data.process {:input user-data}))

(step "Fetch external API data"
  (call :http.fetch {:url "https://api.example.com/data"}))
```

The first step will run with minimal privileges (Inherit isolation), while the second step will automatically get network access controls and isolated execution.

### Manual Profile Override

```clojure
; Force sandboxed execution for sensitive operations
(step "Handle sensitive data"
  (with-security-profile {:isolation_level :sandboxed}
    (call :data.encrypt {:data sensitive-info})))
```

## Testing

### Running MicroVM Security Tests

```bash
# Run all microvm security tests
cargo test -p rtfs_compiler microvm_security

# Run specific test categories
cargo test -p rtfs_compiler test_step_profile_derivation
cargo test -p rtfs_compiler test_isolation_enforcement
cargo test -p rtfs_compiler test_network_acls
```

### Test Structure

The test suite includes:

1. **Profile Derivation Tests**: Verify correct security profiles are derived from step expressions
2. **Isolation Enforcement Tests**: Test that operations run with appropriate isolation levels
3. **Network ACL Tests**: Validate network access controls work correctly
4. **Filesystem Policy Tests**: Test file system access restrictions
5. **Resource Limit Tests**: Verify CPU, memory, and time limits are enforced

### Example Test Cases

```rust
#[test]
fn test_network_operation_isolation() {
    let step_expr = parse_expression("(call :http.fetch {:url \"https://api.com\"})");
    let profile = StepProfileDeriver::derive_profile("fetch-data", &step_expr, &runtime_context);

    assert_eq!(profile.isolation_level, IsolationLevel::Isolated);
    assert_eq!(profile.microvm_config.network_policy, NetworkPolicy::AllowList(vec!["api.com".to_string()]));
    assert_eq!(profile.security_flags.enable_network_acl, true);
}

#[test]
fn test_dangerous_operation_sandboxing() {
    let step_expr = parse_expression("(call :system.execute {:cmd \"rm -rf /\"})");
    let profile = StepProfileDeriver::derive_profile("dangerous-op", &step_expr, &runtime_context);

    assert_eq!(profile.isolation_level, IsolationLevel::Sandboxed);
    assert_eq!(profile.security_flags.enable_syscall_filter, true);
    assert_eq!(profile.security_flags.log_syscalls, true);
}
```

## Configuration

### Runtime Context Configuration

```rust
// Controlled runtime context with MicroVM enabled
let context = RuntimeContext::controlled(vec![
    "ccos.io.log".to_string(),
    "ccos.network.http-fetch".to_string(),
])
.with_microvm(true)
.with_isolation_levels(true, true, false); // Allow inherit, isolated, but not sandboxed
```

### MicroVM Provider Configuration

The system supports multiple MicroVM providers:

- **Firecracker**: Lightweight virtualization (recommended)
- **gVisor**: Container-based sandboxing
- **Process**: Process-level isolation (development)
- **WASM**: WebAssembly-based isolation

## Security Profiles

### StepProfile Structure

Each step gets a comprehensive security profile:

```rust
pub struct StepProfile {
    pub profile_id: String,                    // Unique profile identifier
    pub step_name: String,                     // Step name/description
    pub isolation_level: IsolationLevel,       // Required isolation level
    pub microvm_config: MicroVMConfig,         // MicroVM configuration
    pub deterministic: bool,                   // Whether step is deterministic
    pub resource_limits: ResourceLimits,       // Resource constraints
    pub security_flags: SecurityFlags,         // Security enforcement flags
}
```

### Resource Limits

```rust
pub struct ResourceLimits {
    pub max_execution_time_ms: u64,            // Maximum execution time
    pub max_memory_bytes: u64,                 // Memory limit
    pub max_cpu_usage: f64,                    // CPU limit (multiplier)
    pub max_io_operations: Option<u64>,        // I/O operation limit
    pub max_network_bandwidth: Option<u64>,    // Network bandwidth limit
}
```

### Security Flags

```rust
pub struct SecurityFlags {
    pub enable_syscall_filter: bool,           // System call filtering
    pub enable_network_acl: bool,              // Network access control
    pub enable_fs_acl: bool,                   // File system access control
    pub enable_memory_protection: bool,        // Memory protection
    pub enable_cpu_monitoring: bool,           // CPU usage monitoring
    pub log_syscalls: bool,                    // System call logging
    pub read_only_fs: bool,                    // Read-only file system
}
```

## Integration Points

### Orchestrator Integration

The `Orchestrator` now includes step profile functionality:

```rust
// Derive profile for a step before execution
orchestrator.derive_step_profile(step_name, &step_expr, &runtime_context)?;

// Get current step profile during execution
let profile = orchestrator.get_current_step_profile();

// Clear profile after step completion
orchestrator.clear_step_profile();
```

### Causal Chain Integration

All security decisions are logged to the Causal Chain:

- `StepProfileDerived`: When a security profile is derived for a step
- `StepStarted`: When a step begins execution with its profile
- `StepCompleted`: When a step completes successfully
- `StepFailed`: When a step fails (potentially due to security violations)

## Development

### Adding New Operation Types

To add support for new operation types:

1. Update `StepProfileDeriver::contains_*` methods to detect the new operations
2. Add appropriate security profile rules in the derivation logic
3. Add test cases for the new operation type

### Custom Security Profiles

For custom security profiles beyond automatic derivation:

```rust
impl StepProfileDeriver {
    pub fn custom_profile(step_name: &str, custom_config: &CustomSecurityConfig) -> StepProfile {
        // Custom profile creation logic
    }
}
```

## Troubleshooting

### Common Issues

1. **Profile derivation fails**: Check that step expressions are valid RTFS syntax
2. **Isolation level rejected**: Verify runtime context allows the required isolation level
3. **Resource limits exceeded**: Adjust resource limits in runtime context or step profile
4. **Network access denied**: Check network ACL configuration in derived profile

### Debugging

Enable debug logging to see profile derivation decisions:

```rust
// Enable detailed security logging
let context = runtime_context.with_log_security(true);
```

### Performance Considerations

- Profile derivation has minimal overhead (typically <1ms per step)
- MicroVM startup adds ~50-200ms overhead per isolated step
- Network ACLs have negligible performance impact
- File system policies may add small I/O overhead

## Contributing

When contributing to the microvm-security worktree:

1. **Add tests** for any new security enforcement rules
2. **Update documentation** when adding new operation types
3. **Consider performance impact** of security changes
4. **Follow the principle of least privilege** in default configurations
5. **Test with all isolation levels** (inherit, isolated, sandboxed)

## Related Issues

- **Issue #71**: MicroVM Control Plane and Security Hardening
- **Issue #72**: MicroVM Step-Level Policy Enforcement
- **Issue #60**: Orchestrator: derive per-step MicroVM profile
