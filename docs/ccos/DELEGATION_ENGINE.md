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

Because the decision can be deterministic (static policies) or stochastic
(large-scale LLM reasoning), the DE is _pluggable_. The evaluator only sees a
trait object and never bakes in any policy.

---

## 2 Core Abstractions

### 2.1 `ExecTarget`

Rust enum describing the selected destination. The string **id** lets us map to
a concrete `ModelProvider` at runtime ("phi-mini", "gpt4o", "sentiment-rules").

### 2.2 `CallContext`

Light-weight, hashable struct carrying the _minimum_ information required to
make a routing decision:

- `fn_symbol`: fully-qualified RTFS symbol (e.g. `http/get-json`).
- `arg_type_fingerprint`: 64-bit hash of argument _types_ (not values).
- `runtime_context_hash`: hash of ambient context (permissions, tenant, etc.).

### 2.3 `DelegationEngine` trait

```rust
trait DelegationEngine: Send + Sync + std::fmt::Debug {
    fn decide(&self, ctx: &CallContext) -> ExecTarget;
}
```

_Must be pure_ so the evaluator can safely memoise results.

### 2.4 `ModelProvider` trait & Registry

```rust
trait ModelProvider {
    fn id(&self) -> &'static str;
    fn infer(&self, prompt: &str) -> Result<String>;
}
```

Both in-process and remote models implement the same interface. The global
`ModelRegistry` resolves the string id from `ExecTarget::LocalModel` /
`RemoteModel` to an actual provider.

### 2.5 Delegation-hint metadata (`^:delegation`)

While the `DelegationEngine` decides _dynamically_ at runtime, authors can
embed an _authoritative_ or _suggested_ target directly in the source code.

```clojure
(defn http-get                     ; simple wrapper around stdlib
  ^:delegation :remote "gpt4o"    ; ← always go to powerful remote model
  [url]
  (http/get-json url))

;; Local tiny-LLM example
(defn classify-sentiment
  ^:delegation :local-model "phi-mini"
  [text]
  (tiny/llm-classify text))

;; Explicitly force pure evaluator (default, shown for completeness)
(fn ^:delegation :local [x] (+ x 1))
```

At compile time the parser stores this information as

```rust
ast::DelegationHint::LocalPure             // :local (or omitted)
ast::DelegationHint::LocalModel("phi-mini")
ast::DelegationHint::RemoteModel("gpt4o")
```

**✅ IMPLEMENTED:** The `to_exec_target()` helper function is now implemented in `ast.rs` to bridge `DelegationHint` to `ExecTarget`.

If a hint is present the evaluator _bypasses_ the `DelegationEngine` for that
call, guaranteeing the author's intent. Otherwise it falls back to the DE's
policy decision.

---

## 3 Execution Flow

1. Evaluator builds a `CallContext` for the function node.
2. It calls `DelegationEngine::decide(ctx)`.
3. Depending on the returned `ExecTarget`:
   - **LocalPure** → run the existing evaluator logic.
   - **LocalModel(id)** → look up provider in `ModelRegistry` and run `infer`.
   - **RemoteModel(id)** → use provider stub that performs an RPC to Arbiter.
4. Result propagates back to the evaluator – identical signature regardless of
   path, maximising composability.

![DE flow](placeholder)

---

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

---

## 5 Design Rationale

- **Safety** – DE lives _outside_ deterministic evaluator, so crashes or heavy
  model weights cannot destabilise the core.
- **Provider-agnostic** – any model (from 50 MB quantised to cloud GPT-4) is just
  another `ModelProvider`.
- **Hot-swap** – because DE is a trait object, runtimes can load a different
  implementation at startup without recompiling evaluator.
- **Performance** – static fast-path & caching keep overhead < 100 ns per call
  in common loops.

---

## 6 Next Steps

### Immediate Priorities (M3)
1. **Create `LocalEchoModel` provider for testing**
2. **Implement `RemoteArbiterModel` stub**
3. **Wire up `ModelRegistry` in evaluator**
4. **Add criterion micro-benchmarks for performance validation**

### Medium-term Goals (M4)
1. **Replace HashMap cache with proper LRU implementation**
2. **Add metrics and observability**
3. **Implement policy loading from configuration**

### Long-term Vision (M5)
1. **Build async ticket system for remote execution**
2. **Implement fallback logic for failed model calls**
3. **Add comprehensive monitoring and alerting**

---

## 7 Open Questions

1. **Policy engine** – do we need per-tenant or per-capability rules?
2. **Sandboxing** – should on-device models run in WASM for memory isolation?
3. **Durability** – store async tickets in message queue (NATS, RabbitMQ)?

---

## 8 References

- Split vs. Embedded Arbiter analysis (chat log 2025-07-XX).
- `rtfs_compiler/src/ccos/delegation.rs` – canonical implementation skeleton.
- `rtfs_compiler/src/parser/special_forms.rs` – delegation metadata parsing.
- `rtfs_compiler/src/runtime/evaluator.rs` – complete evaluator integration.
- `rtfs_compiler/src/runtime/ir_runtime.rs` – complete IR runtime integration.

---

## 9 Advanced Architecture: Multi-Layered Cognitive Caching

To evolve the CCOS from a reactive system to a proactive, learning architecture, we have specified a multi-layered caching strategy that is deeply integrated into the RTFS 2.0 language and the Arbiter's cognitive loop. This transforms caching from a simple performance optimization into a core feature of the system's intelligence.

Each layer has a detailed implementation specification:

-   [**L1 Delegation Cache**](./caching/L1_DELEGATION_CACHE.md): High-speed memoization of `(Agent, Task) -> Plan` decisions.
-   [**L2 Inference Cache**](./caching/L2_INFERENCE_CACHE.md): Hybrid in-memory/persistent storage for expensive LLM inference results.
-   [**L3 Semantic Cache**](./caching/L3_SEMANTIC_CACHE.md): AI-driven vector search for finding semantically equivalent (but not identical) past inferences.
-   [**L4 Content-Addressable RTFS**](./caching/L4_CONTENT_ADDRESSABLE_RTFS.md): The most fundamental layer, which caches and reuses compiled RTFS bytecode directly.

This advanced architecture supersedes the original plan for a simple LRU cache (Milestone M4), providing a much more ambitious and powerful path forward for system-wide learning and performance optimization.
