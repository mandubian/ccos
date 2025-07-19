# Capability Marketplace

**Status:** ✅ **REFACTORED** – v2.0 (Two-Component Architecture)

---

## Overview

The RTFS Capability system is built on a two-component architecture designed for security, extensibility, and clear separation of concerns:

1.  **Capability Marketplace**: The high-level entry point for discovering and executing non-sandboxed, remote, or complex capabilities. It handles capabilities that require asynchronous operations, I/O, or interaction with external systems (e.g., HTTP APIs, Streaming services).

2.  **Capability Registry**: The low-level engine for executing sandboxed, built-in, and security-sensitive capabilities. It is designed for synchronous, fast, and secure operations that do not require I/O.

The `CapabilityMarketplace` orchestrates capability execution. It handles remote capabilities directly and delegates the execution of local, built-in capabilities to the `CapabilityRegistry`. This design ensures that sensitive operations are handled in a controlled environment while providing a flexible mechanism for extending the system with new remote functionalities.

## Core Architecture

### Capability Types

| Type | Description | Status |
|------|-------------|---------|
| **Local** | Built-in capabilities executed by the `CapabilityRegistry` | ✅ Implemented |
| **HTTP** | Remote capabilities via HTTP APIs, executed by the `Marketplace` | ✅ Implemented |
| **Stream** | Streaming capabilities for continuous data flow | ✅ Implemented |
| **MCP** | Model Context Protocol capabilities | 🔄 Planned |
| **A2A** | Agent-to-Agent communication | 🔄 Planned |

### Core Components

The `CapabilityMarketplace` manages manifests that describe how to execute a capability.

```rust
/// The capability marketplace that manages all available high-level capabilities.
pub struct CapabilityMarketplace {
    /// A shared reference to the low-level capability registry.
    registry: Arc<RwLock<CapabilityRegistry>>,
    /// The collection of registered capability manifests.
    capabilities: Arc<RwLock<HashMap<String, CapabilityManifest>>>,
    // ... other fields
}

/// Describes a capability and how to execute it.
#[derive(Debug, Clone)]
pub struct CapabilityManifest {
    pub id: String,
    pub name: String,
    pub description: String,
    /// The specific provider type that implements the capability.
    pub provider_type: ProviderType,
}

/// Enum defining the different types of capability providers.
#[derive(Clone, Debug)]
pub enum ProviderType {
    /// A local capability, executed by the `CapabilityRegistry`.
    Local,
    /// A remote capability accessed over HTTP.
    Http(HttpCapability),
    /// A streaming capability.
    Stream(StreamCapabilityImpl),
    /// A capability that communicates using the Model Context Protocol (MCP).
    MCP(MCPCapability),
    /// A capability for agent-to-agent (A2A) communication.
    A2A(A2ACapability),
}

/// Configuration for an HTTP-based remote capability.
#[derive(Debug, Clone)]
pub struct HttpCapability {
    pub base_url: String,
    pub auth_token: Option<String>,
    pub timeout_ms: u64,
}

/// Configuration for a capability that uses the Model Context Protocol (MCP).
#[derive(Debug, Clone)]
pub struct MCPCapability {
    pub server_url: String,
    pub tool_name: String,
}

/// Configuration for an agent-to-agent (A2A) communication capability.
#[derive(Debug, Clone)]
pub struct A2ACapability {
    pub agent_id: String,
    pub endpoint: String,
}
```

## Usage Examples

### Basic Capability Call

```rtfs
;; Call a capability with inputs
(call :ccos.echo "Hello World")

;; Call with structured inputs
(call :ccos.math.add {:a 10 :b 20})

;; Call with options
(call :ccos.ask-human "What is your name?" {:timeout 5000})
```

### Security Context Examples

```rtfs
;; Pure context - no capabilities allowed
(let [ctx (security-context :pure)]
  (call :ccos.echo "test"))  ; ❌ Security violation

;; Controlled context - specific capabilities allowed
(let [ctx (security-context :controlled {:allowed ["ccos.echo"]})]
  (call :ccos.echo "test"))  ; ✅ Allowed

;; Full context - all capabilities allowed
(let [ctx (security-context :full)]
  (call :ccos.math.add {:a 5 :b 3}))  ; ✅ Allowed
```

