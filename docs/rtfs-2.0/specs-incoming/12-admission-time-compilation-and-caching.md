# RTFS 2.0 Incoming Spec: Admission-Time Compilation and Caching

Status: Proposed (specs-incoming)  
Audience: Arbiter implementers, Governance Kernel, Orchestrator, Compiler/Runtime engineers  
Related:
- Language Overview: docs/rtfs-2.0/specs-incoming/00-rtfs-2.0-overview.md
- Language Guide: docs/rtfs-2.0/specs-incoming/02-language-guide.md
- Effect System: docs/rtfs-2.0/specs-incoming/07-effect-system.md
- Concurrency & Determinism: docs/rtfs-2.0/specs-incoming/08-concurrency-and-determinism.md
- Capability Contracts: docs/rtfs-2.0/specs-incoming/09-capability-contracts.md
- Information Flow & Declassification: docs/rtfs-2.0/specs-incoming/11-information-flow-and-declassification.md
- CCOS Orchestration: docs/ccos/specs/002-plans-and-orchestration.md
- Causal Chain: docs/ccos/specs/003-causal-chain.md
- Implementation: rtfs_compiler/src (rtfs.pest, ast.rs, parser/*, ir/*, runtime/*, validator.rs)

---

## 1) Purpose

In CCOS, plans are generated at runtime by the Arbiter from goals/intents. RTFS “compile-time” must therefore occur just-in-time, at Governance admission, before any side effects. This document defines:
- What “compile-time” means in an AI-first, runtime-generated plan model
- The admission-time validation pipeline for types, effects, contracts, determinism, and IFC
- JIT compilation to IR and execution model
- Caching and reuse of admitted/compiled plans to reduce latency and cost

---

## 2) Definitions

- Admission-Time Static Analysis (ATSA): A pre-execution validation phase where the GK uses the compiler to parse, type-check, effect-check, and contract-validate a plan. If successful, the plan is admitted for execution.
- JIT Compilation: Converting the admitted AST to IR with optimization immediately prior to execution.
- Plan Envelope: The admitted set of constraints (effects, resources, determinism settings, capability versions/contracts, policy decisions) that must hold at runtime.
- Plan Cache: A content-addressed store of normalized ASTs, admitted envelopes, and compiled IR artifacts for reuse.

---

## 3) End-to-End Flow

1) Arbiter synthesizes Plan from Intent/Goal
- Produces RTFS program + metadata (policy hints, determinism preferences, budgets).
- References capability versions and providers (or a restricted choice set).

2) Admission-Time Static Analysis (Governance Kernel + Compiler)
- Parse → AST (rtfs_compiler/src/parser, rtfs_compiler/src/ast.rs)
- Type check (structural + refinements) where values are known; insert runtime checks where unknown (rtfs_compiler/src/runtime/type_validator.rs)
- Effect system (07): infer effect rows; validate subtyping against annotations and GK policies
- Capability contracts (09): validate call inputs statically when known; otherwise insert guards; forbid privilege broadening vs contract; ensure version/semver compliance
- Concurrency & determinism (08): validate step.parallel semantics, seeds, timeouts/retries/idempotency
- Information flow & declassification (11): check label propagation/declassify usage against policy
- If all checks pass, GK generates and signs the Plan Envelope and emits PlanApproved to the Causal Chain (003)

3) JIT compilation to IR and execution (Orchestrator)
- Convert AST → IR (rtfs_compiler/src/ir/converter.rs), optimize (rtfs_compiler/src/ir/optimizer.rs)
- Execute on IR runtime (rtfs_compiler/src/runtime/ir_runtime.rs); fallback to AST runtime if necessary
- Enforce Plan Envelope at runtime: least-privilege sandbox profiles, egress/DLP, budgets, seeds, idempotency
- Emit per-step actions and resource debits to the Causal Chain

4) Caching for reuse
- Store normalized AST, Plan Envelope, and compiled IR keyed by content hashes (see §6)
- On future plan proposals that normalize to the same hash and policy version, reuse the admitted envelope + compiled IR directly (or re-validate deltas for parametric templates)

---

## 4) What “Compile-Time” Means in RTFS/CCOS

- Not AOT: there is no human-driven ahead-of-time compilation. Plans are AI-generated at runtime.
- Admission-time compile: the compiler provides static guarantees exactly when governance needs them—before any effectful execution.
- Hybrid guarantees: static where possible (types/effects/contracts), dynamic where necessary (schema checks for unknown data), with clear boundaries and Causal Chain records.

---

## 5) Admission-Time Validation: Scope and Guarantees

5.1 Types and Refinements
- Structural types: check map/vector/tuple/resource/function shapes
- Refinements: evaluate predicates on known literals; otherwise lift to runtime guards
- Match/try/catch shapes: ensure exhaustiveness where feasible; validate typed error branches against contracts

5.2 Effect System (07)
- Infer effect rows from body and capability contracts; normalize and validate subtyping against annotations
- Deny-by-default for unknown/inferable effect boundaries
- Map admitted effects to GK policies and runtime enforcement profiles

5.3 Capability Contracts (09)
- Inputs: validate statically when bound; insert runtime schema checks when deferred
- Outputs: require runtime validation; typed-catch shapes anchored to contract error variants
- Contracts must be signed/attested; semver rules enforced; revocations respected

5.4 Concurrency & Determinism (08)
- Validate step.parallel semantics: deterministic join, failure propagation, retry policies, timeouts
- Determinism metadata: seeds, model/capability versions, env digests recorded in envelope

5.5 Information Flow & Declassification (11)
- Label propagation checks; ensure no forbidden flows (e.g., pii → non-compliant :network)
- Declassify steps must be policy-gated; Causal Chain records rationales

Result: Plan Envelope signed by GK, containing:
- Admitted effects/resources
- Bound contracts and versions
- Determinism settings (seed, versions, env digests)
- IFC policy decisions
- Required approvals and risk-tier notes

---

## 6) Normalization and Caching

6.1 Normalization (to maximize cache hits)
- Canonical pretty-printing and AST normalization (ordering, whitespace, metadata ordering)
- Inline resolution of macro-equivalent forms (where applicable)
- Version pinning of capabilities and models
- Plan parameterization markers (see 6.3)

6.2 Cache Keys
- Hash over:
  - Normalized AST
  - Capability contract digests and versions
  - Effect/resource envelope (normalized)
  - Policy/constitution version and GK identity
  - Determinism metadata (seed strategy, env digest)
- Separate keys for (A) Admitted Envelope and (B) Compiled IR

6.3 Parametric Plan Templates
- Allow parameter markers (e.g., values to be bound at runtime) with typed constraints and effect-invariant guarantees
- Admission-time instantiation validates only deltas (parameter bounds) if the invariant envelope is unchanged
- Useful for repetitive workflows (e.g., “fetch+analyze+notify” with varying topics)

6.4 Reuse Policy
- If a proposed plan normalizes to a cached key, reuse the admitted envelope + compiled IR
- If only parameters differ within admitted constraints, fast-path admission (delta-check + IR reuse)
- If contract versions/effects/policies changed, recompute and re-admit

---

## 7) Runtime Enforcement and Dynamic Checks

- Orchestrator enforces least-privilege profiles per step derived from admitted effects
- Capability calls:
  - Validate dynamic inputs/outputs against contract schemas
  - Enforce IFC labels and data locality at egress via policy-controlled proxies
  - Apply idempotency keys and retries; execute compensations on failure
- Telemetry and Causal Chain:
  - Emit PlanStepStarted/Completed with seeds, versions, profiles, debits, hashes
  - Support deterministic replay where possible

---

## 8) Developer/AI Ergonomics

- Compiler diagnostics at admission-time guide the Arbiter to refine/repair plans quickly
- A canonical formatter/normalizer stabilizes plan text → AST hashing for reliable caching
- Library of plan templates with typed parameters and invariant envelopes for common tasks

---

## 9) Security and Governance Notes

- Admission-time is the single control point where governance and static guarantees meet
- Deny-by-default for missing effect bounds, unsigned contracts, or ambiguous provider choices
- Break-glass paths must be explicitly logged with rationale and risk elevation in the envelope

---

## 10) Implementation Pointers

- Parsing/AST: rtfs_compiler/src/rtfs.pest, rtfs_compiler/src/ast.rs, rtfs_compiler/src/parser/*
- Type checks: rtfs_compiler/src/runtime/type_validator.rs, TypeExpr::to_json for dynamic guards
- Effect inference/checking: integrate per 07-effect-system.md in compiler + GK
- Contracts: load/validate per 09-capability-contracts.md; attach digests to envelope
- IR/JIT: rtfs_compiler/src/ir/converter.rs, rtfs_compiler/src/ir/optimizer.rs, runtime/ir_runtime.rs
- Caching: Plan Archive keyed by normalization; store admitted envelopes + compiled IR
- Runtime enforcement: runtime/host.rs, runtime/capability_marketplace.rs, runtime/secure_stdlib.rs
- CCOS glue: docs/ccos/specs/002-plans-and-orchestration.md, 003-causal-chain.md; rtfs_compiler/src/ccos/*

---

## 11) Acceptance Criteria

- Compiler exposes an admission API to:
  - Parse + normalize + hash plans
  - Perform type/effect/contract/concurrency/IFC checks
  - Produce a signed Plan Envelope and compiled IR (or a cache hit)
- Governance Kernel:
  - Validates policies and approvals; records PlanApproved with envelope hash in Causal Chain
- Orchestrator:
  - Executes with runtime enforcement consistent with the envelope; emits step actions and telemetry
- Cache:
  - Content-addressed; supports parametric templates; safe reuse with policy/contract/version awareness

---

## 12) FAQ

Q: Is this “compiling” in the traditional sense?  
A: It is admission-time compilation. We generate static guarantees and IR immediately before execution. There is no offline AOT step; everything is governed and cached just-in-time.

Q: How do we handle unknown values at admission?  
A: Statically validate shapes and effects; insert runtime guards for late-bound data. GK ensures effects/resources/contracts are fixed; dynamic values must pass schema checks at call time.

Q: Can we skip admission checks for cached plans?  
A: Only if the normalized AST, contracts, effects/resources, determinism metadata, and policy versions match the cached envelope. Otherwise re-admit or delta-check for parametric templates.

---

Changelog
- v0.1 (incoming): Defines admission-time compilation, plan envelopes, and caching strategy for runtime-generated plans.