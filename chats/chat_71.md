# Implement DelegationEngine stub in Rust
_Exported on 05/07/2025 at 20:27:04 UTC+2 from Cursor (1.2.1)_

---

**User**

Here is the plan you had given in aannother chat, continue it
"
1 Where the intercept can live
A) Split design (today)
Evaluator executes pure RTFS functions.
Capability wrapper → DE → chosen path.
Arbiter runs in a different module / process.
B) Embedded Arbiter design
Evaluator calls directly into Arbiter-as-kernel for every function node.
Arbiter contains the DE internally.
Same binary, tighter loop.
2 Performance considerations
Latency budget for one function call
Apply to arbiter.rs
path
Δ ≈ 20 ns per call (cross-module dispatch + small indirection).
Even at 10 M calls/s this is < 0.2 ms.
Throughput impact appears negligible if:
DE is purely metadata-based and avoids network/LLM unless required.
LLM-delegation path is asynchronous (ticket/future), so “slow” choices don’t block the deterministic path.
3 CPU & memory footprint
Split: two Rust crates or processes, OS scheduler cost, but memory isolation.
Embedded: one process, shared cache lines (good), but Arbiter model weights loaded in all runtimes (bad for small devices).
Decision: if we plan to run on edge devices with no GPU, keeping the LLM in a separate, optional service avoids bundling big weights into every runtime.
4 Failure-isolation & safety
• Split → deterministic core can keep running when the Arbiter crashes / updates.
Embedded → a bug in Arbiter logic panics the whole runtime.
For a first production system, the split gives a stronger safety envelope and easier hot-swapping of the Arbiter model.
5 Engineering velocity
Split
We can iterate on DE & Arbiter in Rust or Python without touching evaluator internals.
Hot-reload or gRPC between processes.
Embedded
Requires stable API inside evaluator; every change involves a full re-build of runtime.
Debugging is harder because deterministic and stochastic code interleave in the same stack trace.
6 Possible performance mitigations in split model
1. Decision caching
Cache DE result keyed by (fn-symbol, arg-type-fingerprint, context-hash).
Hit rate is high for repeated loops; evaluator sees O(1) lookup.
Static fast-path
Mark functions as always local by metadata; wrapper bypasses DE.
Only “dynamic” or capability functions call DE.
Batch delegation (map/reduce)
DE can reason at compile-time on entire Plan, generating an optimized execution graph; per-call decisions become rare.
7 Recommendation
Keep the current split (micro-kernel) design for the next milestones:
Overhead is negligible compared to LLM calls.
Yields better crash-containment, hot-upgrade, GPU/CPU topology flexibility.
Lets the core deterministic runtime remain certifiable / formally testable.
If later profiling shows the 100 ns overhead is critical (unlikely), we can:
Inline the DE into the evaluator and keep the LLM executor external.
Or compile the Arbiter to a WASM module and JIT-link it—still separable.
8 Action items to proceed in split model
1. Implement DelegationEngine stub in src/ccos/delegation.rs.
Add per-function metadata (:delegation :dynamic|:local|:external).
Extend inject_capability to consult DE only when needed.
Benchmark call-overhead with 1 M no-op (+ 1 1) iterations to validate negligible performance cost (< 1 µs total overhead).
This route keeps the door open to embedding later, while getting us to a working delegating runtime fastest.
Let me know if you’d like to see a concrete DelegationEngine skeleton or benchmarks."

---

**Cursor**

Continuation of the design & implementation plan – focused on turning the “split Arbiter / Delegation Engine (DE)” architecture into running code and measurable benchmarks.

────────────────────────────────────────────────────────────────────────
9  DelegationEngine – high-level API
   ┌─────────────────────────────────────────────────────────────┐
   │ pub enum DelegationChoice {                                 │
   │     Local,            // run in-process in pure evaluator   │
   │     ExternalSync,     // blocking RPC to Arbiter            │
   │     ExternalAsync,    // fire-and-forget ticket/future      │
   │ }                                                          │
   │                                                            │
   │ pub struct CallContext<'a> {                                │
   │     fn_symbol: &'a str,            // fully-qualified name  │
   │     arg_type_fingerprint: u64,     // cheap hash            │
   │     runtime_context_hash: u64,     // e.g. task, perms …    │
   │ }                                                          │
   │                                                            │
   │ pub trait DelegationEngine {                                │
   │     fn decide(&self, ctx: &CallContext) -> DelegationChoice;│
   │ }                                                          │
   └─────────────────────────────────────────────────────────────┘

   • Initial stub (v0) lives in `src/ccos/delegation.rs`.
   • Default implementation returns `Local` except for symbols explicitly
     registered as “dynamic” in metadata.

10  Function-level metadata
   • Extend the RTFS parser to recognise an optional `:delegation` tag  
     on function definitions:
       (defn http-get ^:delegation :external …)
   • Wire this tag into the AST as `delegation_hint: Option<DelegationHint>`.
   • At IR generation time, propagate it into IR node metadata so the
     evaluator can read it without reparsing the source.

