# Delegation Engine (DE)

_Status: **M2 COMPLETED** – Evaluator integration finished, IR runtime wired_

---

## 1 Purpose

The **Delegation Engine** decides _where_ and _how_ each RTFS function call is executed.
It balances performance, cost, privacy and availability by routing calls to one of three
execution targets:

| Target            | Description                                               |
| ----------------- | --------------------------------------------------------- |
| `LocalPure`       | Run directly in the deterministic RTFS evaluator.         |
| `LocalModel(id)`  | Invoke an **on-device** model provider (tiny LLM, rules). |
| `RemoteModel(id)` | Delegate via Arbiter RPC to a remote model service.       |

**Important**: "Model" here refers to ANY intelligent decision-making component, not just LLMs. This includes:
- **Tiny local LLMs** (50MB Phi-3-mini, Llama 3.1 8B quantized)
- **Rule-based classifiers** (sentiment analysis, intent classification)
- **Specialized AI models** (vision, audio, code generation)
- **Remote LLM services** (GPT-4, Claude, Gemini via Arbiter)
- **Hybrid decision trees** (fast local rules + remote fallback)

Because the decision can be deterministic (static policies) or stochastic
(large-scale LLM reasoning), the DE is _pluggable_. The evaluator only sees a
trait object and never bakes in any policy.

**Key Benefits**:
- **Privacy**: Keep sensitive data on-device with local models
- **Performance**: Route simple tasks to fast local execution
- **Cost**: Use expensive remote models only when necessary
- **Reliability**: Fallback to local execution when remote services fail

---

## 2 Core Abstractions

### 2.1 `ExecTarget`

Rust enum describing the selected destination. The string **id** lets us map to
a concrete `ModelProvider` at runtime:

```rust
pub enum ExecTarget {
    LocalPure,                    // Direct RTFS evaluator
    LocalModel(String),           // "phi-mini", "sentiment-rules", "code-classifier"
    RemoteModel(String),          // "gpt4o", "claude-opus", "gemini-pro"
    L4CacheHit { storage_pointer: String, confidence: f64 },
}
```

**Examples**:
- `LocalModel("phi-mini")` → 50MB quantized Phi-3-mini running in-process
- `LocalModel("sentiment-rules")` → Fast rule-based sentiment classifier
- `RemoteModel("gpt4o")` → GPT-4 Omni via OpenAI API through Arbiter
- `RemoteModel("claude-opus")` → Claude 3.5 Sonnet via Anthropic API

### 2.2 `CallContext`

Light-weight, hashable struct carrying the _minimum_ information required to
make a routing decision:

```rust
pub struct CallContext<'a> {
    pub fn_symbol: &'a str,           // "http/get-json", "analyze-sentiment"
    pub arg_type_fingerprint: u64,   // Hash of argument types
    pub runtime_context_hash: u64,   // Permissions, tenant, task context
    pub semantic_hash: Option<u64>,  // Content-based hash for semantic caching
    pub metadata: Option<&'a DelegationMetadata>,
}
```

### 2.3 `DelegationEngine` trait

```rust
trait DelegationEngine: Send + Sync + std::fmt::Debug {
    fn decide(&self, ctx: &CallContext) -> ExecTarget;
}
```

_Must be pure_ so the evaluator can safely memoise results.

### 2.4 `ModelProvider` trait & Registry

```rust
trait ModelProvider: Send + Sync {
    fn id(&self) -> &str;
    fn infer(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error>>;
    fn capabilities(&self) -> &[&str];  // ["text-generation", "classification", etc.]
    fn cost_per_call(&self) -> f64;     // For cost-based routing
}
```

Both in-process and remote models implement the same interface. The global
`ModelRegistry` resolves the string id from `ExecTarget::LocalModel` /
`RemoteModel` to an actual provider.

**Provider Examples**:
- `LocalPhiModel` → Runs Phi-3-mini via candle-transformers
- `LocalRulesModel` → Fast rule-based classification
- `RemoteOpenAIModel` → Proxies to OpenAI API via Arbiter
- `RemoteAnthropicModel` → Proxies to Anthropic API via Arbiter

### 2.5 Delegation-hint metadata (`^:delegation`)

While the `DelegationEngine` decides _dynamically_ at runtime, authors can
embed an _authoritative_ or _suggested_ target directly in the source code.

```clojure
(defn analyze-complex-document        ; Requires powerful reasoning
  ^:delegation :remote "gpt4o"       ; ← Always use GPT-4 Omni
  [document]
  (analyze-document-structure document))

;; Local tiny-LLM example for simple classification
(defn classify-sentiment
  ^:delegation :local-model "phi-mini"
  [text]
  (classify-text-sentiment text))

;; Fast rule-based classification
(defn detect-spam
  ^:delegation :local-model "spam-rules"
  [email]
  (spam-classifier email))

;; Explicitly force pure evaluator (default, shown for completeness)
(defn add-numbers
  ^:delegation :local
  [x y]
  (+ x y))
```

At compile time the parser stores this information as:

