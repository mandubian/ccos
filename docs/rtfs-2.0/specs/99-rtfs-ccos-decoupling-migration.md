# RTFS–CCOS Decoupling Migration Plan

Status: Proposed
Audience: RTFS runtime/compiler owners, CCOS owners
Related: 13-rtfs-ccos-integration-guide.md, 04-streaming-syntax.md, specs-incoming/07-effect-system.md, ../../ccos/specs/005-security-and-context.md

## 1) Goals
- Keep RTFS language/runtime pure and CCOS-agnostic
- Define a minimal RTFS↔CCOS boundary: `HostInterface` + `SecurityAuthorizer`
- Move orchestration, marketplace, auditing, observability, and providers into CCOS
- Clarify streaming: RTFS provides types/schemas + macros; CCOS executes via capabilities

## 2) Scope
- RTFS crate(s): runtime core, stdlib, secure_stdlib, IR/AST evaluators, type system
- CCOS crate(s): orchestrator, causal chain, capability marketplace, delegation, observability, working memory, streaming providers

## 3) Public Interface (stays in RTFS)
- `HostInterface` trait (no CCOS types)
- `SecurityAuthorizer` + `RuntimeContext` (generic; no CCOS imports)
- Evaluator, IR runtime, ModuleRegistry, stdlib/secure_stdlib (call only via `HostInterface`)
- Stream types/schemas and compile-time validations (no provider execution)

## 4) Modules to MOVE to CCOS
- `runtime/capability_marketplace/*`
- `runtime/host.rs` (concrete host); keep trait in RTFS
- `runtime/ccos_environment.rs` (bootstrap/env + WM sink registration)
- `runtime/metrics_exporter.rs` (CausalChain-based exporter)
- `runtime/capabilities/*` (registry/provider plumbing)
- Any direct CausalChain consumers (metrics, logs, sinks)

## 5) Security split
- Keep (RTFS):
  - `RuntimeContext`, `SecurityAuthorizer::{authorize_capability, authorize_program, validate_execution_context}`
  - Local `IsolationLevel` enum (stop importing CCOS types)
- Move (CCOS):
  - `SecurityPolicies` presets (hard-coded `ccos.*` IDs)
  - Recommended levels and cap-ID–specific validators / governance hooks

## 6) Delegation (Yield-Based Control Inversion)
- **RTFS is Delegation-Unaware**: The RTFS runtime core (`IrRuntime`) will be refactored to be a pure execution kernel. It will have no knowledge of delegation, models, or capabilities.
- **Yield on Non-Pure Calls**: When the RTFS runtime encounters a function call it cannot resolve locally (i.e., a non-builtin, non-pure function), it will not make a decision. Instead, it will **yield** control back to its caller (the CCOS Orchestrator).
- **New `ExecutionOutcome` Return Type**: The `IrRuntime::execute_node` function will return a result like `ExecutionOutcome`, which will have two variants:
  - `Complete(Value)`: The execution finished successfully within the pure RTFS kernel.
  - `RequiresHost(HostCall)`: The runtime encountered a call that requires external handling. The `HostCall` struct will contain the function name and arguments.
- **CCOS Owns Delegation**: The CCOS `Orchestrator` becomes the top-level execution loop. It invokes the RTFS runtime and, upon receiving a `RequiresHost` outcome, uses its own CCOS-specific `DelegationEngine` to decide how to handle the call (e.g., execute a capability, query a model).
- **No RTFS Delegation Trait**: The `DelegationEngine` trait will be removed entirely from the RTFS runtime (`src/runtime/delegation.rs` will be deleted). All delegation logic will reside exclusively within CCOS.

## 7) Streaming evolution
- RTFS:
  - Keep stream schema vocabulary (e.g., `[:stream ...]`, `[:stream-bi ...]`) and static checks
  - Provide macros that lower:
    - `(stream-source id opts)` → `(call :marketplace.stream.start {...})`
    - `(stream-consume h {...})` → `(call :marketplace.stream.register-callbacks {...})` or a pull loop using `:marketplace.stream.next`
    - `(stream-sink id)` → `(call :marketplace.stream.open-sink {...})`; `(stream-produce h item)` → `(call :marketplace.stream.send {...})`
  - No provider/marketplace code in RTFS
- CCOS:
  - Own streaming providers, registration, discovery, callbacks, pipelines, multiplex/demux, monitoring
- Spec updates:
  - Update `04-streaming-syntax.md` to mark operational forms as macros that lower to `(call ...)`
  - Add “Lowering to (call ...)” section and capability IDs
  - Cross-link to `07-effect-system.md`: streaming implies effects (e.g., `:network`, `:ipc`) checked like others

## 8) Constructor changes and PureHost
- RTFS constructors (Runtime, IrStrategy, Evaluator) accept injected `Arc<dyn HostInterface>`
- Provide `PureHost` in RTFS for language-only tests (no external effects)
- Remove construction of CausalChain/marketplace from RTFS code paths

