# RTFS 2.0 Overview and Roadmap (Incoming)

Status: Proposed (specs-incoming)  
Audience: Architects, Compiler/Runtime engineers, CCOS integrators, Capability authors  
Related:
- Core language: docs/rtfs-2.0/specs/01-language-features.md
- Native type system: docs/rtfs-2.0/specs/05-native-type-system.md
- Effect system (incoming): docs/rtfs-2.0/specs-incoming/07-effect-system.md
- Concurrency & determinism (incoming): docs/rtfs-2.0/specs-incoming/08-concurrency-and-determinism.md
- Capability contracts (incoming): docs/rtfs-2.0/specs-incoming/09-capability-contracts.md
- Information flow & declassification (incoming): docs/rtfs-2.0/specs-incoming/11-information-flow-and-declassification.md
- CCOS Integration: docs/ccos/specs/000-ccos-architecture.md, 002-plans-and-orchestration.md, 003-causal-chain.md, 004-capabilities-and-marketplace.md, 014-step-special-form-design.md
- Reference implementation: rtfs_compiler/src (grammar, parser, AST, IR, runtime, validator)

---

## 1) Executive Summary

RTFS (Reason about The Fucking Spec) 2.0 is a homoiconic, AI-first kernel language designed to power governed autonomy in CCOS. It gives AIs a compact, learnable, and strongly-auditable medium to express intents, plans, and actions while keeping the core minimal and secure.

Why it exists
- Traditional prompt-and-script approaches lack verifiable structure, reproducibility, and governance.
- RTFS separates the “why/how/what happened” into distinct artifacts, all machine-checkable and cryptographically attestable.
- The kernel enforces purity-by-default: side effects only occur via a single primitive pathway that is observable and governable.

Main strengths
- Homoiconic S-expressions: perfect for AI synthesis, transformation, and formal reasoning.
- Hybrid/gradual typing with refinements: fast to generate, precise to validate.
- Minimal secure kernel: pure core with explicit, auditable effect boundaries via (call) and (step).
- CCOS-native: first-class integration with Intent Graph, Orchestrator, Governance Kernel, Capability Marketplace, and Causal Chain.
- Multiple execution strategies: AST runtime for stability, IR runtime for performance, IR-with-fallback for robustness.

---

## 2) Architectural Walk-through

High-level stack
1. Surface Language and Grammar
   - Pest grammar: rtfs_compiler/src/rtfs.pest
   - Expressions: lists, vectors, maps, literals (int, float, string, bool, nil, timestamp, uuid), keywords, symbols
   - Special forms: let, if, do, fn/def/defn, try/catch/finally, match, parallel, with-resource, log-step, discover-agents
   - Module system: module, import with :as and :only
   - Delegation metadata: ^:delegation hints for CCOS execution targets

2. AST and Type System
   - AST definitions: rtfs_compiler/src/ast.rs
   - Structural typing (TypeExpr): Primitive, Vector, Tuple, Map (with wildcard), Function (params/variadic/return), Resource, Union, Intersection, Literal, Any/Never, Array with shapes, Optional sugar
   - Refinement predicates: comparison, length, regex, range, collection, map predicates, and custom predicates
   - RTFS 2.0 objects: Intent, Plan, Action, Capability, Resource, Module as top-level constructs
   - Validator: rtfs_compiler/src/validator.rs validates object schemas and fields (e.g., :type keywords)

