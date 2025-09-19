# RTFS 2.0 Incoming Spec: Effect System

Status: Proposed (specs-incoming)  
Audience: RTFS compiler/runtime, Governance Kernel, Orchestrator, Capability Marketplace  
Related: docs/rtfs-2.0/specs/01-language-features.md, 05-native-type-system.md, docs/ccos/specs/002-plans-and-orchestration.md, 004-capabilities-and-marketplace.md, 014-step-special-form-design.md

## 1. Overview

This document proposes a first-class Effect System for RTFS 2.0 that makes side effects explicit, typed, and enforceable. It enables:
- Compile-time validation of program effects
- Admission-time policy checks by the Governance Kernel (GK)
- Runtime enforcement by the Orchestrator (ORCH) via sandbox/egress/DLP
- Clear contracts for capabilities in the Marketplace

Goals
- Make all effects explicit at the language/type level
- Support subtyping/containment for effect scopes (e.g., narrower domain allowlists)
- Provide effect inference where safe, require annotations where necessary
- Integrate with plan admission policies and runtime isolation/egress control
- Preserve purity in the core language: only (call ...) can perform side effects

Non-Goals
- Adding ad-hoc effectful stdlib primitives (effects must go through call)
- Replacing resource constraints (they are tracked separately and complement effects)

## 2. Terminology

Effect: A typed description of a side-effect class (e.g., network, fs) with optional parameters (e.g., domains, methods).  
Effect Row: A vector/list of effect items forming the effect set for a function/step/plan.  
Effect Item: A tuple of kind and parameters, e.g. [:network {:domains ["api.example.com"] :methods [:GET]}].  
Purity: Absence of effectful behavior. Pure expressions have empty effect rows.

## 3. Effect Vocabulary (Initial Closed Set)

The following effect kinds form the initial closed vocabulary. Future kinds must be introduced via language/runtime versioning and GK policy updates.

- :network {:domains [string]} {:methods [:GET|:POST|...]} {:mTLS bool} {:ports [int]} {:ip_ranges [cidr]} {:protocols [:http|:grpc|...]}
- :fs {:paths [string]} {:modes [:read|:write|:append|:exec]}
- :process {:spawn bool} {:signals [:TERM :KILL ...]}
- :ipc {:channels [string|uri]}
- :clock [:read]
- :random [:read]
- :llm {:models [string]} {:max_tokens number} {:tool_use bool}
- :gpu {:max_mem string} {:compute_caps [string]}
- :device {:kinds [:camera :mic :usb ...]}
- :ui {:automation [:keyboard :pointer :window]}

Notes
- Parameters refine the authority granted to an effect.
- Effect kinds are versioned semantically with RTFS. Extensions require spec changes.

## 4. Syntax and Annotations

Effects are carried as metadata on functions, steps, calls, and plans. Metadata syntax follows RTFS conventions.

Examples
```clojure
; Function with explicit effects/resources
(defn fetch-eu
  ^{:effects  [[:network {:domains ["eu.api.example.com"] :methods [:GET]}]]
    :resources {:data_locality :eu_only :max_time_ms 5000}}
  [product-id]
  (call :com.example.eu:v1.specs {:product product-id}))

; Step-level annotation (contracts shown for context; see separate spec)
(step "Notify Team"
  ^{:effects [[:network {:domains ["slack.com"] :methods [:POST]}]]}
  (call :com.collaboration:v1.slack-post {:channel "#ops" :msg "done"}))

; Plan-level envelope
(plan
  ^{:effects   [[:network {:domains ["eu.api.example.com" "slack.com"]}]]
    :resources {:max_cost_usd 50.0 :token_budget 2000000 :data_locality :eu_only}}
  :program
  (do
    (step "Fetch" (fetch-eu "prod-123"))
    (step "Notify" (call :com.collaboration:v1.slack-post {:channel "#ops" :msg "done"}))))
```

Rules
- If a construct is unannotated, the compiler attempts inference. If inference is uncertain around external calls, an explicit annotation is required.
- Pure fragments (no calls) are inferred as pure (empty effect row).

## 5. Type System Semantics

