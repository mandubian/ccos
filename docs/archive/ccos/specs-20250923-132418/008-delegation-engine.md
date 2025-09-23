# CCOS Specification 008: Delegation Engine

- **Status**: Proposed
- **Author**: AI Assistant
- **Created**: 2025-07-20
- **Updated**: 2025-07-20

## 1. Abstract

This specification defines the **Delegation Engine (DE)**, a core component of the CCOS Orchestration Layer and the RTFS runtime. The DE is a pluggable, policy-driven routing mechanism that determines where and how any **delegatable RTFS function call** is executed. It works in concert with the Global Function Mesh (GFM) to balance performance, cost, privacy, and reliability.

## 2. Motivation

A key feature of CCOS is its ability to run on diverse hardware, from resource-constrained edge devices to powerful cloud servers. A one-size-fits-all execution model is not viable. The DE provides the necessary intelligence to dynamically adapt execution strategy to the current context, enabling:

- **Privacy**: Processing sensitive data on-device using local models.
- **Performance**: Routing simple, low-latency tasks to a local evaluator.
- **Power**: Leveraging large-scale remote models for complex reasoning tasks.
- **Cost-Efficiency**: Using expensive remote models only when necessary.
- **Resilience**: Falling back to local or alternative providers when a primary provider fails.

## 3. Core Concepts

### 3.1. The DE and GFM Partnership

The Delegation Engine and the Global Function Mesh are distinct but complementary components that govern the execution of any function call within the RTFS runtime.

- **Global Function Mesh (GFM)**: The GFM is the **discovery and resolution layer** for *external capabilities*. It maintains a registry of all available capability providers and answers the question: "_What providers can fulfill this capability ID?_" It is primarily concerned with the `(call ...)` primitive.
- **Delegation Engine (DE)**: The DE is the **selection and policy layer** for *any RTFS function*. It answers the question: "_Given the current context and policies, where should this function execute?_" This applies to standard RTFS functions `(my-func ...)` as well as capability calls `(call ...)`

The interaction is as follows:
1. For a standard function `(my-func arg1)`, the RTFS runtime first asks the DE where to execute it. The DE might decide `LocalPure`, `LocalModel`, or `RemoteModel`.
2. For a capability call `(call :my-cap arg1)`, the runtime *still* asks the DE first. If the DE chooses a target like `RemoteModel("some-arbiter")`, that remote arbiter will then use its own GFM to resolve the final provider.

```mermaid
graph TD
    subgraph RTFS Runtime
        A[1. Encounter any function call <br/> e.g., (my-func) or (call :cap-id)] --> B{2. Invoke DE};
        B -- "Function Symbol + Context" --> C[3. DE applies policy <br/> (e.g., privacy-first)];
        C -- "Decision: ExecTarget" --> D[4. Execute via selected target];
    end

    subgraph "Example Target: RemoteModel"
        D --> E{5. RPC to Remote Arbiter};
        E --> F{6. Arbiter invokes GFM};
        F --> G[7. GFM finds provider];
        G --> H[8. Remote Execution];
    end

    style B fill:#cde4ff,stroke:#333,stroke-width:2px
```

### 3.2. Execution Targets

The DE's decision materializes as an `ExecTarget` enum, which instructs the Orchestrator on how to proceed.

```rust
pub enum ExecTarget {
    /// Execute in the local, deterministic RTFS runtime.
    /// This may involve interpreting the function's AST/IR directly for
    /// maximum portability, or executing an optimized, JIT-compiled
    /// version of the function that has been cached.
    LocalPure,
    /// Execute using a registered on-device model provider (e.g., tiny LLM, rules engine).
    LocalModel(String), // e.g., "phi-mini", "sentiment-rules"
    /// Delegate to a remote model service via the Arbiter's RPC mechanism.
    RemoteModel(String), // e.g., "gpt4o", "claude-opus"
    /// Use a result from a cache.
    CacheHit { cache_level: u8, storage_pointer: String },
}
```

### 3.3. Pluggable Policies

The core of the DE is the `DelegationEngine` trait. This allows different policies to be loaded at runtime without altering the Orchestrator.

```rust
// Represents all information needed to make a routing decision.
pub struct CallContext<'a> {
    pub function_symbol: &'a str, // "my-func", "call", etc.
    pub capability_id: Option<&'a str>, // Only for `(call ...)`
    pub candidate_providers: Vec<ProviderInfo>, // From GFM, if applicable
    pub user_context: &'a UserContext, // Includes permissions, tenancy
    pub device_context: &'a DeviceContext, // Includes network status, battery
    // ... and other relevant metadata
}

// The pluggable policy engine.
trait DelegationEngine: Send + Sync {
    fn decide(&self, ctx: &CallContext) -> ExecTarget;
}
```

Example implementations could include:
- `PrivacyFirstEngine`: Prefers `LocalPure` and `LocalModel` targets.
- `CostOptimizedEngine`: Avoids `RemoteModel` targets that have a high cost metric.
- `PerformanceEngine`: Prefers providers with the lowest latency.
- `StaticEngine`: Implements a fixed routing table from a configuration file.

### 3.4. Developer-Defined Hints

To ensure deterministic behavior when required, developers can embed delegation hints directly in RTFS source code.

```clojure
;; This function MUST run remotely on a powerful model.
(defn analyze-complex-image
  ^:delegation :remote "gpt4o"
  [image-data]
  (call :com.acme.vision:v1:analyze image-data))
```

If a valid `^:delegation` hint is present, the Orchestrator **bypasses** the Delegation Engine entirely, guaranteeing the developer's intent is respected.

## 4. Caching Architecture

The DE is also responsible for managing a multi-layered caching strategy to minimize redundant computation and network traffic.

- **L1 (Delegation Cache)**: High-speed memoization of DE decisions. Maps `hash(CallContext) -> ExecTarget`.
- **L2 (Inference Cache)**: Caches the actual results of expensive model inferences. Maps `hash(CallContext) -> Result`.
- **L3 (Semantic Cache)**: A vector-based cache that finds semantically similar, previously computed results.
- **L4 (Content-Addressable JIT Cache)**: Caches compiled, native machine code for frequently executed, pure RTFS functions. This is the core of the JIT compilation strategy.

The DE first checks these caches before invoking the GFM or making a policy decision. When a pure function is called frequently, the DE can decide to trigger a JIT compilation process, storing the resulting machine code in the L4 cache. Subsequent calls can then bypass interpretation entirely for near-native performance.


## 5. Integration with the Causal Chain

Every delegated function call and its outcome must be recorded in the Causal Chain for auditability and observability. The `Action` in the chain will be augmented to include this information, whether it's a standard function or a capability call.

```rust
// From 003-causal-chain.md
pub struct Action {
    // ... existing fields
    pub details: ActionType,
}

pub enum ActionType {
    // ... other types
    FunctionCall {
        function_symbol: String,
        // ... input/output
        delegation_info: DelegationInfo,
    },
    CapabilityCall {
        capability_id: String,
        // ... input/output
        delegation_info: DelegationInfo,
    }
}

pub struct DelegationInfo {
    /// The list of candidates considered by the DE.
    providers_considered: Vec<String>,
    /// The policy engine that made the decision.
    decision_engine: String, // e.g., "PrivacyFirstEngine"
    /// The final target chosen by the DE.
    chosen_target: ExecTarget,
    /// Whether the decision was overridden by a developer hint.
    is_override: bool,
}
```

This ensures that every action is transparent and the reasoning behind the execution path is explicitly recorded.
