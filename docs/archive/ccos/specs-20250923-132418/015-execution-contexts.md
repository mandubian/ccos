# 015 - Execution Contexts in CCOS

This document specifies the roles, semantics, and integration of execution-related contexts in CCOS.
It complements the existing architecture and orchestration specs by defining the separation of concerns
between security, runtime container, hierarchical execution data/state, and host metadata.

Related specs:
- 000 - Architecture
- 001 - Intent Graph
- 002 - Plans & Orchestration
- 003 - Causal Chain
- 014 - Step Special Form Design

## Context Types

CCOS uses four context constructs with distinct responsibilities:

### 1) RuntimeContext (Security & Permissions)
- Location: `rtfs_compiler/src/runtime/security.rs`
- Purpose: Security policy and capability permissions for the current execution
- Examples: security level, allowed capabilities, resource/timeout limits, microVM enforcement
- Ownership: Created by `CCOSEnvironment` and passed to `RuntimeHost` and `Evaluator`
- Scope: Determines WHAT operations are allowed

### 2) CCOSEnvironment (Runtime Container)
- Location: `rtfs_compiler/src/runtime/ccos_environment.rs`
- Purpose: Owns and wires together the evaluator, host, marketplace, and configuration
- Components: `Evaluator`, `RuntimeHost`, `CapabilityMarketplace`, `CausalChain`
- Scope: Provides WHERE execution happens (complete runtime container)

### 3) ExecutionContext (Hierarchical Data & State) [NEW]
- Location: `rtfs_compiler/src/ccos/execution_context.rs`
- Purpose: Hierarchical data/state that flows between plan steps and parallel branches
- Structures:
  - `ContextManager`: high-level API for step entry/exit, get/set, checkpoint, serialize/deserialize
  - `ContextStack`: maintains parent/child relationships and current context
  - `ExecutionContext`: per-step context with `data`, `metadata`, `children`
  - `IsolationLevel`: `Inherit`, `Isolated`, `Sandboxed`
- Scope: Manages HOW execution data flows during orchestration

### 4) HostPlanContext (Host Logging Metadata)
- Location: `rtfs_compiler/src/runtime/host.rs`
- Purpose: Minimal metadata for causal chain logging
- Fields: `plan_id`, `intent_ids`, `parent_action_id`
- Notes: Previously named `ExecutionContext`; renamed to avoid confusion with hierarchical context

## Isolation Semantics

Isolation levels define parent visibility and mutation behavior:
- Inherit: Reads and writes are local; reads fall back to parent; parent is NOT mutated automatically
- Isolated: Reads fall back to parent (read-only view); writes are local to the child
- Sandboxed: No parent visibility; reads/writes are local only

Implementation note:
- `ContextStack::lookup` climbs to parent unless `Sandboxed`
- `merge_child_to_parent` is explicit and uses `ConflictResolution` (`KeepExisting`, `Overwrite`, `Merge`)

## Lifecycle & API

ContextManager operations:
- `initialize(step_name: Option<String>)`: creates root context (ID defaults to provided name or "root")
- `enter_step(step_name: &str, isolation: IsolationLevel) -> context_id`: pushes child context
- `exit_step() -> Option<ExecutionContext>`: pops current context
- `get(key) -> Option<Value>`: lookup per isolation semantics
- `set(key, value)`: sets value in current context
- `checkpoint(id)`: annotates current context with checkpoint ID
- `serialize() -> String` / `deserialize(data)`: persistence and resume
- `create_parallel_context(step_name)` / `switch_to(context_id)`: support for parallel orchestration

Metadata captured per context (`ContextMetadata`):
- `created_at`, `step_name`, `step_id`, `checkpoint_id`, `is_parallel`, `isolation_level`, `tags`

## Orchestration Integration

### Evaluator
- Location: `rtfs_compiler/src/runtime/evaluator.rs`
- Holds `context_manager: RefCell<ContextManager>`
- Provides step special forms per 014-spec:
  - `(step "name" expr ...)`
  - `(step-if cond then-step else-step)`
  - `(step-loop cond body-step)`
  - `(step-parallel step1 step2 ...)` with isolated child contexts
- Semantics:
  - Enter child context on step entry; exit on step completion/failure
  - Step results can be written into the current context (e.g., `set`)
  - Parallel steps create isolated child contexts to avoid cross-branch interference

### RuntimeHost
- Location: `rtfs_compiler/src/runtime/host.rs`
- Uses `HostPlanContext` to create `Action` records in the Causal Chain
- Methods: `notify_step_started`, `notify_step_completed`, `notify_step_failed`, `execute_capability`
- Works under the active `RuntimeContext` for security checks