Effect Row
- A construct’s effect row is a vector of effect items.
- Effect rows compose by union with coalescing (see normalization below).

Normalization
- Duplicate effect kinds merge by intersecting parameters to the most restrictive form where well-defined.
- Example: [:network {:domains ["a.com"]}] + [:network {:domains ["a.com" "b.com"]}] ⇒ [:network {:domains ["a.com"]}]
- If parameter intersection is empty or undefined, compilation fails with a precise diagnostic.

Subtyping (≤)
- Effect subtyping is containment-based and covariant in restrictions:
  - [:network {:domains [a.com]}] ≤ [:network {:domains [a.com b.com]}]
  - [:fs {:paths ["/tmp"] :modes [:read]}] ≤ [:fs {:paths ["/tmp" "/var/tmp"] :modes [:read :write]}]
- For rows: E1 ≤ E2 iff for every kind in E1 there exists a corresponding kind in E2 such that itemE1 ≤ itemE2.
- Missing kinds: An effect present in E1 but not in E2 breaks subtyping.

Row Polymorphism
- Functions may be generic over an open effect row (e.g., “pure + α”). Concretization occurs at call sites by unification and containment checks.
- Compiler error surfaces when a row variable cannot be instantiated under policy or declared bounds.

## 6. Inference and Checking

Inference
- Pure fragments: inferred as [] (empty effect row).
- (call ...) nodes: require a capability contract (see capability contracts spec) to provide effects; effects propagate upward.
- let/if/match/do: effects are unions of child expressions; parallel branches union and then normalize.

Explicit Annotations
- Required at capability boundaries if the capability lacks a declared contract or the compiler cannot determine narrowed scopes.
- Annotations on a higher-level construct (e.g., plan) are checked to be a supertype of the composed effects of the body.

Compiler Diagnostics
- On violation, the compiler emits a structured error including:
  - The inferred effect row
  - The declared/allowed effect row
  - The specific item/kind failing subtyping
  - Suggestions to narrow domains/methods or refactor calls

## 7. Resources: Complementary but Separate

Resources are tracked in parallel to effects:
```clojure
^{:resources {:max_cost_usd 50.0 :max_time_ms 30000 :token_budget 2e6
              :data_locality :eu_only :privacy_budget {:epsilon 1.0}}}
```
- Compiler validates shape and refinements (leveraging existing refinement types).
- GK enforces at admission (pre-commit checks vs policy).
- ORCH accounts at runtime; exceeding ceilings triggers fail-safe and audit.

## 8. Governance Kernel (GK) Integration

Admission Checks
- Verify that the plan’s effect row conforms to organization/project/intent policies:
  - Example policies:
    - Forbid :random in high-determinism plans unless seeded deterministically
    - Deny :network to non-EU domains when :data_locality is :eu_only
    - Require human approval for :fs :write outside /tmp
- Verify resource ceilings are within allowed bounds.
- Record the admitted effect/resource envelope into the Causal Chain as part of PlanStarted/PlanApproved actions.

Policy Annotations
- GK interprets metadata hints (e.g., ^{:policy {:risk_tier :high :requires_approvals 2}}) to select the appropriate admission path (human in the loop, quorum, break-glass).

## 9. Orchestrator (ORCH) Enforcement

Runtime Mapping
- Effects → sandbox profiles:
  - :network → policy-controlled egress proxy with DNS pinning, domain allowlists, TLS pinning/mTLS, DLP
  - :fs → ephemeral filesystem roots; path allowlists; teardown on step completion
  - :random/:clock → controlled sources; bind to deterministic seeds when required
  - :llm/:gpu → quota tracking; model/version pinning
- Enforce least privilege per step:
  - Each (step ...) executes under a profile derived from the step’s effect subset (not the entire plan).
  - Credentials are short-lived and scoped to the step profile; destroyed on completion.

Audit
- Emit per-step effect profile, resource debits, and any policy downgrades/declassifications into the Causal Chain.

## 10. Capabilities and Contracts

Capabilities in the Marketplace MUST declare their effects and resources (see “Capability Contracts” incoming spec). Call sites inherit/compose from capability declarations; additional narrowing via annotations at the call site is allowed and checked.

