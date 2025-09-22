# RTFS 2.0 Language Guide (Incoming)

Status: Proposed (specs-incoming)  
Audience: Architects, CCOS integrators, Compiler/Runtime engineers, Capability authors  
Read with:
- Core features: docs/rtfs-2.0/specs/01-language-features.md
- Native type system: docs/rtfs-2.0/specs/05-native-type-system.md
- Effect system (incoming): docs/rtfs-2.0/specs-incoming/07-effect-system.md
- Concurrency & determinism (incoming): docs/rtfs-2.0/specs-incoming/08-concurrency-and-determinism.md
- Capability contracts (incoming): docs/rtfs-2.0/specs-incoming/09-capability-contracts.md
- Information flow & declassification (incoming): docs/rtfs-2.0/specs-incoming/11-information-flow-and-declassification.md
- Overview: docs/rtfs-2.0/specs-incoming/00-rtfs-2.0-overview.md

Implementation pointers:
- Grammar: rtfs_compiler/src/rtfs.pest
- AST: rtfs_compiler/src/ast.rs
- Parser: rtfs_compiler/src/parser/*
- IR + optimizer: rtfs_compiler/src/ir/*
- Runtimes: rtfs_compiler/src/runtime/*
- Validator (RTFS 2.0 objects): rtfs_compiler/src/validator.rs
- Error diagnostics: rtfs_compiler/src/error_reporting.rs, rtfs_compiler/src/parser_error_reporter.rs
- Capability marketplace: rtfs_compiler/src/runtime/capability_marketplace.rs
- CCOS integration: rtfs_compiler/src/ccos/*

---

## 1) What is RTFS 2.0 and why it exists

RTFS (Reason about The Fucking Spec) 2.0 is a homoiconic, AI-first kernel language for governed autonomy in CCOS:
- Homoiconic S-expressions: code is data; trivially synthesized and transformed by AI planners (Arbiters).
- Minimal and secure kernel: purity by default, with a single gateway to side-effects (call), and first-class orchestration (step/log-step).
- Hybrid typing with refinements: fast to generate, precise to validate; ideal for AI-authored programs verified by governance.
- CCOS-native artifacts: Intent, Plan, Action are first-class, signed, and auditable.

Reasons
- Move from unstructured prompts to verifiable, reproducible, governed execution.
- Separate the “why / how / what happened” with machine-checkable artifacts bound to a Causal Chain.
- Make safety and auditability architectural, not ad-hoc.

Main strengths
- Learnable by AIs: small set of regular forms, consistent patterns.
- Expressive: rich data structures, pattern matching, error handling, modules, and parallel orchestration.
- Governable: explicit steps, capability contracts, effect/resource typing (incoming), IFC (incoming).
- Efficient: IR runtime and optimizer; AST runtime for stability; fallback strategy for robustness.

---

## 2) Architecture at a glance

Surface -> AST -> IR -> Runtime -> CCOS

- Surface Language
  - Grammar (rtfs_compiler/src/rtfs.pest)
  - Expressions: literals, symbols, keywords, lists/vectors/maps, special forms, modules/imports
  - Delegation metadata for preferred execution targets (local/remote/model)

- AST (rtfs_compiler/src/ast.rs)
  - Complete representation of expressions, types, patterns, and RTFS 2.0 objects (Intent/Plan/Action/Capability/Resource/Module)
  - Structural type system with refinements
  - JSON Schema export for interoperability

- IR and Optimization (rtfs_compiler/src/ir/*)
  - Canonical form with binding resolution
  - Optimizations: constant folding, DCE, normalization
  - Analysis-friendly for execution planning and verification

- Runtimes (rtfs_compiler/src/runtime/*)
  - AST evaluator (stable, traceable)
  - IR runtime (high performance)
  - IR-with-fallback (robust transition path)
  - Secure stdlib and capability marketplace integration

- CCOS Integration (rtfs_compiler/src/ccos/*)
  - Governance Kernel: plan admission and policy checks
  - Orchestrator: deterministic execution with enforcement (sandbox/egress/DLP)
  - Causal Chain: immutable, signed audit trail
  - Intent Graph: living memory of goals/relationships

Reference overview: docs/rtfs-2.0/specs-incoming/00-rtfs-2.0-overview.md

---

## 3) Language features walkthrough

3.1 Core data and syntax
- Literals: integer, float (incl. special float), string, boolean, nil, timestamp, uuid, resource-handle
  - Grammar rules: literal, integer, float, special_float, timestamp, uuid, resource_handle in rtfs.pest
- Symbols and keywords: identifiers and namespaced forms; keywords can be qualified or versioned
- Collections: lists (function/call form), vectors, maps with keyword/string/integer keys
- Example:
```clojure
[42 "hello" :ok {:k 1}]
```

3.2 Control, binding, and composition
- let: destructuring patterns for vectors and maps; optional type annotations per binding
- if: optional else branch
- do: sequential composition
- Example:
```clojure
(let [{:as user :keys [id name]} {:id 1 :name "Ada"}]
  (do
    (log-step :info "User" name)
    (if (> 1 0) :ok :err)))
```
- Implementation:
  - AST: LetExpr/IfExpr/DoExpr in rtfs_compiler/src/ast.rs
  - Grammar: let_expr, if_expr, do_expr in rtfs_compiler/src/rtfs.pest

3.3 Functions and definitions
- fn: parameter list with patterns, optional return type, body; supports variadic args
- def/defn: global definitions and named functions
- Delegation metadata: ^:delegation (:local | :local-model "id" | :remote "id")
- Example:
```clojure
(defn greet
  ^:delegation :local
  [name: :string] :string
  (str "Hello " name))
```
- Implementation:
  - AST: FnExpr/DefExpr/DefnExpr, ParamDef
  - Grammar: fn_expr, def_expr, defn_expr; delegation_meta

3.4 Pattern matching
- match with guards, support for literal, keyword, symbol binding, type patterns, vector/map patterns, and :as
- Example:
```clojure
(match x
  0 :zero
  [:vector a b] (str "pair:" a "," b)
  (when (> x 10)) :big
  _ :other)
```
- Implementation:
  - AST: MatchExpr, MatchPattern, MapMatchEntry
  - Grammar: match_expr, match_pattern, WHEN

3.5 Errors and try/catch/finally
- try with multiple catch patterns (type/keyword/symbol/wildcard) and finally
- Designed to pair with typed capability error variants
- Example:
```clojure
(try
  (dangerous ...)
  (catch :error/network e (handle e))
  (catch MyError e (recover e))
  (finally (log-step :info "cleanup")))
```
- Implementation:
  - AST: TryCatchExpr, CatchClause, CatchPattern
  - Grammar: try_catch_expr, catch_clause, catch_keyword, finally_keyword
  - Diagnostics: error_reporting.rs, parser_error_reporter.rs

3.6 Parallelism and determinism (incoming)
- step.parallel executes branches concurrently with deterministic join semantics
- Determinism metadata: seed/model versions/environment digests for exact replay
- Example (spec-level):
```clojure
(step.parallel
  (step "A" (call ...))
  (step "B" (call ...)))
```
- Spec: docs/rtfs-2.0/specs-incoming/08-concurrency-and-determinism.md

3.7 Resource scoping
- with-resource: acquire and use typed resources under governance
```clojure
(with-resource [h :resource com.fs/Handle (open ...)]
  (do (use h) (close h)))
```
- Implementation:
  - AST: WithResourceExpr
  - Grammar: with_resource_expr

3.8 Logging and orchestration hooks
- log-step: structured logging that maps to CCOS step actions
```clojure
(log-step :info "starting step" {:k 1})
```
- Grammar: log_step_expr
- Spec: docs/ccos/specs/014-step-special-form-design.md

3.9 Modules and imports
- module with :exports, import with :as and :only
```clojure
(module my.lib
  (:exports [greet])
  (defn greet [n] (str "Hi " n)))

(import my.lib :as lib :only [greet])
```
- Grammar: module_definition, import_definition
- AST: ModuleDefinition, ModuleLevelDefinition

3.10 Types and refinements
- Structural type system with:
  - Primitives, Vector, Tuple, Map (entries + wildcard), Function (params/variadic/return), Resource, Union, Intersection, Literal types, Any/Never, Array with shapes, Optional sugar
  - Refinement predicates: comparison, length, regex, range, collection, map, custom
- JSON Schema export for cross-checks
- Spec: docs/rtfs-2.0/specs/05-native-type-system.md
- Implementation: TypeExpr in rtfs_compiler/src/ast.rs

3.11 RTFS 2.0 objects (domain artifacts)
- Intent, Plan, Action, Capability, Resource, Module as top-level forms
- Validated with schema-like checks (rtfs_compiler/src/validator.rs)
- Links to CCOS specs:
  - Intent Graph: docs/ccos/specs/001-intent-graph.md
  - Plans & Orchestration: docs/ccos/specs/002-plans-and-orchestration.md
  - Causal Chain: docs/ccos/specs/003-causal-chain.md
  - Marketplace: docs/ccos/specs/004-capabilities-and-marketplace.md

3.12 Effects and resources (incoming)
- Type-level effect rows with subtyping and inference; resource constraints for budget/time/locality
- Compiler: checks declared/inferred effects; GK: admission; ORCH: enforcement/sandbox/egress/DLP
- Spec: docs/rtfs-2.0/specs-incoming/07-effect-system.md

3.13 Capability contracts (incoming)
- Typed inputs/outputs, effects/resources, determinism/idempotency, error variants, security attestation, semver
- Compiler validates call sites; Marketplace enforces provenance and versioning rules
- Spec: docs/rtfs-2.0/specs-incoming/09-capability-contracts.md
- Runtime marketplace: rtfs_compiler/src/runtime/capability_marketplace.rs

3.14 Information flow control (incoming)
- Labels (pii, secret, eu_only, export_restricted), taint propagation across (call) boundaries
- Policy-gated (declassify ...) operations; provenance in Causal Chain
- Spec: docs/rtfs-2.0/specs-incoming/11-information-flow-and-declassification.md

---

## 4) Execution model and toolchain

4.1 Parsing and AST
- parse_expression, parse in rtfs_compiler/src/parser
- Enhanced error messages with source spans, hints, and delimiter analysis:
  - error_reporting.rs, parser_error_reporter.rs

4.2 IR conversion and optimization
- IrConverter and passes in rtfs_compiler/src/ir/*
- Benefits: binding resolution, canonical form, optimization-friendly, faster execution

4.3 Runtimes
- AST runtime: rtfs_compiler/src/runtime/evaluator.rs
- IR runtime: rtfs_compiler/src/runtime/ir_runtime.rs
- Hybrid strategy: IrWithFallback for resiliency
- Secure stdlib: rtfs_compiler/src/runtime/secure_stdlib.rs

4.4 Development tooling
- REPL + test harness + benchmarks: rtfs_compiler/src/development_tooling.rs
- Extensive tests:
  - Feature tests: rtfs_compiler/tests/rtfs_files/features/*
  - Type system: rtfs_compiler/tests/type_system_tests.rs
  - Capability tests: rtfs_compiler/tests/capability_integration_tests.rs
  - Runtime integration: rtfs_compiler/tests/integration_tests.rs

---

## 5) Examples (concise)

5.1 Function and use
```clojure
(defn sum3 [a: :int b: :int & c: :int] :int
  (+ a b c))

(sum3 1 2 3)
```

5.2 Match with types and guards
```clojure
(match data
  [:map [:id :int] [:name :string?]]
  (when (> (count (:name data)) 3))
  :ok
  _ :reject)
```

5.3 Parallel orchestration (spec)
```clojure
(plan
  ^{:determinism {:seed "0x1234"}}
  :program
  (step.parallel
    (step "A" (call :svc.a {:x 1}))
    (step "B" (call :svc.b {:y 2}))))
```

5.4 Capability call with typed input/output (incoming contracts)
```clojure
(defn create-invoice
  ^{:effects [[:network {:domains ["billing.vendor.com"] :methods [:POST]}]]}
  [req]
  (try
    (call :com.vendor.billing:v2.1.0
          (merge req {:idempotency_key (hash req)}))
    (catch :error/network e {:status :retry})
    (catch :policy/forbidden e {:status :halt})))
```

---

## 6) What to improve (prioritized roadmap)

P0 — Safety and governance
- Effect system
  - Implement compiler parsing/inference/subtyping of ^{:effects ...} on fn/defn/plan/step/call
  - GK admission: effect/resource envelope validation; CC logging
  - ORCH runtime: least-privilege sandbox per step; egress/DLP; deterministic seeding

- Resource constraints
  - ^{:resources {...}} for cost/time/tokens/data_locality/privacy_budget
  - GK pre-commit checks; ORCH continuous accounting with fail-safe stops

- Capability contracts
  - Load contracts; validate inputs/outputs and error variants; merge effect/resource privileges
  - Marketplace: attestation (Sigstore/TUF), SBOM/provenance, semver enforcement and revocation

- Concurrency + determinism
  - Deterministic seeds, branch effect/resource rows, failure propagation, retries/idempotency, compensations
  - Emit detailed Causal Chain entries per branch (seeds, versions, debits)

- Information flow control
  - Labels and taint propagation in runtime; (declassify ...) under policy
  - Data-locality enforcement and provenance

P1 — Ergonomics
- Canonical formatter/normalizer for stable diffs
- Macro/pattern library: with-budget, with-locality, with-quorum, with-compensation
- Diagnostics aligned with contracts and effect policies; guided fix-its

P2 — Formalization
- Small-step semantics for core; preservation/progress notes
- Pre/postconditions on steps using refinement checks
- Determinism proof sketch for parallel under fixed seeds and pinned versions

---

## 7) Cross-reference map (where to read the code/specs)

- Grammar and parsing
  - rtfs_compiler/src/rtfs.pest
  - rtfs_compiler/src/parser/*

- AST and types
  - rtfs_compiler/src/ast.rs
  - docs/rtfs-2.0/specs/05-native-type-system.md

- IR and optimizer
  - rtfs_compiler/src/ir/*
  - rtfs_compiler/src/ir/optimizer.rs

- Runtimes and stdlib
  - rtfs_compiler/src/runtime/evaluator.rs
  - rtfs_compiler/src/runtime/ir_runtime.rs
  - rtfs_compiler/src/runtime/secure_stdlib.rs

- CCOS integration
  - rtfs_compiler/src/ccos/*
  - docs/ccos/specs/001-intent-graph.md
  - docs/ccos/specs/002-plans-and-orchestration.md
  - docs/ccos/specs/003-causal-chain.md
  - docs/ccos/specs/004-capabilities-and-marketplace.md
  - docs/ccos/specs/014-step-special-form-design.md

- Governance and marketplace (incoming)
  - docs/rtfs-2.0/specs-incoming/07-effect-system.md
  - docs/rtfs-2.0/specs-incoming/08-concurrency-and-determinism.md
  - docs/rtfs-2.0/specs-incoming/09-capability-contracts.md
  - docs/rtfs-2.0/specs-incoming/11-information-flow-and-declassification.md

- Tests
  - rtfs_compiler/tests/rtfs_files/features/*
  - rtfs_compiler/tests/type_system_tests.rs
  - rtfs_compiler/tests/capability_integration_tests.rs
  - rtfs_compiler/tests/integration_tests.rs

---

## 8) Closing

RTFS 2.0 is a compact yet expressive language designed for AI authorship and human governance. It pairs homoiconicity and gradual/refined typing with a secure orchestration model. With effect typing, capability contracts, concurrency/determinism, and IFC finalized, RTFS becomes a production-grade kernel for trustworthy autonomous agents while remaining lightweight, efficient, and easy for AIs to learn and use.

Changelog
- v0.1 (incoming): Initial language guide aligned with current implementation and incoming specs.