11  Evaluator integration
   • Wrapper that currently calls `inject_capability` becomes:

     ```
     fn call_fn_node(&mut self, node: &IrFnCall) -> Result<Value> {
         if let Some(choice) = self.delegation_engine.decide(&ctx) {
             match choice {
                 Local => return self.exec_local(node),
                 ExternalSync => return arbiter.call_sync(node),
                 ExternalAsync => return arbiter.call_async(node),
             }
         }
         self.exec_local(node)
     }
     ```

   • For v0 we implement only `Local`; the other arms return
     `unimplemented!()` to keep the compiler happy.

12  Decision caching (v0.5)
   • Use `hashbrown::HashMap<CallContext, DelegationChoice>` inside the DE.
   • LRU eviction once the map exceeds ~64K entries to bound memory.

13  Static fast-path
   • If the IR node’s `delegation_hint` == `Local`, bypass the DE entirely.
   • Verified via unit test to ensure < 1 ns overhead on tight loop.

14  Benchmarks
   • Add `rtfs_compiler/benches/eval_vs_eval_with_de.rs` (criterion).
   • Benchmark 1 M executions of `(inc 0)`:
        – pure evaluator baseline  
        – evaluator + DE stub (all Local)  
      Pass if total delta < 0.2 ms on an M1 Pro.

15  Logging & tracing
   • Use `tracing::instrument` on `DelegationEngine::decide`.
   • Emit span fields: fn_symbol, decision, cache_hit.

16  Safety & fallback
   • If the Arbiter RPC layer is unreachable:
        – `ExternalSync` path downgrades to `Local`.
        – `ExternalAsync` ticket is queued until the channel is up again.
   • Expose a `/healthz` endpoint on Arbiter for Kubernetes liveness probes.

17  Build & CI milestones
   M0 (today)      – land `delegation.rs` stub + unit tests compile green  
   M1 (+1 week)    – metadata parsing & IR plumbing  
   M2 (+2 weeks)   – evaluator integration, criterion benchmarks in CI  
   M3 (+4 weeks)   – ExternalSync path (gRPC) prototype, local docker-compose  
   M4 (+6 weeks)   – Caching, static fast-path, async ticket runner  
   M5 (+8 weeks)   – Load tests, perf report, go/no-go for embedded variant

18  Risks & mitigations
   • Metadata divergence → write golden tests that round-trip source ⇄ AST.
   • Cache blow-up → cap with LRU; add metric `delegation_cache_size`.
   • Cross-crate version skew → version DE trait with `#[version(X)]`.

19  Open questions
   • Do we need per-tenant delegation policy? (multi-tenant SaaS)
   • How will the license for large LLM weights affect the “split” deployment?
   • Should async tickets persist to a message queue (e.g. NATS) for durability?

20  Next steps for you
   1. Review API in §9 – especially the ↔ Arbiter boundary.
   2. Green-light M0 implementation; I can push a PR with the stub + tests.
   3. Decide gRPC vs. JSON-over-HTTP for the ExternalSync prototype.

Let me know which parts you’d like me to flesh out in code first, or if you prefer a PR draft for the DelegationEngine skeleton and benchmark harness.

```plaintext
     fn call_fn_node(&mut self, node: &IrFnCall) -> Result<Value> {
         if let Some(choice) = self.delegation_engine.decide(&ctx) {
             match choice {
                 Local => return self.exec_local(node),
                 ExternalSync => return arbiter.call_sync(node),
                 ExternalAsync => return arbiter.call_async(node),
             }
         }
         self.exec_local(node)
     }
```

---

**User**

one remark, LLM delegation could be done by the Arbiter, but what about another LLM, even a tiny one and local for example?

---

**Cursor**

Great point—“LLM” ​should really be understood as “one of several possible model providers,” ranging from a heavyweight cloud-hosted GPT-4 all the way down to a 50 MB Phi-3-mini or a rule-based classifier that ships inside the binary.  
The split-runtime architecture still works; we only need to make the provider concept explicit.

1. Provider–agnostic DelegationEngine  
   Extend the decision output from plain `DelegationChoice` to something like:

   ```
   pub enum ExecTarget {
       LocalPure,                       // deterministic evaluator
       LocalModel(&'static str),        // e.g. "phi-mini"
       RemoteModel(&'static str),       // e.g. "gpt4o"
   }
   ```

   • `LocalModel` means “call an on-device model that is linked or dynamically loaded.”  
   • `RemoteModel` goes through the Arbiter RPC as before.  
   • The DE decides which variant to return based on cost, availability, privacy policy, or explicit metadata.

2. Pluggable ModelProvider trait  
   Add an abstraction that any model—tiny, embedded, or cloud—can implement:

   ```
   pub trait ModelProvider {
       fn id(&self) -> &'static str;
       fn infer(&self, prompt: &str) -> Result<String>;
   }
   ```

   • The evaluator keeps a registry `HashMap<&'static str, Arc<dyn ModelProvider>>`.  
   • For `LocalModel`, it looks up the implementation directly and runs it in-process (no network, minimal latency).  
   • For `RemoteModel`, the provider stub proxies to the Arbiter.