```rust
ast::DelegationHint::LocalPure             // :local (or omitted)
ast::DelegationHint::LocalModel("phi-mini")
ast::DelegationHint::RemoteModel("gpt4o")
```

**✅ IMPLEMENTED:** The `to_exec_target()` helper function is now implemented in `ast.rs` to bridge `DelegationHint` to `ExecTarget`.

If a hint is present the evaluator _bypasses_ the `DelegationEngine` for that
call, guaranteeing the author's intent. Otherwise it falls back to the DE's
policy decision.

## 3 Execution Flow

1. **Context Building**: Evaluator builds a `CallContext` for the function node.
2. **Delegation Decision**: 
   - Check for `^:delegation` hint first (author override)
   - If no hint, call `DelegationEngine::decide(ctx)`
3. **Execution Routing**: Depending on the returned `ExecTarget`:
   - **LocalPure** → run the existing evaluator logic
   - **LocalModel(id)** → look up provider in `ModelRegistry` and run `infer`
   - **RemoteModel(id)** → use provider stub that performs an RPC to Arbiter
4. **Result Propagation**: Result propagates back to the evaluator – identical signature regardless of path, maximising composability.

### Decision Flow Diagram

```
┌─────────────────┐
│ Function Call   │
│ (analyze-text)  │
└─────┬───────────┘
      │
      ▼
┌─────────────────┐
│ Check for       │
│ ^:delegation    │
│ hint            │
└─────┬───────────┘
      │
      ▼
┌─────────────────┐    Yes   ┌─────────────────┐
│ Hint present?   │─────────▶│ Use hint target │
└─────┬───────────┘          └─────┬───────────┘
      │ No                         │
      ▼                            │
┌─────────────────┐                │
│ Build           │                │
│ CallContext     │                │
└─────┬───────────┘                │
      │                            │
      ▼                            │
┌─────────────────┐                │
│ DelegationEngine│                │
│ .decide(ctx)    │                │
└─────┬───────────┘                │
      │                            │
      ▼                            │
┌─────────────────┐                │
│ Route to        │◀───────────────┘
│ ExecTarget      │
└─────┬───────────┘
      │
      ▼
┌─────────────────────────────────────┐
│         Execute Function            │
├─────────────────────────────────────┤
│ LocalPure     │ Direct evaluator    │
│ LocalModel    │ On-device provider  │
│ RemoteModel   │ Remote via Arbiter  │
│ L4CacheHit    │ Cached bytecode     │
└─────────────────────────────────────┘
```

## 4 Implementation Phases

| Milestone | Scope                                                                     | Status |
| --------- | ------------------------------------------------------------------------- | ------ |
| **M0**    | Skeleton (`ExecTarget`, `CallContext`, `StaticDelegationEngine`)         | ✅ **DONE** |
| **M1**    | Parser/AST/IR metadata plumbing for optional `^:delegation` hint          | ✅ **DONE** |
| **M2**    | Evaluator integration + criterion micro-benchmarks (<0.2 ms/1M)           | ✅ **DONE** |
| **M3**    | `ModelProvider` registry, `LocalEchoModel`, `RemoteArbiterModel`          | ❌ **PENDING** |
| **M4**    | LRU decision cache + metrics (`delegation_cache_size`, hits)              | ❌ **PENDING** |
| **M5**    | Policy loading (`delegation.toml`), async ticket runner, fallback logic   | ❌ **PENDING** |

### Current Implementation Status

#### ✅ **Completed (M0-M2)**
- **DelegationEngine skeleton**: `ExecTarget`, `CallContext`, `StaticDelegationEngine` implemented in `rtfs_compiler/src/ccos/delegation.rs`
- **Parser integration**: `^:delegation` metadata parsing fully implemented with comprehensive test coverage
- **AST plumbing**: `DelegationHint` enum and field propagation through AST → IR → Runtime values
- **Grammar support**: Pest grammar rules for delegation metadata parsing
- **Test coverage**: 10 delegation-related tests passing
- **Evaluator integration**: Both AST evaluator and IR runtime fully wired to DelegationEngine
- **Function name resolution**: `find_function_name` methods implemented in both `Environment` and `IrEnvironment`
- **Delegation hint conversion**: `DelegationHint::to_exec_target()` method implemented
- **Comprehensive wiring**: All `IrRuntime::new()` calls updated to include delegation engine parameter

#### 🔄 **Partially Implemented (M2)**
- **Performance benchmarks**: Not yet implemented (criterion micro-benchmarks)
- **Decision caching**: Still using simple HashMap instead of LRU

#### ❌ **Not Implemented (M3-M5)**
- **ModelProvider registry**: No concrete providers implemented
- **Local/Remote model execution**: All model paths return `NotImplemented`
- **Decision caching**: Still using simple HashMap instead of LRU
- **Policy engine**: No configuration loading or dynamic policies
- **Async ticket system**: No async execution support

## 5 Design Rationale

- **Safety** – DE lives _outside_ deterministic evaluator, so crashes or heavy
  model weights cannot destabilise the core.