### Orchestrator
- Location: `rtfs_compiler/src/ccos/orchestrator.rs`
- Creates `RuntimeHost`, `Evaluator`, sets execution context on host
- Drives plan execution (RTFS code) and ensures host/evaluator wiring

## Causal Chain & Auditing

- Each step emits: `PlanStepStarted`, `PlanStepCompleted` or `PlanStepFailed`
- Capability calls emit: `CapabilityCall` with arguments and results
- `HostPlanContext` provides linkage: `plan_id`, `intent_ids`, `parent_action_id`
- Execution data (hierarchical state) is separate from Causal Chain events; do not store bulk state on chain

## Security & Governance

- `RuntimeContext` (security policy) is independent from `ExecutionContext` (data flow)
- Capabilities must pass `RuntimeContext` checks; context data should be validated before passing to capabilities
- Checkpoint/resume points should be covered by governance policies if human-in-the-loop is required

## Persistence & Resume

- Use `ContextManager.serialize()` to persist the full context stack (JSON)
- Use `ContextManager.deserialize()` to restore
- Recommended: persist checkpoints at major plan milestones `(step ...)`

## RTFS Usage Examples

Step with data propagation:
```clojure
(step "Fetch User"
  (do
    (def user (call :app.user:get {:id 42}))
    (set! :user user)))

(step "Send Email"
  (do
    (let [user (get :user)]
      (call :app.email:send {:to (:email user) :template "welcome"}))))
```

Parallel with isolation:
```clojure
(step-parallel
  (step "Render PDF" (do (set! :pdf (call :doc:render {:id 42}))))
  (step "Index Search" (do (set! :indexed (call :search:index {:id 42}))))
)
```
- Each branch runs in an isolated child context; parent context is not mutated until an explicit merge.

Note: current implementation supports configurable merge policies when consolidating branch context data:

- `:keep-existing` (default): parent-wins; child-only keys are added
- `:overwrite` (child-wins): child values overwrite parent values
- `:merge`: deep merge; maps are merged key-wise recursively and vectors are concatenated

### Merge behavior examples

Parent-wins default (keep existing):
```clojure
; Parent has :k "parent"
(def _ (set! :k "parent"))

(step-parallel
  (step "A" (do (set! :k "child-a") (set! :a 1)))
  (step "B" (do (set! :k "child-b") (set! :b 2))))

; After consolidation:
; - :k remains "parent" (parent-wins)
; - :a and :b are introduced if not present in parent
```

Manual overwrite today (explicit parent write after branches):
```clojure
(let [winner (step-parallel
               (step "A" (do (set! :candidate "a")))
               (step "B" (do (set! :candidate "b"))))]
  ; Select preferred value and set in parent explicitly
  (set! :candidate (if (some-condition) "a" "b")))
```

Implemented: configurable `:merge-policy` including `:keep-existing`, `:overwrite`, and `:merge` (deep merge for maps/vectors).

Conditional step:
```clojure
(step-if (> (get :cost) 100)
  (step "Budget Path" (do ...))
  (step "Premium Path" (do ...)))
```

## Implementation Notes

- Root context ID: defaults to provided name or `"root"`
- `Isolated` can read from parent; `Sandboxed` cannot read from parent
- Automatic checkpointing via `ContextManager::with_checkpointing(interval_ms)`
- Serialization uses `serde_json` for both single context and stack
 - Read-only context exposure to capabilities is security-gated. The runtime exposes a sanitized snapshot
   only when policy allows it for the specific capability. Defaults are off; exposure requires both a global flag
   and a per-capability allowlist match.

### Dynamic Exposure Policy

- **Exact IDs**: Allow specific capability IDs.
- **Prefixes**: Allow namespaces (e.g., `ccos.ai.`) without listing each capability.
- **Tags**: Allow capabilities that declare specific metadata tags (e.g., `needs-context`).
- **Step overrides**: Plans may request exposure for a step with `:expose-context` and restrict keys via `:context-keys`.

The Host evaluates exposure at call-time using the active `RuntimeContext` policy and capability manifest metadata.

## Backward Compatibility

- `HostPlanContext` rename avoids conflicts with prior `ExecutionContext` naming in `RuntimeHost`
- No changes required to existing `RuntimeContext` or `CCOSEnvironment` contracts

## Future Work

- Deep merge semantics for complex values
- True parallel execution (threads/tasks) in evaluator for `step-parallel`
- Policy-based automatic merges from child to parent (e.g., `:merge-policy :overwrite|:merge`)
- Richer typed getters/setters with schema validation

## References
- 000: Architecture
- 002: Plans & Orchestration
- 003: Causal Chain
- 014: Step Special Form Design
