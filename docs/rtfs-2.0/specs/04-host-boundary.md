# RTFS 2.0 Host Boundary and Capabilities

## Implementation Status

**✅ Implemented - Production-ready**

The RTFS 2.0 host boundary and capability system is fully implemented and integrated with CCOS governance. The implementation status is:

| Feature | Status | Notes |
|---------|--------|-------|
| **Host Boundary Model** | ✅ **Implemented** | `ExecutionOutcome::Complete/RequiresHost` in `runtime/execution_outcome.rs` |
| **Capability Invocation** | ✅ **Implemented** | `call` special form with keyword/string capability IDs |
| **Security Context** | ✅ **Implemented** | `RuntimeContext` with agent identity, intent, permissions in `runtime/security.rs` |
| **Causal Context** | ✅ **Implemented** | Audit trail tracking in causal chain |
| **Call Metadata** | ✅ **Implemented** | Timeout, idempotency, performance hints in `HostCall` |
| **CCOS Governance** | ✅ **Implemented** | Policy evaluation, provider selection, audit logging |
| **Provider Architecture** | ✅ **Implemented** | LocalProvider, Marketplace providers, custom providers |
| **Capability Marketplace** | ✅ **Implemented** | Service discovery, dynamic capability loading |
| **Execution Policies** | ✅ **Implemented** | Marketplace, Hybrid, InlineDev policy modes |
| **Resource Management** | ✅ **Implemented** | Host-mediated resource cleanup with automatic finalization |
| **Async Operations** | ✅ **Implemented** | Host-mediated futures and promises via capabilities |
| **Streaming Integration** | ✅ **Implemented** | MCP streaming and marketplace stream capabilities |
| **Error Propagation** | ✅ **Implemented** | Structured error handling across boundary |
| **Testing Support** | ✅ **Implemented** | Capability mocking for pure host interface |

### Key Implementation Details
- **Strict Separation**: Pure RTFS computation yields to host for all effects via `ExecutionOutcome::RequiresHost`
- **Mandatory Security**: Every host call includes `RuntimeContext` for governance and audit
- **Provider Plugability**: Multiple provider types (Local, Marketplace, Custom) with policy-based selection
- **Capability Contracts**: Input/output schemas using `TypeExpr` for validation
- **Causal Chain Integration**: All host calls recorded in immutable audit trail
- **Performance Hints**: `CallMetadata` with timeout, idempotency, semantic hashing
- **Resource Safety**: Automatic cleanup through host-mediated resource management

### Implementation Reference
- `runtime/execution_outcome.rs`: `ExecutionOutcome` and `HostCall` types
- `runtime/security.rs`: `RuntimeContext` and security metadata
- `runtime/host_interface.rs`: `HostInterface` trait definition
- `runtime/host.rs`: Host implementation and integration
- `ccos/src/capability_marketplace/`: CCOS capability marketplace implementation
- `ccos/src/environment.rs`: Plan-to-capability registration and host wiring
- `ccos/src/governance/`: Governance kernel and policy evaluation

**Note**: The host boundary is production-ready with comprehensive security, audit, and governance features. All external interactions are properly mediated through CCOS capabilities with mandatory security context.

## Overview

RTFS 2.0 maintains a **strict separation** between pure computation and side effects through a **host boundary**. All external interactions are mediated through CCOS capabilities with mandatory security and audit controls.

## Host Boundary Philosophy

The host boundary enables:

- **Purity**: RTFS code remains referentially transparent
- **Security**: All side effects are governed and auditable
- **Composability**: Pure and effectful code can be safely combined
- **Testability**: Pure functions are easily testable in isolation

## Execution Outcomes

RTFS evaluation returns `ExecutionOutcome`, implementing a yield-based control flow inversion:

```clojure
enum ExecutionOutcome {
  Complete(Value),           ; Pure computation finished
  RequiresHost(HostCall)     ; Host intervention needed
}
```

## Host Calls

When RTFS encounters a capability invocation:

```clojure
(call :ccos.state.kv/get "my-key")
```

It yields a `HostCall` with mandatory fields:

```clojure
struct HostCall {
  capability_id: String,        ; e.g., "ccos.state.kv.get"
  args: Vec<Value>,             ; function arguments
  security_context: RuntimeContext,  ; mandatory security metadata
  causal_context: Option<CausalContext>, ; mandatory audit trail
  metadata: Option<CallMetadata> ; optional performance hints
}
```

## Capability Invocation Syntax

RTFS provides multiple ways to invoke capabilities:

### Direct Capability Calls
```clojure
;; Fully qualified capability ID
(call :ccos.state.kv/get "my-key")
(call :ccos.state.kv/put "my-key" "my-value")

;; Database operations
(call :ccos.db/postgres/query "SELECT * FROM users WHERE id = ?" [user-id])

;; HTTP requests
(call :ccos.http/get "https://api.example.com/data")

;; File system
(call :ccos.fs/read-file "/path/to/file")
```

### Capability Discovery
```clojure
;; Discover available capabilities
(call :discover-capabilities "ccos.state.*")

;; Dynamic capability invocation
(let [cap-id (compute-capability-id)]
  (call cap-id args))
```

## Security Context

Every host call includes mandatory security metadata:

```clojure
struct RuntimeContext {
  agent_id: String,           ; executing agent
  intent_id: String,          ; current intent
  delegation_chain: Vec<String>, ; delegation history
  permissions: Vec<String>,   ; granted permissions
  environment: HashMap<String, Value> ; execution environment
}
```

## Causal Context

Audit trail for governance and debugging:

```clojure
struct CausalContext {
  parent_action_id: Option<String>,  ; parent in causal chain
  intent_id: String,                 ; originating intent
  action_path: Vec<String>,          ; execution path
  timestamp: String,                 ; when call was made
  metadata: HashMap<String, String>  ; additional audit data
}
```

## Call Metadata

Optional performance and reliability hints:

```clojure
struct CallMetadata {
  timeout_ms: Option<u64>,           ; operation timeout
  idempotency_key: Option<String>,   ; idempotent operations
  arg_type_fingerprint: u64,         ; argument type hash
  runtime_context_hash: u64,         ; context hash
  semantic_hash: Option<Vec<f32>>,   ; semantic embedding
  context: HashMap<String, String>   ; additional context
}
```

## CCOS Governance Pipeline

When RTFS yields a host call, CCOS performs:

1. **Security Validation**: Check permissions and context
2. **Governance Rules**: Apply organizational policies
3. **Provider Selection**: Choose appropriate capability provider based on execution policy
4. **Capability Resolution**: Find and validate capability in selected provider
5. **Execution**: Perform the operation through the provider
6. **Audit Logging**: Record in causal chain
7. **Result Return**: Send result back to RTFS

### Execution Policies

CCOS supports different execution policies for capability providers:

- **Marketplace**: All capabilities must go through marketplace providers (production mode)
- **Hybrid**: Safe capabilities can use LocalProvider, risky ones use marketplace
- **InlineDev**: Development mode allowing local provider for all capabilities

### Provider Architecture

Capabilities are executed through pluggable providers:

- **LocalProvider**: Basic host operations for development/bootstrap
- **Marketplace Providers**: Full-featured providers from the capability marketplace
- **Custom Providers**: Organization-specific providers for specialized needs

This architecture ensures **flexibility** while maintaining **security** and **auditability**.

## Resource Management

RTFS provides resource-safe operations through host-mediated resource management:

```clojure
;; Manual resource management via host
(let [file (call :ccos.fs/open "/tmp/data.txt")]
  (try
    (call :ccos.fs/read file)
    (finally
      (call :ccos.fs/close file))))
```

### Automatic Resource Cleanup

```clojure
;; File handles
(call :fs.with-open "/file.txt" :read
  (fn [handle]
    (let [content (read-all handle)]
      (process content))))
;; File automatically closed when continuation completes
```

## Async and Concurrent Operations

Host-mediated concurrency:

```clojure
;; Parallel execution (via step-parallel)
(step-parallel
  (call :ccos.http/get url1)
  (call :ccos.http/get url2)
  (call :ccos.fs/read file))

;; Futures and promises (conceptual)
(let [future (call :ccos.async/compute-heavy-task data)]
  (do-other-work)
  (call :ccos.async/await future))
```

## Error Handling Across Boundary

Structured error propagation:

```clojure
;; Host errors bubble up as RTFS exceptions
(try
  (call :ccos.http/get "invalid-url")
  (catch :network-error e
    (log-error e)
    (fallback-value)))
```

## Testing with Host Boundary

Pure host interface enables testing through capability mocking:

```clojure
;; Test with capability mocking
(with-mock-host {:fs.read (fn [_] "mock content")}
  (let [content (call :fs.read "/test.txt")]
    (assert (= content "mock content"))))
```

## Capability Marketplace

RTFS can discover and use capabilities dynamically:

```clojure
;; Browse marketplace
(let [caps (call :ccos.marketplace/search {:tags ["database" "postgresql"]})]
  (map :id caps))

;; Load and use capability
(let [cap (call :ccos.marketplace/load "custom.analytics/process")]
  (cap data))
```

## Streaming and Reactive Capabilities

Real-time data processing:

```clojure
;; Stream processing
(call :ccos.stream/map
  (fn [event] (process-event event))
  (call :ccos.kafka/consume "user-events"))

;; Reactive programming
(let [updates (call :ccos.state.kv/watch "config.*")]
  (map reconfigure updates))
```

## Host Boundary Benefits

### Security
- All external access is mediated
- Mandatory audit trails
- Governance policy enforcement

### Reliability
- Controlled resource usage
- Timeout and error handling
- Idempotency support

### Performance
- Lazy evaluation across boundary
- Parallel capability execution
- Caching and optimization hints

### Composability
- Pure and effectful code integration
- Capability abstraction
- Dynamic service discovery

## Implementation Architecture

```
RTFS Code → Parser → AST → Evaluator → ExecutionOutcome
                                               ↓
                                      Complete(Value) → Result
                                               ↓
                                    RequiresHost(HostCall)
                                               ↓
                         ┌─────────────────────┐
                         │    CCOS Governance  │
                         │ - Security Check    │
                         │ - Policy Evaluation │
                         │ - Provider Selection│
                         │ - Audit Logging     │
                         └─────────────────────┘
                                               ↓
                         ┌─────────────────────┐
                         │  Capability Provider│
                         │ - LocalProvider     │
                         │ - Marketplace Prov. │
                         │ - Custom Providers  │
                         └─────────────────────┘
                                               ↓
                                    Result → RTFS Continuation
```

### Provider Selection Flow

```
HostCall → Execution Policy → Provider Selection
    ↓
Marketplace Policy → Marketplace Provider
Hybrid Policy → Safe Capability? → LocalProvider : Marketplace Provider  
InlineDev Policy → LocalProvider
    ↓
Provider.execute_capability() → Result
```

This architecture ensures RTFS remains **pure and composable** while enabling **secure, auditable interaction** with the external world through CCOS capabilities and pluggable providers.