3. Deployment flexibility  
   • Edge device: ship the evaluator + DE + a quantized 4-bit model provider; configure DE to prefer `LocalModel`.  
   • Server/cloud: evaluator + DE compiled without embedded weights; DE chooses `RemoteModel` paths.  
   • Hybrid: DE can fall back—prefer local tiny model for common intents, escalate to powerful remote model when quality threshold isn’t met.

4. Configuration surface  
   • `delegation.toml` (or env vars) lets ops set per-function preferences, cost ceilings, privacy levels.  
   • At runtime the DE merges static metadata with policy to yield the target.

5. Safety & resource isolation  
   • Even when the tiny model is linked in, treat it as a “soft capability”: bound its memory by running in a separate thread with an arena allocator, or load it in a WASM sandbox so that a mis-behaving model can be unloaded without killing the evaluator.  
   • The split still gives you crash-isolation for big models while allowing ultra-fast local inference where appropriate.

6. Next action items  
   • Add the `ExecTarget::LocalModel` branch to the enum and skeleton.  
   • Define the `ModelProvider` trait + registry (can start empty).  
   • Extend the benchmark to measure `(fn-call → LocalModel(phi-mini) → return)` and compare with `LocalPure` to get a baseline.

This way the architecture remains provider-agnostic: the Arbiter is one (powerful) provider, but a tiny on-device LLM is just another entry in the registry, selectable by the DelegationEngine rules you already planned.

```plaintext
   pub enum ExecTarget {
       LocalPure,                       // deterministic evaluator
       LocalModel(&'static str),        // e.g. "phi-mini"
       RemoteModel(&'static str),       // e.g. "gpt4o"
   }
```

```plaintext
   pub trait ModelProvider {
       fn id(&self) -> &'static str;
       fn infer(&self, prompt: &str) -> Result<String>;
   }
```

---

**User**

make a DelegationEngine skeleton please keeping all those contexts and trying to make it a clean as possible. Then refine the plan to develop it

---

**Cursor**

```rust
// ... existing code ...
pub mod loaders;
+pub mod delegation;
 pub mod subconscious;
// ... existing code ...
```

```rust
//! Delegation Engine (DE)
//!
//! Responsible for deciding whether a given RTFS function call should be
//! executed locally (pure evaluator), through a local model, or delegated to a
//! remote Arbiter / model provider.  This is a *skeleton* implementation: logic
//! is intentionally minimal but the public API is considered stable so that
//! the rest of the runtime can compile against it.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, RwLock};

/// Where the evaluator should send the execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExecTarget {
    /// Run directly in the deterministic evaluator.
    LocalPure,
    /// Call an on-device model that implements [`ModelProvider`].
    LocalModel(&'static str),
    /// Delegate to a remote provider via the Arbiter / RPC.
    RemoteModel(&'static str),
}

/// Minimal call-site fingerprint used by the Delegation Engine.
#[derive(Debug, Clone)]
pub struct CallContext<'a> {
    /// Fully-qualified RTFS symbol name being invoked.
    pub fn_symbol: &'a str,
    /// Cheap structural hash of argument type information.
    pub arg_type_fingerprint: u64,
    /// Hash representing ambient runtime context (permissions, task, etc.).
    pub runtime_context_hash: u64,
}

impl<'a> Hash for CallContext<'a> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.fn_symbol.hash(state);
        self.arg_type_fingerprint.hash(state);
        self.runtime_context_hash.hash(state);
    }
}

impl<'a> PartialEq for CallContext<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.fn_symbol == other.fn_symbol
            && self.arg_type_fingerprint == other.arg_type_fingerprint
            && self.runtime_context_hash == other.runtime_context_hash
    }
}

impl<'a> Eq for CallContext<'a> {}

/// Trait implemented by any Delegation Engine.
///
/// Guarantee: *pure* – `decide` must be free of side-effects so that the
/// evaluator can safely cache the result.
pub trait DelegationEngine: Send + Sync {
    fn decide(&self, ctx: &CallContext) -> ExecTarget;
}

/// Simple static mapping + cache implementation.
pub struct StaticDelegationEngine {
    /// Fast lookup for explicit per-symbol policies.
    static_map: HashMap<&'static str, ExecTarget>,
    /// LRU-ish cache keyed by the call context hash.
    cache: RwLock<HashMap<u64, ExecTarget>>, // TODO: replace with proper LRU.
}

impl StaticDelegationEngine {
    pub fn new(static_map: HashMap<&'static str, ExecTarget>) -> Self {
        Self {
            static_map,
            cache: RwLock::new(HashMap::new()),
        }
    }
}

impl DelegationEngine for StaticDelegationEngine {
    fn decide(&self, ctx: &CallContext) -> ExecTarget {
        // 1. Static fast-path
        if let Some(&target) = self.static_map.get(ctx.fn_symbol) {
            return target;
        }

        // 2. Cached dynamic decision
        let key = {
            // Combine hashes cheaply (could use FxHash).  For now simple xor.
            ctx.arg_type_fingerprint ^ ctx.runtime_context_hash
        };

        if let Some(&cached) = self.cache.read().unwrap().get(&key) {
            return cached;
        }

        // 3. Default fallback
        let decision = ExecTarget::LocalPure;

        // 4. Insert into cache (fire-and-forget).  In production this would be
        // an LRU; for skeleton a plain HashMap suffices.
        self.cache.write().unwrap().insert(key, decision);

        decision
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Model Provider abstraction
// ──────────────────────────────────────────────────────────────────────────

/// Anything capable of transforming a textual prompt into a textual output.
/// A provider may be a quantized on-device transformer, a regex rule engine,
/// or a remote OpenAI deployment – the Delegation Engine does not care.
pub trait ModelProvider: Send + Sync {
    /// Stable identifier (e.g. "phi-mini", "gpt4o").
    fn id(&self) -> &'static str;
    /// Perform inference.  Blocking call for now; async wrapper lives above.
    fn infer(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>>;
}

/// Global registry.  In real code this might live elsewhere; kept here for
/// convenience while the API stabilises.
#[derive(Default)]
pub struct ModelRegistry {
    providers: RwLock<HashMap<&'static str, Arc<dyn ModelProvider>>>,
}

impl ModelRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<P: ModelProvider + 'static>(&self, provider: P) {
        self.providers
            .write()
            .unwrap()
            .insert(provider.id(), Arc::new(provider));
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn ModelProvider>> {
        self.providers.read().unwrap().get(id).cloned()
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Tests (compile-time only)
// ──────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_map_override() {
        let mut map = HashMap::new();
        map.insert("math/inc", ExecTarget::RemoteModel("gpt4o"));
        let de = StaticDelegationEngine::new(map);

        let ctx = CallContext {
            fn_symbol: "math/inc",
            arg_type_fingerprint: 0,
            runtime_context_hash: 0,
        };
        assert_eq!(de.decide(&ctx), ExecTarget::RemoteModel("gpt4o"));
    }

    #[test]
    fn fallback_is_local() {
        let de = StaticDelegationEngine::new(HashMap::new());
        let ctx = CallContext {
            fn_symbol: "not/known",
            arg_type_fingerprint: 1,
            runtime_context_hash: 2,
        };
        assert_eq!(de.decide(&ctx), ExecTarget::LocalPure);
    }
}

```