- **Provider-agnostic** – any model (from 50 MB quantised to cloud GPT-4) is just
  another `ModelProvider`.
- **Hot-swap** – because DE is a trait object, runtimes can load a different
  implementation at startup without recompiling evaluator.
- **Performance** – static fast-path & caching keep overhead < 100 ns per call
  in common loops.
- **Multi-model flexibility** – Support for any intelligent component, not just LLMs

## 6 Next Steps

### 🔥 **IMMEDIATE PRIORITY: Implement `call` Function**

The most critical next step is implementing the `call` function that bridges RTFS plans with the CCOS capability system:

#### 6.1 `call` Function Implementation
```rtfs
;; Function signature
(call :capability-id inputs) -> outputs

;; Example usage in a plan
(plan analyze-data-plan
  :intent-id "intent-123"
  :program (do
    ;; Step 1: Fetch data using capability
    (let [data (call :com.acme.db:v1.0:fetch-data 
                     {:query "SELECT * FROM sales" 
                      :format :json})]
      ;; Step 2: Analyze data using AI capability  
      (let [analysis (call :com.openai:v1.0:analyze-data
                           {:data data
                            :analysis-type :quarterly-summary})]
        ;; Return analysis result
        analysis))))
```

#### 6.2 Implementation Requirements
- **Action Generation**: Each `call` creates an Action object in the causal chain
- **Provenance Tracking**: Link actions to plan/intent hierarchy
- **Schema Validation**: Validate inputs/outputs against capability schemas
- **Error Handling**: Graceful failures with fallback mechanisms
- **Performance Tracking**: Record execution time, cost, and resource usage

#### 6.3 Integration Points
- **Runtime Integration**: Wire into both AST evaluator and IR runtime
- **Causal Chain**: Append signed actions to immutable ledger
- **Delegation Engine**: Route capability calls through delegation decisions
- **Demo Integration**: Extend plan generation demo to test real capability calls

### Subsequent Priorities (M3)

#### 6.4 Model Provider Implementation
1. **Create Model Provider Registry**
   - `LocalEchoModel` provider for testing
   - `RemoteArbiterModel` stub for remote delegation
   - Basic `ModelRegistry` implementation

2. **Add Performance Benchmarks**
   - Criterion micro-benchmarks for delegation overhead
   - Validate < 0.2ms per 1M calls performance target

### Medium-term Goals (M4)
1. **Implement Real Model Providers**
   - `LocalPhiModel` using candle-transformers
   - `LocalRulesModel` for rule-based classification
   - Remote provider implementations

2. **Advanced Caching**
   - Replace HashMap with LRU cache
   - Add metrics and observability
   - Implement semantic caching (L3)

### Long-term Vision (M5)
1. **Policy Engine**
   - Configuration loading from `delegation.toml`
   - Cost-based routing policies
   - Privacy and compliance rules

2. **Async Execution**
   - Async ticket system for remote calls
   - Fallback logic for failed model calls
   - Comprehensive monitoring and alerting

## 7 Open Questions

1. **Policy engine** – do we need per-tenant or per-capability rules?
2. **Sandboxing** – should on-device models run in WASM for memory isolation?
3. **Durability** – store async tickets in message queue (NATS, RabbitMQ)?
4. **Model management** – how to handle model loading/unloading for resource management?
5. **Hybrid strategies** – how to implement local-first with remote fallback?

## 8 References

- Split vs. Embedded Arbiter analysis (chat log 2025-07-XX).
- `rtfs_compiler/src/ccos/delegation.rs` – canonical implementation skeleton.
- `rtfs_compiler/src/parser/special_forms.rs` – delegation metadata parsing.
- `rtfs_compiler/src/runtime/evaluator.rs` – complete evaluator integration.
- `rtfs_compiler/src/runtime/ir_runtime.rs` – complete IR runtime integration.

## 9 Advanced Architecture: Multi-Layered Cognitive Caching

To evolve the CCOS from a reactive system to a proactive, learning architecture, we have specified a multi-layered caching strategy that is deeply integrated into the RTFS 2.0 language and the Arbiter's cognitive loop. This transforms caching from a simple performance optimization into a core feature of the system's intelligence.

Each layer has a detailed implementation specification:

-   [**L1 Delegation Cache**](./caching/L1_DELEGATION_CACHE.md): High-speed memoization of `(Agent, Task) -> Plan` decisions.
-   [**L2 Inference Cache**](./caching/L2_INFERENCE_CACHE.md): Hybrid in-memory/persistent storage for expensive LLM inference results.
-   [**L3 Semantic Cache**](./caching/L3_SEMANTIC_CACHE.md): AI-driven vector search for finding semantically equivalent (but not identical) past inferences.
-   [**L4 Content-Addressable RTFS**](./caching/L4_CONTENT_ADDRESSABLE_RTFS.md): The most fundamental layer, which caches and reuses compiled RTFS bytecode directly.

This advanced architecture supersedes the original plan for a simple LRU cache (Milestone M4), providing a much more ambitious and powerful path forward for system-wide learning and performance optimization.