3. Parsing → AST → IR → Runtime
   - Parser: rtfs_compiler/src/parser (see parse_expression and helpers)
   - AST: high-fidelity representation for readability and transformation
   - IR: canonicalized form for optimization and execution
     - Converter/optimizer: rtfs_compiler/src/ir/*
     - Advantages: resolved bindings, constant folding, dead code elimination, analysis-friendly
   - Runtime strategies: rtfs_compiler/src/runtime/*
     - AST evaluator (TreeWalkingStrategy) for stability
     - IR Strategy for performance
     - IRWithFallbackStrategy for robustness
   - Development Tooling: REPL, test harness, benchmarks (rtfs_compiler/src/development_tooling.rs)

4. CCOS Integration
   - Governance Kernel, Orchestrator, Causal Chain: rtfs_compiler/src/ccos/*
   - Delegation Engine: runtime target selection
   - Capability Marketplace: discovery and invocation (rtfs_compiler/src/runtime/capability_marketplace.rs)
   - Intent Graph: persistent goal management (docs/ccos/specs/001-intent-graph.md)
   - Orchestration and Plans: docs/ccos/specs/002-plans-and-orchestration.md
   - Immutable auditing: Causal Chain (docs/ccos/specs/003-causal-chain.md)

Control flow primitives (selected)
- let / if / do: standard functional scaffolding
- fn/def/defn: functions with optional types and delegation hints; destructuring parameters
- try/catch/finally: typed error handling, pairs with contract-defined error variants
- match: pattern matching on literals, keywords, vectors, maps, and types with optional guards
- parallel: concurrent child steps, deterministic join semantics (see 08-concurrency-and-determinism)
- with-resource: resource acquisition/usage patterns (under governance)
- log-step: structured step logging to map to CCOS actions
- discover-agents: marketplace discovery scaffolding

Data structures and literals
- Rich literal set including domain-operational types (timestamp, uuid, resource handle)
- Maps with keyword/string/integer keys (grammar), structured Map types with optional fields
- Arrays with shape constraints for ML-friendly semantics
- Keywords and symbols as first-class

Modules and namespacing
- module ... with (:exports [...]) to define and expose APIs
- import ... with :as and :only for qualified and selective imports
- Namespaced identifiers and versioned namespaces in grammar

Delegation metadata
- ^:delegation {:local | :local-model "id" | :remote "id"} hints align with CCOS Delegation Engine (rtfs_compiler/src/ccos/delegation.rs)
- Non-binding hints; final routing resolved by policies and runtime

---

## 3) Language Features Deep Dive

3.1 Homoiconicity and transformation
- Program = data: S-expressions enable direct AST manipulation and macro-like transformations by the Arbiter
- Deterministic pretty-printing and normalization recommended for stable diffs and audits

3.2 Type system
- Structural, gradual with refinements (docs/rtfs-2.0/specs/05-native-type-system.md)
- Function types: param list (variadic supported), return type; helps plan synthesis and contract validation
- Union/Intersection: precise interface expression
- Refinements/predicates: precise constraints (e.g., [:and number [:> 0] [:<= 1000]])
- JSON Schema export (ast.rs: TypeExpr::to_json) to bridge to external validators

3.3 Error handling and contracts
- try/catch with typed patterns (CatchPattern::Type, Keyword, Symbol, Wildcard)
- Pairs naturally with Capability Contracts error variants (incoming 09-capability-contracts.md)
- Error reporting: enhanced diagnostics with spans, suggestions, contextual hints (rtfs_compiler/src/error_reporting.rs, parser_error_reporter.rs)

3.4 Concurrency and determinism
- step.parallel defined with deterministic join ordering and failure propagation semantics (incoming 08-concurrency-and-determinism.md)
- Determinism metadata: seeds/model versions/env digests for replay
- Branch seeds derivation defined for reproducibility

3.5 Effects and resources
- Core design: pure kernel; all effects via (call) and governed orchestration
- Incoming effect system (07-effect-system.md): type-level effect rows with subtyping and inference; resource constraints alongside effects
- Enables compile-time checks, GK admission, and ORCH enforcement (sandboxing, egress/DLP)

3.6 Information flow and declassification
- Incoming IFC spec (11-information-flow-and-declassification.md): labels (pii, secret, eu_only, export_restricted), taint propagation, explicit declassification
- Ties to data-locality policies and Causal Chain provenance

3.7 Capability ecosystem
- Marketplace-backed discovery and invocation (runtime/capability_marketplace.rs)
- Contracts (incoming 09-capability-contracts.md): typed inputs/outputs, effect/resource declarations, determinism/idempotency, errors, security attestation, semver
- Enables Arbiter to synthesize safe calls and compiler to validate shape and privileges

3.8 Modules, imports, and visibility
- module, :exports, :as aliasing, :only selection: build reusable libraries that are analyzable and governable
- Integration tests show module scenarios (rtfs_compiler/src/integration_tests.rs)

3.9 Runtimes and optimization
- AST runtime for correctness and traceable evaluation
- IR runtime for performance; optimizer supports constant folding, DCE, canonicalization (rtfs_compiler/src/ir/optimizer.rs)
- IR-with-fallback ensures robustness while IR matures

---

## 4) Implementation Pointers

- Grammar: rtfs_compiler/src/rtfs.pest
- Parser: rtfs_compiler/src/parser/*
- AST: rtfs_compiler/src/ast.rs
- Type validation: rtfs_compiler/src/runtime/type_validator.rs, ast.rs to_json
- IR and optimizer: rtfs_compiler/src/ir/*
- Runtimes: rtfs_compiler/src/runtime/* (evaluator.rs, ir_runtime.rs, secure_stdlib.rs)
- Capability marketplace: rtfs_compiler/src/runtime/capability_marketplace.rs
- CCOS integration: rtfs_compiler/src/ccos/*
- Dev tooling and REPL: rtfs_compiler/src/development_tooling.rs
- Tests: rtfs_compiler/tests/* and integration harnesses (rtfs_compiler/src/integration_tests.rs)

Key specs to read with code
- Language features: docs/rtfs-2.0/specs/01-language-features.md ↔ grammar/AST/parser
- Native type system: docs/rtfs-2.0/specs/05-native-type-system.md ↔ TypeExpr definitions and validators
- Effect system: specs-incoming/07-effect-system.md ↔ planned compiler/runtime enforcement points
- Concurrency/determinism: specs-incoming/08-concurrency-and-determinism.md ↔ parallel runtime + Causal Chain
- Capability contracts: specs-incoming/09-capability-contracts.md ↔ marketplace + compiler call-site validation
- CCOS orchestration/step logging: docs/ccos/specs/002-plans-and-orchestration.md, 014-step-special-form-design.md ↔ log-step, step mapping

---

## 5) Areas to Improve (Roadmap)

P0: Safety and Governance
1. Effect typing (compile-time)
   - Implement parsing/inference/normalization/subtyping of ^{:effects ...} per 07-effect-system.md
   - Admission: GK validates plan envelopes; runtime enforces least privilege
   - Code: extend parser metadata handling, type checker, runtime profiles

2. Resource constraints and budgets
   - Type-level ^{:resources {...}} with GK admission checks and ORCH accounting/hard-stops
   - map to cost/time/token budgets and data-locality

3. Capability contracts (compile-time + runtime)
   - Load contracts; validate call-site inputs/outputs, errors; merge effect/resource privileges; enforce semver constraints and attestation
   - Integrate with marketplace publish and revocation

4. Concurrency & determinism
   - Deterministic seeds, branch profiles, failure propagation, retries/idempotency, compensations
   - Emit structured Causal Chain events per-branch with seeds and versions

5. Information flow control and declassification
   - Label propagation in runtime; policy-enforced declassification special form
   - Provenance persisted in Causal Chain; enforced data-locality

P1: Developer/AI ergonomics
- Canonical pretty-printer/formatter for stable diffs and AST normalization
- Macro/pattern library for with-budget, with-locality, with-quorum, with-compensation
- Enhanced diagnostics: effect/resource mismatch suggestions; contract-driven hints

P2: Formalization and proofs
- Small-step semantics for core forms; preservation/progress notes
- Pre/postconditions on steps (contracts) with refinement checks
- Determinism proof sketch for parallel with fixed seeds and pinned versions

---

## 6) Why RTFS 2.0 is fit for AI-first governed autonomy

- Learnable by AIs: S-exprs, consistent patterns, minimal primitives
- Transformable: Arbiter can synthesize, rewrite, and verify swiftly
- Governable: explicit steps/calls, audit logging, capability contracts, effect/resource typing
- Reproducible: seeds, version pinning, Causal Chain anchoring
- Extensible: contracts and marketplace grow capabilities safely under policy

RTFS 2.0 provides the right substrate: a small, precise language that maximizes AI productivity while enabling human governance and cryptographic accountability. With effect typing, capability contracts, and concurrency/determinism finalized, it reaches production-grade robustness without sacrificing lightness or expressivity.

---
Changelog
- v0.1 (incoming): Initial overview aligned to current implementation and incoming specs.