## 9) Invariants and CI guardrails
- No `ccos::*` imports allowed under `rtfs_compiler/src/runtime/**`
- Lint/unit test: ensure `HostInterface` is the only boundary for external effects
- Feature gates: keep any CCOS-facing shims behind CCOS crate

## 10) Migration steps (checklist)
- Host + Marketplace moves
  - [x] Move `runtime/host.rs` → `ccos/host/runtime_host.rs`
  - [x] Keep `runtime/host_interface.rs` in RTFS (remove CCOS type refs)
  - [x] Move `runtime/capability_marketplace/**` → `ccos/capability_marketplace/**`
  - [x] Move `runtime/capabilities/**` → `ccos/capabilities/**`
  - [x] Move `runtime/metrics_exporter.rs` → `ccos/observability/metrics_exporter.rs`
  - [x] Move `runtime/ccos_environment.rs` → `ccos/environment.rs`
- RTFS core refactors
  - [x] Add `PureHost`
  - [x] Update `runtime/mod.rs`, `ir_runtime.rs`, `evaluator.rs` to accept injected host and stop constructing CCOS types
  - [x] Remove `register_default_capabilities` from stdlib (rehome in CCOS bootstrap)
- Security split
  - [ ] Remove `use crate::ccos::execution_context::IsolationLevel` from `security.rs`; define RTFS-local enum
  - [ ] Move `SecurityPolicies`, recommended mappings to CCOS
- Delegation
  - [ ] **Refactor `IrRuntime` to be Delegation-Unaware**:
  - [ ] Remove the `delegation_engine` field from `IrRuntime`.
  - [ ] Modify `IrRuntime::execute_node` to return an `ExecutionOutcome` enum (`Complete(Value)` or `RequiresHost(HostCall)`).
  - [ ] When a non-pure function call is encountered, the runtime should yield by returning `RequiresHost`.
- [ ] **Elevate CCOS Orchestrator**:
  - [ ] The `Orchestrator` will now own the `DelegationEngine` and manage the top-level execution loop.
  - [ ] Implement logic in the `Orchestrator` to handle the `RequiresHost` outcome, make a delegation decision, and resume RTFS execution with the result.
- [ ] **Consolidate Delegation Code**:
  - [ ] Delete the `rtfs_compiler/src/runtime/delegation.rs` file and its associated trait.
  - [ ] Ensure all delegation logic, traits, and implementations reside solely within the `ccos` module.
- Streaming
  - [x] Move `runtime/rtfs_streaming_syntax.rs` (executor, provider, marketplace usage) to CCOS
  - [x] Keep stream schema types + validation in RTFS type system (no providers)
  - [x] Implement macro lowering of streaming forms to `(call ...)`
  - [x] Update `docs/rtfs-2.0/specs/04-streaming-syntax.md` accordingly
- Docs
  - [x] Update `docs/ccos/specs/005-security-and-context.md` with RTFS–CCOS interface section
  - [x] Update `docs/rtfs-2.0/specs/13-rtfs-ccos-integration-guide.md` to reflect injection, PureHost, and streaming lowering
    - [x] Reference `07-effect-system.md` for streaming effects

## 11) Backward compatibility and deprecations
- Provide a transitional feature flag that still constructs `RuntimeHost` in RTFS for internal tests; mark deprecated and remove after migration
- Keep `(stream-*)` forms available but implemented as macros that expand to `(call ...)`
- SemVer: bump RTFS minor/major depending on public API changes (HostInterface signature changes, removed re-exports)

## 12) Testing strategy
- RTFS: run language suites with `PureHost`; no external side effects
- CCOS: integration tests for capability execution, streaming, delegation, auditing
- Contract linking tests: when capability contracts are present, perform static checks; otherwise exercise runtime validation on CCOS side

## 13) Migration Status Summary

### ✅ **COMPLETED (Phase 1, 2, 3 & 6)**
- **Structural Migration**: All CCOS components moved from RTFS runtime to CCOS
- **PureHost Implementation**: RTFS now uses PureHost by default for testing
- **Interface Cleanup**: All import paths updated, compilation successful
- **Streaming Integration**: Streaming code moved to CCOS with proper interface
- **Security Split**: RTFS has its own isolation levels independent of CCOS
- **Delegation Cleanup**: RTFS has its own delegation system independent of CCOS
- **Documentation**: Complete integration guide with host injection patterns and effect system references
- **Test Separation**: Pure RTFS tests separated from CCOS integration tests
- **CCOS Tests Fixed**: All CCOS unit tests and integration tests now compile and run successfully

