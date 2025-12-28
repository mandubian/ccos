# Sandbox Isolation Strategy

**Status**: Implemented  
**Issue**: #163  
**Version**: 1.0.0

## Overview

CCOS provides a sandboxed execution environment for running untrusted or resource-constrained capabilities. This document describes the architecture, components, and usage patterns for sandbox isolation.

## Goals

1. **Security**: Execute untrusted code (Python, scripts) without compromising the host system
2. **Minimal Footprint**: Provide a "tiny" build profile that strips TUI, server, and LLM dependencies
3. **Flexibility**: Support multiple isolation providers (process, firecracker, gvisor, wasm)
4. **Integration**: Seamless integration with the CapabilityMarketplace

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    CapabilityMarketplace                        │
├─────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐  │
│  │ MCPExecutor │  │ HttpExecutor│  │   SandboxedExecutor     │  │
│  └─────────────┘  └─────────────┘  └───────────┬─────────────┘  │
│                                                │                │
│                                    ┌───────────▼─────────────┐  │
│                                    │    MicroVMFactory       │  │
│                                    ├─────────────────────────┤  │
│                                    │ ┌─────────┐ ┌─────────┐ │  │
│                                    │ │ process │ │  mock   │ │  │
│                                    │ └─────────┘ └─────────┘ │  │
│                                    │ ┌─────────┐ ┌─────────┐ │  │
│                                    │ │firecracker│ │ gvisor │ │  │
│                                    │ └─────────┘ └─────────┘ │  │
│                                    └─────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

## Components

### SandboxedCapability

A new provider type representing code that should execute in isolation:

```rust
pub struct SandboxedCapability {
    pub runtime: String,        // e.g., "python", "node", "wasm"
    pub source: String,         // Code or path to executable
    pub entry_point: Option<String>,
    pub provider: Option<String>, // e.g., "process", "firecracker"
}
```

### SandboxedExecutor

Executes `SandboxedCapability` instances through the MicroVM provider system:

```rust
pub struct SandboxedExecutor {
    factory: Arc<Mutex<MicroVMFactory>>,
}
```

The executor:
1. Initializes all available providers on creation
2. Selects the appropriate provider based on `SandboxedCapability.provider`
3. Constructs an `ExecutionContext` with the program and inputs
4. Returns the execution result as an RTFS `Value`

### MicroVM Providers

| Provider | Description | Availability |
|----------|-------------|--------------|
| `process` | Basic process isolation | Always (if Python/runtime available) |
| `mock` | Testing provider | Always |
| `firecracker` | AWS Firecracker microVMs | If firecracker installed |
| `gvisor` | Google gVisor sandbox | If runsc available |
| `wasm` | WebAssembly sandbox | If wasmtime available |

## Minimal Build Profile

The `minimal` feature flag produces a lightweight CCOS binary:

### Cargo Configuration

```toml
[features]
default = ["repl", "tui", "server"]
minimal = []  # Excludes tui, server, llm
tui = ["dep:ratatui", "dep:crossterm", "dep:dialoguer"]
server = ["dep:axum", "dep:tokio-tungstenite", "dep:tower-http"]
llm = ["dep:llama_cpp"]
```

### Build Commands

```bash
# Full build (default)
cargo build -p ccos --release

# Minimal build (no TUI, server, LLM)
cargo build -p ccos --no-default-features --features minimal --release
```

### Docker Images

Two Dockerfiles are provided:

| File | Base | Size | Use Case |
|------|------|------|----------|
| `Dockerfile.tiny` | Alpine | **72MB** | Production, minimal footprint |
| `Dockerfile.debian` | Debian | 552MB | Dev/debug, full Python tooling |

```bash
# Tiny Alpine image (recommended for production)
docker build -f Dockerfile.tiny -t ccos-tiny .

# Debian image with full Python dev tools
docker build -f Dockerfile.debian -t ccos-debian .
```

## Usage

### Registering a Sandboxed Capability

```rust
let manifest = CapabilityManifest {
    id: "my.sandboxed.python".to_string(),
    name: "Python Calculator".to_string(),
    provider: ProviderType::Sandboxed(SandboxedCapability {
        runtime: "python".to_string(),
        source: r#"
import sys, json
data = json.loads(sys.argv[-1])
print(json.dumps({"result": data["a"] + data["b"]}))
"#.to_string(),
        entry_point: None,
        provider: Some("process".to_string()),
    }),
    // ... other fields
};

marketplace.register_capability_manifest(manifest).await?;
```

### Executing a Sandboxed Capability