Example capability declaration (non-normative preview)
```clojure
(capability :com.vendor.billing:v2
  {:input  {:amount [:and number [:> 0]] :currency [:enum "USD" "EUR"]}
   {:output {:invoice_id string :status [:enum "ok" "pending" "error"]}}
   :effects  [[:network {:domains ["billing.vendor.com"] :methods [:POST]}]]
   :resources {:max_time_ms 15000}
   :deterministic? false})
```

## 11. Determinism and Replay

Determinism Metadata
```clojure
^{:determinism {:seed "0xDEADBEEF" :model_ver "v1.2.3" :env_digest "..."}}
```
- GK may require determinism for certain intents.
- ORCH records seeds/model versions/capability versions to support exact replay.

Interactions with Effects
- :random requires deterministic seeding or is forbidden under strict policies.
- :llm may be declared deterministic only under specific runtime/mode; otherwise recorded as non-deterministic.

## 12. Concurrency

Parallel Composition
- step.parallel unions effect rows of branches; runtime profiles are applied per-branch.
- Deterministic join semantics and happens-before guarantees are specified in the “Concurrency and Determinism” incoming spec.
- Resource contention and partitioning are validated at admission-time (GK) and enforced at runtime (ORCH).

## 13. Backward Compatibility and Migration

- Default assumption for unannotated legacy code: compiler attempts inference; if effectful boundaries cannot be inferred safely, emit a hard error with actionable hints.
- Transitional mode (feature flag): allow soft-warnings for selected projects; GK can still enforce admission-time gates strictly.
- Code mod tooling: provide an automatic annotator that inserts conservative effect/resource annotations based on static analysis and capability contracts.

## 14. Security Considerations

- Deny-by-default: missing annotations at effectful boundaries fail admission.
- Narrowing over time: policies should prefer more restrictive parameters (domains/paths) and phase out wildcards.
- DLP and redaction: ORCH must filter outbound payloads when policies require (e.g., pii, export_restricted labels).

## 15. Open Questions (for this incoming stage)

- Exact normalization behavior when parameters cannot be intersected (conflict vs. union?): current stance is “fail closed”.
- Row polymorphism ergonomics: how much implicit generalization is acceptable for AI-authored code?
- Capability-level optional effects (e.g., feature flags): how to express conditional effects cleanly?

## 16. Acceptance Criteria

Compiler
- Parses ^{:effects ...} metadata on defn/fn/plan/step/call.
- Computes effect rows with inference; validates subtyping and normalization.
- Emits structured diagnostics on violations.

Governance Kernel
- Validates plan-level effect/resource envelopes against policy.
- Supports risk-tiered paths (approvals/quorum/break-glass).
- Emits admission results to the Causal Chain.

Orchestrator
- Derives sandbox profiles per step from effect subsets.
- Enforces egress/DLP policies for :network; ephemeral FS for :fs; deterministic seeding for :random where required.
- Accounts resources and logs to the Causal Chain.

Marketplace
- Requires capability contracts to declare effects/resources.
- Blocks publication of unsigned/unattested capabilities (see attestation specs).

## 17. Examples

Narrowing at call site
```clojure
(defn post-to-slack
  ^{:effects [[:network {:domains ["slack.com"] :methods [:POST]}]]}
  [msg]
  (call :com.collaboration:v1.slack-post {:channel "#ops" :msg msg}))

; Plan narrows to a.com + slack.com
(plan
  ^{:effects [[:network {:domains ["a.com" "slack.com"]}]]}
  :program
  (do
    (step "Fetch" (call :com.data:v1.get {:url "https://a.com/x"}))
    (step "Notify" (post-to-slack "done"))))
```

Violation (compiler error)
```clojure
(defn fetch-wild
  ^{:effects [[:network {:domains ["*"]}]]}   ; too broad
  [u] (call :com.http:get {:url u}))
; ERROR: Declared domains ["*"] are not permitted by policy; suggest enumerating allowed domains or using a marketplace capability with a constrained domain list.
```

---
This is an incoming specification intended for review and iteration. Once stabilized, it should be merged into the core RTFS 2.0 specs and cross-referenced from Plans & Orchestration and Capability Marketplace documents.