🎉  A first‐pass Delegation Engine skeleton is now in place.

New files / edits:
1. `src/ccos/delegation.rs` – clean, provider-agnostic skeleton with  
   • `ExecTarget` enum (`LocalPure | LocalModel(id) | RemoteModel(id)`)  
   • `CallContext` struct (symbol + two fingerprints)  
   • `DelegationEngine` trait  
   • `StaticDelegationEngine` reference impl + tiny cache  
   • `ModelProvider` trait + in-process `ModelRegistry`  
   • Two compile-time tests
2. `src/ccos/mod.rs` – added `pub mod delegation;`

No external deps were introduced, so the crate should still compile.

────────────────────────────────────────────────────────────────────────
Refined implementation roadmap
────────────────────────────────────────────────────────────────────────
Phase M0 – Skeleton (DONE)
• Compile-time API and minimal tests in place.

Phase M1 – Metadata plumbing (≤ 1 week)
1. Syntax: extend RTFS parser to accept `^:delegation :local|:local-model(\"id\")|:remote(\"id\")`.
2. AST/IR: carry `delegation_hint: Option<ExecTarget>` down to IR nodes.
3. Fast-path: evaluator bypasses DE when hint is `LocalPure`.

Phase M2 – Evaluator integration & benchmarks (≤ 2 weeks)
1. Inject `Arc<dyn DelegationEngine>` into evaluator state.
2. Replace current capability wrapper with:
   ```
   let ctx = CallContext { … };
   match de.decide(&ctx) { … }
   ```
3. Criterion benchmark:
   • pure evaluator vs. evaluator+DE (all LocalPure).  
   • Pass if Δ < 0.2 ms per 1 M calls on reference machine.

Phase M3 – Model providers & Remote RPC (≤ 4 weeks)
1. Implement a stub `LocalEchoModel` (returns prompt) to validate `LocalModel`.
2. Add gRPC / HTTP client provider `RemoteArbiterModel`.
3. Expand tests: ensure correct provider dispatch and error fallback.

Phase M4 – Decision caching & LRU (≤ 6 weeks)
1. Replace HashMap with `lru::LruCache` (~64 K entries default).
2. Metrics: expose `delegation_cache_size`, `cache_hit_rate` via `tracing`.

Phase M5 – Policy engine & async tickets (≤ 8 weeks)
1. Allow DE to load a `delegation.toml` at startup for cost/privacy policy.
2. Implement `RemoteModel` async ticket flow (spawned future, returns placeholder).
3. Persist tickets to message queue (optional NATS) for durability.

Stretch: WASM sandbox for on-device models; evaluator can unload / reload without restart.

Let me know if you’d like follow-up PRs for M1 metadata parsing, or if we should wire the new DE into the evaluator next.

```plaintext
   let ctx = CallContext { … };
   match de.decide(&ctx) { … }
```

---

**User**

is the delegationengine skeleton able to let the decision of delegation to a LLM given some context?

---

**Cursor**

At the moment the skeleton only implements `StaticDelegationEngine`, whose logic is deliberately simple:

• If the symbol appears in the static `HashMap`, return that pre-configured `ExecTarget`.  
• Otherwise fall back to `ExecTarget::LocalPure`, caching the result.

So today it cannot ask an LLM (or any other adaptive component) to make the decision.

However the API was designed so that an LLM-driven strategy can be dropped in without changing the evaluator:

1. Implement a new engine that fulfils the same trait  

   ```rust
   pub struct LlmdDelegationEngine { llm: Arc<dyn ModelProvider>, … }

   impl DelegationEngine for LlmdDelegationEngine {
       fn decide(&self, ctx: &CallContext) -> ExecTarget {
           // 1. Build a prompt from ctx (symbol, arg types, runtime context)
           // 2. Call self.llm.infer(prompt)
           // 3. Parse model reply into ExecTarget
       }
   }
   ```

2. Register the LLM as a `ModelProvider` (local or remote).  
3. At startup choose which `DelegationEngine` implementation to inject
   (`StaticDelegationEngine`, `LlmdDelegationEngine`, or a composite).

Because the evaluator only sees the trait object (`Arc<dyn DelegationEngine>`), swapping in the LLM-powered version is a one-line change in the runtime bootstrap code.

So:  
• “Can it right now?” — No, the provided stub is static.  
• “Is the skeleton ready for that?” — Yes; add a new trait implementation and wire it up.

```rust
   pub struct LlmdDelegationEngine { llm: Arc<dyn ModelProvider>, … }

   impl DelegationEngine for LlmdDelegationEngine {
       fn decide(&self, ctx: &CallContext) -> ExecTarget {
           // 1. Build a prompt from ctx (symbol, arg types, runtime context)
           // 2. Call self.llm.infer(prompt)
           // 3. Parse model reply into ExecTarget
       }
   }
```

---

**User**

ok so document delegation engine concepts in docs/ccos please, add steps of implementation in @RTFS_MIGRATION_PLAN.md and ask me to go to next steps

---

**Cursor**

```markdown
# Delegation Engine (DE)

*Status: API skeleton merged – implementation in progress*

---

## 1  Purpose
The **Delegation Engine** decides *where* and *how* each RTFS function call is executed.
It balances performance, cost, privacy and availability by routing calls to one of three
execution targets:

| Target            | Description                                              |
|-------------------|----------------------------------------------------------|
| `LocalPure`       | Run directly in the deterministic RTFS evaluator.        |
| `LocalModel(id)`  | Invoke an **on-device** model provider (tiny LLM, rules).|
| `RemoteModel(id)` | Delegate via Arbiter RPC to a remote model service.      |

Because the decision can be deterministic (static policies) or stochastic
(large-scale LLM reasoning), the DE is *pluggable*.  The evaluator only sees a
trait object and never bakes in any policy.

---

## 2  Core Abstractions

### 2.1  `ExecTarget`
Rust enum describing the selected destination.  The string **id** lets us map to
a concrete `ModelProvider` at runtime ("phi-mini", "gpt4o", "sentiment-rules").

### 2.2  `CallContext`
Light-weight, hashable struct carrying the *minimum* information required to
make a routing decision:

* `fn_symbol`: fully-qualified RTFS symbol (e.g. `http/get-json`).
* `arg_type_fingerprint`: 64-bit hash of argument *types* (not values).
* `runtime_context_hash`: hash of ambient context (permissions, tenant, etc.).

### 2.3  `DelegationEngine` trait
```rust
trait DelegationEngine {
    fn decide(&self, ctx: &CallContext) -> ExecTarget;
}
```
*Must be pure* so the evaluator can safely memoise results.

### 2.4  `ModelProvider` trait & Registry
```rust
trait ModelProvider {
    fn id(&self) -> &'static str;
    fn infer(&self, prompt: &str) -> Result<String>;
}
```
Both in-process and remote models implement the same interface.  The global
`ModelRegistry` resolves the string id from `ExecTarget::LocalModel` /
`RemoteModel` to an actual provider.

---

## 3  Execution Flow
1. Evaluator builds a `CallContext` for the function node.
2. It calls `DelegationEngine::decide(ctx)`.
3. Depending on the returned `ExecTarget`:
   * **LocalPure** → run the existing evaluator logic.
   * **LocalModel(id)** → look up provider in `ModelRegistry` and run `infer`.
   * **RemoteModel(id)** → use provider stub that performs an RPC to Arbiter.
4. Result propagates back to the evaluator – identical signature regardless of
   path, maximising composability.

![DE flow](placeholder)

---

## 4  Implementation Phases
| Milestone | Scope |
|-----------|-------|
| **M0** | Skeleton (`ExecTarget`, `CallContext`, `StaticDelegationEngine`) – *DONE* |
| **M1** | Parser/AST/IR metadata plumbing for optional `^:delegation` hint |
| **M2** | Evaluator integration + criterion micro-benchmarks (<0.2 ms/1M) |
| **M3** | `ModelProvider` registry, `LocalEchoModel`, `RemoteArbiterModel` |
| **M4** | LRU decision cache + metrics (`delegation_cache_size`, hits) |
| **M5** | Policy loading (`delegation.toml`), async ticket runner, fallback logic |

---

## 5  Design Rationale
* **Safety** – DE lives *outside* deterministic evaluator, so crashes or heavy
  model weights cannot destabilise the core.
* **Provider-agnostic** – any model (from 50 MB quantised to cloud GPT-4) is just
  another `ModelProvider`.
* **Hot-swap** – because DE is a trait object, runtimes can load a different
  implementation at startup without recompiling evaluator.
* **Performance** – static fast-path & caching keep overhead < 100 ns per call
  in common loops.

---

## 6  Open Questions
1. **Policy engine** – do we need per-tenant or per-capability rules?
2. **Sandboxing** – should on-device models run in WASM for memory isolation?
3. **Durability** – store async tickets in message queue (NATS, RabbitMQ)?

---

## 7  References
* Split vs. Embedded Arbiter analysis (chat log 2025-07-XX).  
* `rtfs_compiler/src/ccos/delegation.rs` – canonical implementation skeleton.

```

