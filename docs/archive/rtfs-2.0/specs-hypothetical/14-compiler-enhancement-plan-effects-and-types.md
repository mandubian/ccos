# RTFS Compiler Enhancement Plan: Effect System and Type System Upgrades

Status: Proposed (specs-incoming)
Audience: Compiler/Runtime engineers, Governance Kernel, Orchestrator
Related:
- Language Overview: docs/rtfs-2.0/specs-incoming/00-rtfs-2.0-overview.md
- Language Guide: docs/rtfs-2.0/specs-incoming/02-language-guide.md
- Effect System: docs/rtfs-2.0/specs-incoming/07-effect-system.md
- Concurrency & Determinism: docs/rtfs-2.0/specs-incoming/08-concurrency-and-determinism.md
- Capability Contracts: docs/rtfs-2.0/specs-incoming/09-capability-contracts.md
- Admission-time Compilation: docs/rtfs-2.0/specs-incoming/12-admission-time-compilation-and-caching.md
- Resource Estimation: docs/rtfs-2.0/specs-incoming/13-resource-estimation-and-envelopes.md
- Implementation entry-points:
  - Grammar: rtfs_compiler/src/rtfs.pest
  - AST: rtfs_compiler/src/ast.rs
  - Parser: rtfs_compiler/src/parser/*
  - Type validation: rtfs_compiler/src/runtime/type_validator.rs
  - IR + Optimizer: rtfs_compiler/src/ir/*
  - Runtimes: rtfs_compiler/src/runtime/*
  - GK/CCOS glue: rtfs_compiler/src/ccos/*

---

## 1) Goals

1. Introduce a first-class Effect System to the compiler:
   - Parse ^{:effects [...]} and infer effect rows bottom-up.
   - Validate effect subtyping/normalization against annotations and GK policy.
   - Surface actionable diagnostics for Arbiter auto-repair.

2. Strengthen the Type System:
   - Enforce function signatures and return types more strictly (while remaining gradual).
   - Expand refinement predicate coverage and static evaluation.
   - Add step-level pre/post contracts with runtime guards where needed.
   - Improve match exhaustiveness and typed error handling integration.

3. Integrate Capability Contracts in the compiler:
   - Load contracts to validate call-site inputs and expected outputs.
   - Merge declared effects/resources from contracts into program effect analysis.
   - Generate runtime schema checks where statics are unknown.

4. Wire admission-time pipeline with Orchestrator/GK:
   - Provide a single “admit_plan” compiler API returning Effect/Type reports, Plan Envelope material, and compiled IR.
   - Support content-addressed caching for normalized AST and compiled artifacts.

---

## 2) Effect System Implementation

References: specs-incoming/07-effect-system.md

2.1 AST/Metadata support
- Extend AST nodes (FnExpr, DefnExpr, WithResourceExpr, ParallelExpr, TryCatchExpr, Expression::FunctionCall) to carry optional metadata map including :effects and :resources.
  - File: rtfs_compiler/src/ast.rs
- Parser:
  - Recognize metadata ^{...} preceding fn/defn/plan/step/call and attach to AST.
  - File: rtfs_compiler/src/rtfs.pest + parser/*

2.2 Effect Row data model
- New internal types:
  - EffectKind (enum), EffectParams (map), EffectItem {kind, params}, EffectRow(Vec<EffectItem>)
- Normalization and subtyping utilities:
  - normalize_row(row) -> row’
  - item_subtype?(a, b) and row_subtype?(a, b)
- File: new module rtfs_compiler/src/runtime/effects.rs (or type_validator.rs sibling)

2.3 Inference pass (AST → EffectRow)
- Bottom-up inference:
  - Pure expressions = []
  - Calls: use capability contract effects; allow additional narrowing annotations at call-site.
  - let/if/match/do: union children effect rows then normalize.
  - parallel: union branch rows (branch profiles preserved for runtime).
  - try/catch: union try-body and handler effects.
- Validate declared annotations:
  - If node has ^{:effects e_decl}, then require row_subtype?(inferred, e_decl) else error.
- Emit EffectReport attached to nodes for GK mapping.
- Files:
  - New pass in rtfs_compiler/src/runtime/type_validator.rs or a new validator module.

2.4 Diagnostics and repair hints
- On violations, include:
  - inferred vs declared rows, failing items, suggested domain/path narrowing, or capability versions with narrower effects.
- Provide machine-readable hints for Arbiter.

---

## 3) Type System Enhancements

References: docs/rtfs-2.0/specs/05-native-type-system.md

3.1 Function signatures and returns
- Enforce return type when present; insert runtime check if not statically known.
- Enforce variadic param types (& T).
- Strengthen destructuring param type checks.

3.2 Refinements: broader static coverage
- Evaluate more predicates statically when literals available:
  - Numeric (>, >=, <, <=), string length bounds, regex (pre-check), collection counts, map key presence.
- For non-literal inputs, generate runtime guard expressions to validate preconditions:
  - Auto-insert guard steps before effectful calls.

3.3 Step contracts (pre/post)
- Support ^{:pre (fn [ctx] ...) :post (fn [ctx ctx’] ...)} metadata:
  - Compile-time: shape/type validation of the contract lambdas.
  - Runtime: orchestrator evaluates contract; failures trigger compensations/quarantine and CC logging.

3.4 Match exhaustiveness and typed errors
- For finite enums and union types, warn on non-exhaustive matches.
- Error handling:
  - Align catch patterns with contract-defined error variants (09).
  - Validate catch shapes; generate diagnostics for unreachable/wider-than-needed catches.

---

## 4) Capability Contracts in Compiler

References: specs-incoming/09-capability-contracts.md

4.1 Contract loading
- Add contract loader sourcing from Marketplace (local registry for tests).
- Cache contracts by capability id + version + digest.

4.2 Call-site validation
- Inputs:
  - Validate statically when value shapes known; otherwise stage runtime schema check.
- Outputs:
  - Require runtime validation by default; allow statics if fully determined.
- Errors:
  - Allow typed catch; validate branch shapes against contract error variants.

4.3 Effects/resources merging
- Capability-declared effects/resources must be included in inferred rows.
- Forbid privilege broadening: if plan annotations exceed contract effects, error.
- Allow call-site narrowing (domains/methods subset).

---

## 5) Admission-Time Compiler API

References: specs-incoming/12-admission-time-compilation-and-caching.md

Provide new entrypoint:

compile::admit_plan(plan_source: &str, options: AdmitOptions) -> AdmitResult

Returns:
- Normalized AST + hash
- TypeReport (per-node shapes, static checks, runtime-guard insertions)
- EffectReport (per-node rows, normalized envelopes)
- ContractReport (capabilities bound, versions, digests)
- ResourceEstimates (when enabled; integrates 13)
- PlanEnvelopeDraft (material for GK to sign)
- Compiled IR (with optimizer report) or cache hit

Files:
- New module rtfs_compiler/src/admission/mod.rs integrating parser, validators, IR converter.

Caching:
- Key by normalized AST hash + contracts digests + policy version + determinism metadata.
- Store EffectReport/TypeReport for GK/ORCH mapping.

---

## 6) Orchestrator/GK Integration Points

- GK uses EffectReport + ContractReport to enforce policy and sign Plan Envelope.
- ORCH receives:
  - Branch-level effect subsets → sandbox profiles
  - Runtime guards to execute before calls
  - Determinism metadata (08) and idempotency keys (from step metadata)

Emit to Causal Chain:
- PlanApproved (envelope hash)
- PlanStepStarted/Completed with effect profile, guard outcomes, contract version, debits.

---

## 7) IR/Optimizer implications

- IR nodes carry optional effect/type annotations to preserve decisions through optimization.
- Ensure optimizations do not remove required runtime guards.
- Dead code elimination must respect guard side-effects (guards are pure but block downstream effects).

---

## 8) Test Plan

Add new test suites:

8.1 Effect typing
- Positive: inference + annotations; call-site narrowing; parallel aggregation.
- Negative: missing annotations at boundaries; privilege broadening; non-intersectable rows.

8.2 Capability contracts
- Input/output schema static success/failure; runtime guard insertion; typed errors.

8.3 Type/refinements
- Pre/post contract checks; return types; match exhaustiveness; destructuring with types.

8.4 Admission API
- Full pipeline golden tests producing PlanEnvelopeDraft and compiled IR.
- Cache hit/miss tests; parametric templates.

8.5 Integration
- GK policy gates; ORCH sandbox mapping; runtime guard execution; CC audit artifacts.

---

## 9) Migration Strategy

- Phase 1 (Warn): effect system optional, emit warnings; runtime guards inserted.
- Phase 2 (Enforce): deny-by-default for missing effect bounds at capability boundaries.
- Phase 3 (Strict): contracts required for external calls; semver and attestation enforced.

Provide code mod tooling to insert conservative ^{:effects ...} and ^{:resources ...} annotations.

---

## 10) Work Items (Files/Changes)

- rtfs_compiler/src/rtfs.pest
  - Metadata parse improvements; attach to fn/defn/call/step

- rtfs_compiler/src/ast.rs
  - Metadata on nodes; new types for effect rows (serde for reports)

- rtfs_compiler/src/parser/*
  - Metadata builder and attachment; error spans

- rtfs_compiler/src/runtime/effects.rs (new)
  - Effect kinds, params, normalization, subtyping

- rtfs_compiler/src/runtime/type_validator.rs
  - Integrate effects inference; refinement evaluation; runtime guard generation stubs

- rtfs_compiler/src/admission/mod.rs (new)
  - Orchestrates parse → validate → contracts → effects → IR → reports

- rtfs_compiler/src/runtime/capability_marketplace.rs
  - Contract loader API; digest verification (stub for tests)

- rtfs_compiler/src/ir/*
  - Carry effect/type annotations; guard-preserving passes

- rtfs_compiler/tests/*
  - New suites as per §8

---

## 11) Acceptance Criteria

- Compiler can admit a plan and produce Type/Effect/Contract reports, with IR or cache hit.
- Effect inference + subtyping validated; diagnostics with repair hints.
- Capability contract integration validates call sites; runtime guards inserted as needed.
- GK can sign Plan Envelope from compiler outputs; ORCH can map to runtime enforcement.
- Tests cover positive/negative paths; integration tests pass under strict mode.

---

## 12) Risks and Mitigations

- Over-conservative effects may reject valid plans → provide precise diagnostics and Arbiter repair hints.
- Performance overhead at admission → cache normalized AST/IR; incremental checks; short-circuit on unchanged envelopes.
- Contract drift vs providers → semver + attestation + revocation; telemetry feedback to tighten estimates.

---

Changelog
- v0.1 (incoming): Initial implementation plan for effect system and type system upgrades integrated with admission-time compilation and contracts.