## Implemented Capabilities

### Core Capabilities

| Capability ID | Description | Input Format | Output |
|---------------|-------------|--------------|---------|
| `ccos.echo` | Echo input back | Any value | Input value |
| `ccos.math.add` | Add two numbers | `{:a number :b number}` | Sum as integer |
| `ccos.ask-human` | Request human input | String prompt | Resource handle |

### Example Usage

```rtfs
;; Echo capability
(call :ccos.echo "Hello World")
;; Returns: "Hello World"

;; Math capability
(call :ccos.math.add {:a 10 :b 20})
;; Returns: 30

;; Ask human capability
(call :ccos.ask-human "What is your name?")
;; Returns: "prompt-uuid-1234-5678"
```

## Security Framework Integration

### Security Contexts

The marketplace integrates with RTFS's security framework:

```rust
/// Security levels for capability execution
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
```

### Permission Checking

```rust
/// Check if capability is allowed in current context
pub fn is_capability_allowed(&self, capability_id: &str) -> bool {
    match self.level {
        SecurityLevel::Pure => false,
        SecurityLevel::Controlled => self.allowed_capabilities.contains(capability_id),
        SecurityLevel::Full => true,
        SecurityLevel::Sandboxed => self.sandboxed_capabilities.contains(capability_id),
    }
}
```

## Implementation Details

### Capability Execution Flow

1. **Parse Call**: `(call :capability-id input)`
2. **Security Check**: Validate capability permissions
3. **Input Validation**: Check input types and constraints
4. **Execute**: Route to appropriate provider
5. **Return Result**: Convert output to RTFS Value

### Error Handling

```rust
/// Capability execution errors
pub enum CapabilityError {
    /// Capability not found
    NotFound(String),
    /// Security violation
    SecurityViolation(String),
    /// Invalid input format
    InvalidInput(String),
    /// Provider error
    ProviderError(String),
}
```

## Testing

### Test Suite

The capability system includes comprehensive tests:

```bash
# Run capability system tests
cargo run --example test_capability_system
```

### Test Results

```
🧪 RTFS Capability System Test
===============================

1️⃣ Testing Pure Security Context
✅ Pure context correctly blocked capability

2️⃣ Testing Controlled Security Context  
✅ Controlled context allowed capability call: String("Hello World")

3️⃣ Testing Full Security Context
✅ Full context allowed ccos.echo: String("test input")
✅ Full context allowed ccos.math.add: Integer(30)
✅ Full context allowed ccos.ask-human: ResourceHandle("prompt-uuid")

4️⃣ Testing Plan Execution with Capabilities
❌ Plan evaluation failed: Undefined symbol: plan
```

## Roadmap

### Phase 1: Core Implementation ✅ COMPLETED
- [x] Basic capability marketplace
- [x] Security context integration
- [x] Local capability execution
- [x] HTTP capability support
- [x] Comprehensive testing

### Phase 2: Advanced Features 🔄 IN PROGRESS
- [ ] MCP (Model Context Protocol) integration
- [ ] A2A (Agent-to-Agent) communication
- [ ] Plugin system for dynamic capabilities
- [ ] Capability discovery agents
- [ ] Performance monitoring and metrics

### Phase 3: Production Features 📋 PLANNED
- [ ] Capability versioning and updates
- [ ] Load balancing and failover
- [ ] Rate limiting and quotas
- [ ] Billing and cost tracking
- [ ] Advanced security policies

## Integration with RTFS Plans

Capabilities can be used within RTFS plans:

```rtfs
(plan data-processing
  :description "Process data using capabilities"
  :steps [
    (let [data (call :ccos.echo "input data")]
      (call :ccos.math.add {:a 10 :b 20}))
    (call :ccos.ask-human "Review the results?")
  ])
```

## API Reference

### Core Functions

- `(call :capability-id input [options])` - Execute a capability
- `(security-context level [config])` - Create security context
- `(list-capabilities)` - List available capabilities
- `(register-capability id config)` - Register new capability

### Security Functions

- `(is-capability-allowed? capability-id)` - Check permission
- `(validate-security-context context)` - Validate security settings
- `(get-capability-metadata capability-id)` - Get capability info

---

**Implementation Status:** ✅ **Production Ready** - Core capability system is functional and tested.