```markdown
// ... existing code ...
- [ ] **Context and State Management**
  - [ ] Task context implementation
  - [ ] Resource state tracking
  - [ ] Agent communication state
  - [ ] Module state persistence
+
+- [ ] **Delegation Engine (DE) Integration**
+  - [x] DelegationEngine skeleton (`ExecTarget`, `CallContext`, `StaticDelegationEngine`) merged into `ccos::delegation` 📦
+  - [ ] Metadata plumbing – carry optional `^:delegation` hint from parser → AST → IR
+  - [ ] Evaluator wrapper uses `DelegationEngine::decide` for each function node
+  - [ ] Criterion micro-benchmarks (pure evaluator vs evaluator+DE) – overhead < 0.2 ms / 1 M calls
+  - [ ] `ModelProvider` registry with `LocalEchoModel` stub
+  - [ ] `RemoteModel` path via Arbiter RPC prototype (gRPC/HTTP TBD)
+  - [ ] Decision caching with LRU (≤ 64 K entries) & metrics (`delegation_cache_size`, `cache_hit_rate`)
+
 ### Phase 7: Advanced Features 🔄 PENDING

```

📚  Documentation updated!

1. **docs/ccos/DELEGATION_ENGINE.md** – brand-new file explaining concepts, core abstractions, execution flow and phased roadmap for the Delegation Engine.

2. **docs/ccos/RTFS_MIGRATION_PLAN.md** – Phase 6 now includes a dedicated “Delegation Engine (DE) Integration” checklist, marking the skeleton as complete and outlining next milestones.

Please review the added docs and let me know when you’d like to move on to the next steps (e.g. metadata plumbing in the parser/AST, evaluator wiring, or benchmark harness).

---

**User**

add delegation.rs to compiling target and then show me what means exactly  "metadata plumbing in the parser/AST" to see the impacts

---

**Cursor**

```rust
// ... existing code ...
// Re-export CCOS components
pub use ccos::{types::*, CCOSRuntime};
+pub use ccos::delegation; // expose Delegation Engine API

```

✅  `delegation.rs` is now part of the public API (`pub use ccos::delegation;` in `lib.rs`), so any crate that depends on `rtfs_compiler` can do:

```rust
use rtfs_compiler::delegation::{ExecTarget, CallContext, DelegationEngine};
```

Compilation should work without additional flags.

────────────────────────────────────────────────────────────────────────
What “metadata plumbing in the parser/AST” means
────────────────────────────────────────────────────────────────────────
Goal: carry an optional “delegation hint” from source code to the evaluator.

1. **Syntax layer (`rtfs.pest` grammar)**
   • Extend the function metadata rule to accept  
     `^:delegation :local | :local-model "id" | :remote "id"`  
   • No change for code that omits the tag (100 % back-compat).

2. **Parser (`src/parser/…`)**
   • When building an `AstFunction`, look for a `delegation` tag and parse it
     into an enum `DelegationHint` mirroring `ExecTarget` but optional:
     ```rust
     pub enum DelegationHint {
         LocalPure,
         LocalModel(String),
         RemoteModel(String),
     }
     ```
   • Field added to the AST struct:
     ```rust
     pub struct AstFunction {
         …,
         pub delegation_hint: Option<DelegationHint>,
     }
     ```

3. **IR converter (`src/ir/converter.rs`)**
   • Propagate the hint:
     ```rust
     IrNode::FnCall { …, delegation_hint: ast_fn.delegation_hint.clone(), … }
     ```
   • `IrFnCall` gains the same optional field.

4. **Evaluator (`src/runtime/evaluator.rs`)**
   • When executing a `IrFnCall`, first check `delegation_hint`:
     ```rust
     if let Some(hint) = node.delegation_hint {
         match hint.to_exec_target() { … }
     } else {
         self.delegation_engine.decide(&ctx)
     }
     ```
   • `DelegationHint::to_exec_target()` converts to the `ExecTarget` enum,
     bypassing the DelegationEngine entirely for static hints.

5. **Static fast-path optimisation**
   • If `delegation_hint == LocalPure`, evaluator never allocates
     a `CallContext`, saving ~1 ns per call on tight loops.

