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

### Docker Image

```dockerfile
# Dockerfile.tiny
FROM rust:1.83-slim-bookworm as builder
# ... build with minimal features ...

FROM debian:bookworm-slim
# Includes Python for sandboxed capabilities
# Final image: ~552MB
```

Build: `docker build -f Dockerfile.tiny -t ccos-tiny .`

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

## Testing

Integration tests are available in `ccos/tests/test_sandboxed_capability.rs`:

```bash
cargo test --test test_sandboxed_capability -- --nocapture
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