### 🔄 **REMAINING TASKS (Revised Architecture)**
- **Implement Yield-Based Control Flow**: Refactor the `IrRuntime` to remove the `DelegationEngine` and return an `ExecutionOutcome` to yield control for non-pure calls.
- **Update CCOS Orchestrator**: Implement the top-level execution loop in the `Orchestrator` to handle the `RequiresHost` outcome from the RTFS runtime.
- **Consolidate Delegation Logic**: Delete `src/runtime/delegation.rs` and ensure all delegation code is owned by CCOS.
- **Fix Integration Tests**: Update all integration tests to work with the new CCOS-driven execution model.
- **Final Validation**: Run comprehensive test suite and validate complete independence.
- **Performance Testing**: Benchmark the new architecture.
- **Production Readiness**: Final integration testing and documentation review.

### 📊 **Progress**: 15/20 tasks completed (75%) - Phase 4 In Progress ⚠️

## 14) Open items / risks
- Contract availability at compile time (static vs runtime checks)
- Determinism metadata for streaming (replayability)
- Callback re-entry semantics (ensure Orchestrator hops are well defined)
- Performance regression risk from macro lowering; validate with benchmarks

## 15) Architectural Decision: Inverting Control for Delegation

### Problem Description
The previous migration phase left the RTFS `IrRuntime` "delegation-aware," meaning it contained a `DelegationEngine` and made decisions about where to execute code. This created a tight coupling with CCOS, resulted in duplicated `delegation.rs` modules, and violated the principle of a pure, minimal RTFS core. The presence of two incompatible `DelegationEngine` traits was a symptom of this deeper architectural issue.

### The New Architecture: Yield-Based Control Inversion
To achieve a true separation of concerns, we are adopting a **yield-based control flow**.

1.  **RTFS Runtime Becomes a Pure Kernel**: The `IrRuntime` will be stripped of all delegation logic. It will no longer contain a `DelegationEngine`. Its sole responsibility is to execute pure RTFS code.

2.  **Yielding on Non-Pure Calls**: When the `IrRuntime` encounters a function call that it cannot resolve locally (e.g., a call to a model or a capability), it will **yield** execution. It will return an `ExecutionOutcome::RequiresHost(HostCall)` variant, passing the details of the required operation back to its caller.

3.  **CCOS Orchestrator as the Execution Driver**: The CCOS `Orchestrator` is now the top-level execution loop. It invokes the RTFS runtime.
    *   If the runtime returns `Complete(Value)`, the execution is finished.
    *   If the runtime returns `RequiresHost(HostCall)`, the `Orchestrator` uses its **own internal `DelegationEngine`** to decide how to handle the call. It then executes the required action (e.g., via the `CapabilityMarketplace`) and can resume the RTFS runtime with the result.

### Impact and Benefits
- **Clean Separation**: RTFS becomes a simple, verifiable library. CCOS has full authority over all side effects and complex orchestration logic.
- **Eliminates Duplication**: The `src/runtime/delegation.rs` module will be deleted, and all delegation logic will be consolidated within CCOS. The incompatible trait problem is resolved at its root.
- **Architectural Clarity**: This model enforces the desired boundary: RTFS is the "engine," and CCOS is the "chassis" that decides how to use it.

## Recent workspace edits (assistant activity)

The following concise log records recent changes applied to the workspace while migrating the runtime to the new ExecutionOutcome-based control flow. These edits were applied to stabilize the repository and continue the migration incrementally. They are intentionally minimal and reversible; a follow-up sweep is recommended to make them comprehensive.

- Fixed an unexpected closing delimiter in `src/runtime/evaluator.rs` by removing a duplicated trailing block that caused a brace imbalance and a compile error.
- Began migrating evaluator return types to `ExecutionOutcome` semantics:
  - Replaced specific branches that returned raw `Value` with `Ok(ExecutionOutcome::Complete(value))` where appropriate (example: keyword-as-function `Value::Nil` branch).
  - Unwrapped `ExecutionOutcome` from `eval_expr` in the `eval_match` implementation so pattern-matching helpers receive `&Value` and `RequiresHost` is propagated correctly.
- Updated the CLI binary `src/bin/rtfs_compiler.rs` to normalize runtime results: values produced by `runtime.run(expr)` are wrapped into `ExecutionOutcome::Complete(value)` before being collected into `all_results` to avoid mixed-type collections.

Notes and next steps:

- These edits were verified by running `cargo check` (development profile); the build now completes with warnings but no errors.
- The migration to `ExecutionOutcome` is partial. There are still multiple locations in `src/runtime/evaluator.rs`, `src/runtime/stdlib.rs`, and `src/runtime/secure_stdlib.rs` that require consistent wrapping or unwrapping of `ExecutionOutcome`.
- Recommended immediate next work: systematic sweep of `src/runtime/evaluator.rs` to finish replacing raw `Value` returns and update call-sites to handle `ExecutionOutcome`, then update stdlib modules, and finally implement orchestrator resume support for `RequiresHost`.