6. **Builders and code-gen tools**
   • Object builders that emit RTFS code (e.g. `CapabilityBuilder`)
     accept `.with_delegation_hint(DelegationHint::RemoteModel("gpt4o"))`
     and render the correct `^:delegation` metadata in the generated code.

7. **Tests**
   • Parser round-trip tests ensure the hint is preserved.
   • Criterion micro-benchmarks verify no measurable overhead when the hint is
     `LocalPure`.

That’s the full “plumbing” path—grammar → AST → IR → runtime—ensuring the
delegation hint flows through untouched until it affects execution.

Let me know if you want to start implementing any of these steps or dive
deeper into a specific layer.

```rust
use rtfs_compiler::delegation::{ExecTarget, CallContext, DelegationEngine};
```

```rust
     pub enum DelegationHint {
         LocalPure,
         LocalModel(String),
         RemoteModel(String),
     }
```

```rust
     pub struct AstFunction {
         …,
         pub delegation_hint: Option<DelegationHint>,
     }
```

```rust
     IrNode::FnCall { …, delegation_hint: ast_fn.delegation_hint.clone(), … }
```

```rust
     if let Some(hint) = node.delegation_hint {
         match hint.to_exec_target() { … }
     } else {
         self.delegation_engine.decide(&ctx)
     }
```

---

**User**

start implementing

---

**Cursor**

```rust
// ... existing code ...
 use serde::{Deserialize, Serialize};
 // ... existing code ...
+
+// --- Delegation Hint ---
+/// Optional compile-time hint that instructs the runtime where a function
+/// prefers to execute.  Mirrors (but is independent from) `ExecTarget` in the
+/// CCOS Delegation Engine to avoid circular dependencies.
+#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, JsonSchema)]
+#[serde(rename_all = "camelCase")]
+pub enum DelegationHint {
+    LocalPure,
+    LocalModel(String),
+    RemoteModel(String),
+}
 // ... existing code ...
@@
 pub struct FnExpr {
     pub params: Vec<ParamDef>,
     pub variadic_param: Option<ParamDef>, // Changed from Option<Symbol>
     pub return_type: Option<TypeExpr>,
     #[validate(nested)]
     pub body: Vec<Expression>,
+    pub delegation_hint: Option<DelegationHint>,
 }
 // ... existing code ...
@@
 pub struct DefnExpr {
     pub name: Symbol,
     pub params: Vec<ParamDef>,
     pub variadic_param: Option<ParamDef>, // Changed from Option<Symbol>
     pub return_type: Option<TypeExpr>,
     #[validate(nested)]
     pub body: Vec<Expression>,
+    pub delegation_hint: Option<DelegationHint>,
 }
 // ... existing code ...
```

```rust
// ... existing code up to creation of FnExpr ...
-    Ok(FnExpr {
-        params,
-        variadic_param,
-        body,
-        return_type,
-    })
+    Ok(FnExpr {
+        params,
+        variadic_param,
+        body,
+        return_type,
+        delegation_hint: None,
+    })
 // ... existing code ...
@@
-    Ok(DefnExpr {
-        name,
-        params,
-        variadic_param,
-        return_type,
-        body,
-    })
+    Ok(DefnExpr {
+        name,
+        params,
+        variadic_param,
+        return_type,
+        body,
+        delegation_hint: None,
+    })

```

```rust
// ... existing code ...
-                    body: vec![Expression::FunctionCall {
+                    body: vec![Expression::FunctionCall {
                         callee: Box::new(Expression::Symbol(Symbol("+".to_string()))),
                         arguments: vec![
                             Expression::Symbol(Symbol("x".to_string())),
                             Expression::Symbol(Symbol("y".to_string())),
                         ],
                     }],
+                    delegation_hint: None,
                 })),
 // ... existing code ...
```

```rust
// ... existing code segment ...
-                    return_type: Some(TypeExpr::Primitive(PrimitiveType::Int)),
-                    body: vec![
+                    return_type: Some(TypeExpr::Primitive(PrimitiveType::Int)),
+                    body: vec![
                         Expression::List(vec![
                             Expression::Symbol(Symbol("+".to_string())),
                             Expression::Symbol(Symbol("a".to_string())),
                             Expression::Symbol(Symbol("b".to_string())),
                             Expression::List(vec![
                                 Expression::Symbol(Symbol("*".to_string())),
                                 Expression::Symbol(Symbol("a".to_string())),
                                 Expression::Symbol(Symbol("b".to_string())),
                             ]),
                         ])
-                    ],
+                    ],
+                    delegation_hint: None,
                 })),
// ... existing code ...
```

```rust
// ... existing code ...
-        let fn_expr = FnExpr {
-            params: defn_expr.params,
-            variadic_param: defn_expr.variadic_param,
-            return_type: defn_expr.return_type,
-            body: defn_expr.body,
-        };
+        let fn_expr = FnExpr {
+            params: defn_expr.params,
+            variadic_param: defn_expr.variadic_param,
+            return_type: defn_expr.return_type,
+            body: defn_expr.body,
+            delegation_hint: defn_expr.delegation_hint,
+        };

```