```rust
let inputs = Value::Map(/* {"a": 5, "b": 3} */);
let result = marketplace.execute_capability("my.sandboxed.python", &inputs).await?;
// result: {"result": 8}
```

## Security Considerations

1. **Process Isolation**: The default `process` provider runs code in a separate process with limited permissions
2. **Input Sanitization**: Inputs are serialized as JSON and passed as command-line arguments
3. **Output Capture**: Only stdout is captured; stderr is logged but not returned
4. **Resource Limits**: Configurable via `ExecutionContext.config` (timeout, memory, CPU)

### Recommended Production Setup

For production environments with untrusted code:
- Use `firecracker` or `gvisor` provider for stronger isolation
- Configure network policies to restrict outbound access
- Set appropriate resource limits (memory, CPU, timeout)
- Enable audit logging for all sandbox executions

## Firecracker MicroVM Setup

For true VM-level isolation, Firecracker provides hardware-enforced security boundaries.

### Requirements

| Requirement | Details |
|-------------|---------|
| Linux only | No macOS/Windows native support |
| KVM access | Needs `/dev/kvm` (hardware virtualization) |
| x86_64 or aarch64 | Only these architectures |
| Root or kvm group | User must have access to `/dev/kvm` |

### Quick Check

```bash
# Check if KVM is available
ls -la /dev/kvm

# Check if your CPU supports virtualization
grep -E "(vmx|svm)" /proc/cpuinfo

# Test write access
test -w /dev/kvm && echo "KVM access OK"
```

### Installation

```bash
# Download Firecracker v1.6.0
mkdir -p /tmp/firecracker-setup && cd /tmp/firecracker-setup
curl -sL https://github.com/firecracker-microvm/firecracker/releases/download/v1.6.0/firecracker-v1.6.0-x86_64.tgz | tar xz

# Install binaries (requires sudo)
sudo mkdir -p /opt/firecracker
sudo cp release-v1.6.0-x86_64/firecracker-v1.6.0-x86_64 /opt/firecracker/firecracker
sudo cp release-v1.6.0-x86_64/jailer-v1.6.0-x86_64 /opt/firecracker/jailer
sudo chmod +x /opt/firecracker/firecracker /opt/firecracker/jailer
sudo ln -sf /opt/firecracker/firecracker /usr/local/bin/firecracker

# Download kernel and rootfs (quickstart assets)
curl -fsSL -o vmlinux.bin "https://s3.amazonaws.com/spec.ccfc.min/img/quickstart_guide/x86_64/kernels/vmlinux.bin"
curl -fsSL -o rootfs.ext4 "https://s3.amazonaws.com/spec.ccfc.min/img/quickstart_guide/x86_64/rootfs/bionic.rootfs.ext4"

# Install assets (requires sudo)
sudo cp vmlinux.bin /opt/firecracker/vmlinux
sudo cp rootfs.ext4 /opt/firecracker/rootfs.ext4
sudo chmod 644 /opt/firecracker/vmlinux /opt/firecracker/rootfs.ext4

# Verify installation
firecracker --version
ls -la /opt/firecracker/
```

### Expected Assets

After installation, `/opt/firecracker/` should contain:

| File | Size | Description |
|------|------|-------------|
| `firecracker` | ~2.8MB | MicroVM hypervisor |
| `jailer` | ~1MB | Security wrapper |
| `vmlinux` | ~21MB | Linux kernel |
| `rootfs.ext4` | ~300MB | Ubuntu 18.04 rootfs with Python 2.7 |

### Using Firecracker Provider

```rust
let sandboxed = SandboxedCapability {
    runtime: "python".to_string(),
    source: r#"
import json
print(json.dumps({"message": "Hello from Firecracker!", "value": 42}))
"#.to_string(),
    entry_point: None,
    provider: Some("firecracker".to_string()),  // Use Firecracker
};
```

> **Note**: The Firecracker provider falls back to direct Python execution if the full VM isn't available. This provides the security boundary while remaining functional.

## Testing

Integration tests are available in `ccos/tests/test_sandboxed_capability.rs`:

```bash
# Run all sandboxed tests
cargo test --test test_sandboxed_capability -- --nocapture

# Run Firecracker-specific tests
cargo test firecracker -- --nocapture
```

Tests cover:
- Executor creation
- Python code execution
- Provider type validation
- Marketplace integration
- Error handling

## Future Work

- [ ] WASM-based sandbox for portable execution
- [ ] Fine-grained filesystem and network policies per capability
- [ ] Resource usage metrics and quotas
- [ ] Capability attestation for sandboxed code