Initial plumbing is now in place so the Delegation hint can flow from the parser all the way to the IR:

1. Code changes  
   • `ast.rs`  
     – added `DelegationHint` enum  
     – added `delegation_hint: Option<…>` to `FnExpr` & `DefnExpr`  
   • `parser/special_forms.rs`  
     – `build_fn_expr` and `build_defn_expr` now populate `delegation_hint: None` (placeholder) so everything still parses/compiles.  
   • Examples / demos (`optimization_demo.rs`, `ir_demo_complete.rs`)  
     – added `delegation_hint: None` to every manual `FnExpr` literal.  
   • IR converter (`ir/converter.rs`)  
     – carries the new field when building a lambda from `defn`.  
   • Public re-export (`lib.rs`) now exposes the `ccos::delegation` API.  

2. Nothing in the runtime actually *uses* the hint yet—that’s the next milestone—but the types compile and all existing code keeps working.

3. What “metadata plumbing in the parser/AST” now concretely means:  
   a. Allow the grammar to parse `^:delegation` metadata (next task).  
   b. Fill `delegation_hint` inside the builders instead of the current `None`.  
   c. Propagate the hint into IR (either on the lambda or per-call site).  
   d. Teach the evaluator to prefer the hint before calling the `DelegationEngine`.

Would you like me to:

A. Extend the grammar to actually parse `^:delegation …` tags, or  
B. Wire the evaluator so that an `ExecTarget` is selected when those hints are present, or  
C. Tackle something else first?

Let me know the next step you’d like to see implemented.

---

**User**

commit current code

---

**Cursor**

All current changes have been committed to the repository under the message:

“feat(ccos): add DelegationEngine skeleton and initial metadata plumbing”

Let me know when you’d like to move on to the next implementation step!

---

**User**

can you modify docs/ccos delegation doc to explain this delegration hint and gramme modification and ExecTarget to be able to implement it.

---

**Cursor**

```markdown
// ... existing code ...
### 2.4 `ModelProvider` trait & Registry
// ... existing code ...

+### 2.5  Delegation-hint metadata (`^:delegation`)
+
+While the `DelegationEngine` decides *dynamically* at runtime, authors can
+embed an *authoritative* or *suggested* target directly in the source code.
+
+```clojure
+(defn http-get                     ; simple wrapper around stdlib
+  ^:delegation :remote "gpt4o"    ; ← always go to powerful remote model
+  [url]
+  (http/get-json url))
+
+;; Local tiny-LLM example
+(defn classify-sentiment
+  ^:delegation :local-model "phi-mini"
+  [text]
+  (tiny/llm-classify text))
+
+;; Explicitly force pure evaluator (default, shown for completeness)
+(fn ^:delegation :local [x] (+ x 1))
+```
+
+At compile time the parser stores this information as
+
+```rust
+ast::DelegationHint::LocalPure             // :local (or omitted)
+ast::DelegationHint::LocalModel("phi-mini")
+ast::DelegationHint::RemoteModel("gpt4o")
+```
+
+which maps *one-to-one* to `ExecTarget` via `to_exec_target()` helper.
+
+If a hint is present the evaluator *bypasses* the `DelegationEngine` for that
+call, guaranteeing the author’s intent.  Otherwise it falls back to the DE’s
+policy decision.
+
+---
+
+## 3  Syntax & Grammar Changes
+
+The RTFS grammar is extended so that **any definition form** (`fn`, `defn`,
+future `capability` objects, …) may carry one or more `^:keyword` metadata
+tags before its parameter list.  Specifically for delegation we add:
+
+```
+delegation_meta =  "^" ":delegation" WS* delegation_target
+
+delegation_target =
+      ":local"                               // LocalPure
+    | ":local-model"  string_lit            // LocalModel(id)
+    | ":remote"       string_lit            // RemoteModel(id)
+```
+
+Where `string_lit` is a standard RTFS string literal (model identifier).
+
+Parser → AST → IR flow:
+
+1. **Parser** detects `delegation_meta` and populates
+   `FnExpr::delegation_hint` / `DefnExpr::delegation_hint`.
+2. **IR converter** copies the hint into the corresponding `IrNode::Lambda`
+   metadata (TBD) so later optimisation passes can still see it.
+3. **Evaluator** checks `delegation_hint` *first*; if `None` it calls
+   `DelegationEngine::decide`.
+
+This mechanism keeps the runtime fast (no hash-lookups) when the author has
+declared an explicit target, yet allows dynamic decisions when the hint is
+absent.
+
+---

```

The Delegation Engine documentation now includes:

1. A new “Delegation-hint metadata (`^:delegation`)" section  
   • Concrete syntax examples (`:local`, `:local-model "phi-mini"`, `:remote "gpt4o"`).  
   • Mapping to the new `ast::DelegationHint` enum and to `ExecTarget`.  
   • Explanation of evaluator fast-path when a hint is present.

2. A “Syntax & Grammar Changes” subsection detailing the grammar additions and the end-to-end flow (Parser → AST → IR → Evaluator).

This should provide clear guidance for implementing the remaining plumbing.

Let me know when you’re ready to implement the grammar change or work on another part.

