# RTFS 2.0 Incoming Spec: Intent Synthesis and Plan Translation (Arbiter → GK → ORCH)

Status: Proposed (specs-incoming)
Audience: Arbiter implementers, Governance Kernel, Orchestrator, Compiler/Runtime engineers
Related:
- Intent Graph: docs/ccos/specs/001-intent-graph.md
- Plans & Orchestration: docs/ccos/specs/002-plans-and-orchestration.md
- Causal Chain: docs/ccos/specs/003-causal-chain.md
- Effect System: docs/rtfs-2.0/specs-incoming/07-effect-system.md
- Concurrency & Determinism: docs/rtfs-2.0/specs-incoming/08-concurrency-and-determinism.md
- Capability Contracts: docs/rtfs-2.0/specs-incoming/09-capability-contracts.md
- IFC & Declassification: docs/rtfs-2.0/specs-incoming/11-information-flow-and-declassification.md
- Admission-time Compilation & Caching: docs/rtfs-2.0/specs-incoming/12-admission-time-compilation-and-caching.md
- Resource Estimation: docs/rtfs-2.0/specs-incoming/13-resource-estimation-and-envelopes.md
- Compiler Enhancement Plan: docs/rtfs-2.0/specs-incoming/14-compiler-enhancement-plan-effects-and-types.md
- Showcase Scenarios: docs/rtfs-2.0/specs-incoming/15-showcase-scenarios.md

This document specifies how a non-RTFS-fluent Arbiter progressively translates natural language goals into structured RTFS Intents and executable Plans with typed constraints, using compiler diagnostics and governance feedback to converge safely.

---

## 1) Pipeline Overview

1. Goal intake (NL)
   - User provides a natural language goal with optional hints (budget, locality, deadlines).
   - Arbiter extracts candidate constraints and objectives using a Goal→Intent heuristic (see §2).

2. Intent synthesis (RTFS Intent)
   - Arbiter produces an Intent object with:
     - goal (string), constraints (typed map), preferences, success-criteria (fn), policy hints.
     - Unknowns are represented as symbolic constraints with defaults and confidence scores.

3. Marketplace discovery
   - Arbiter queries capability contracts; selects candidate providers that can satisfy constraints.

4. Draft Plan generation (RTFS Plan)
   - Arbiter emits a first-pass Plan with (step)/(call) skeletons, effect/resource annotations (best-effort), and determinism hints.

5. Admission-time compile (GK + Compiler)
   - Type/effect/contract/IFC checks; resource envelope estimation; concurrency validation.
   - Diagnostics returned to Arbiter for auto-repair; GK may reject or request approvals.

6. Replanning/refinement loop
   - Arbiter iterates, narrowing effects/resources, pinning provider versions, adding guards/compensations until admitted.

7. Execution & learning
   - ORCH executes with runtime enforcement; Causal Chain captures artifacts.
   - Arbiter learns patterns from diagnostics and actuals to improve future synthesis.

---

## 2) From Goal to Typed Intent

2.1 Constraint extraction heuristics (examples)
- Budget: “under $X”, “budget Y” → :max_cost_usd number
- Time: “within N minutes/hours” → :max_time_ms number
- Locality: “EU-only”, “on-prem”, “no US” → :data_locality keyword/enum
- Privacy: “no PII”, “HIPAA/GDPR” → :privacy_policy keyword, :privacy_budget {:epsilon number}
- Risk/approvals: “requires sign-off”, “quorum” → :policy {:requires_approvals N|:quorum [...]}
- Determinism: “reproducible”, “deterministic” → :determinism {:required true}
- Notifications: channels/users → preference map
- Compliance: standards keywords → policy tags

2.2 Typed Intent template
- Arbiter uses canonical RTFS map schemas with refinements.
- Uncertain fields carry confidence and default bounds; GK can tighten or reject.

Example schema (conceptual)
```clojure
{:goal string
 :constraints
  {:max_cost_usd [:and number [:>= 0]]
   :max_time_ms  [:and number [:> 0]]
   :data_locality [:enum :eu_only :us_only :on_prem :global]
   :privacy_policy [:enum :gdpr :hipaa :none]
   :privacy_budget {:epsilon [:and number [:>= 0]]}
   :token_budget [:and number [:>= 0]]
   :risk_tier [:enum :low :medium :high]}
 :preferences {:notify [:vector string] :providers [:vector keyword]}
 :policy {:requires_approvals [:or number [:enum :quorum]]}
 :success-criteria (fn [result] boolean)}
```

2.3 Intent synthesis example (SRE)
```clojure
(intent
  :type :rtfs.core:v2.0:intent
  :intent-id "intent-sre-001"
  :goal "Mitigate a latency spike in service X without exceeding $100; EU-only operations; notify on-call for risky changes."
  :created-by "user:ops"
  :constraints {
    :max_cost_usd 100.0
    :data_locality :eu_only
    :max_time_ms 180000
    :risk_tier :high
  }
  :preferences {:notify ["#on-call-sre"]}
  :policy {:requires_approvals 2}
  :success-criteria (fn [res]
    (and (<= (:p95_latency_ms res) 120)
         (= (:mitigation_status res) :stable)))
  :status :active)
```

2.4 How typed constraints are guessed
- Arbiter uses pattern-based extractors + a small ontology of constraint types.
- Extractors emit: value, type, confidence, default bound if uncertain.
- Low confidence → add to Intent with conservative defaults and a “needs-confirmation” flag; GK can require human confirmation.

---

## 3) From Intent to Executable Plan (for a non-RTFS-fluent Arbiter)

3.1 Plan skeletonization
- Use templates per domain (SRE, analytics, trading, etc.) that map Intent constraints to plan structures.
- Each template contains:
  - step layout (sequential/parallel)
  - required guards and compensations
  - typical effect/resource annotations
  - policy annotations where risk is known high

3.2 Compiler-guided authoring loop
- Arbiter drafts plan → calls Admission API with “dry-admit” to collect diagnostics.
- Diagnostics include:
  - effect row mismatches (domains/methods too broad)
  - missing bounds → suggest ^{:resources ...} or runtime guards
  - contract shape mismatches → suggest schema conversions
  - IFC violations → suggest declassification with policy gate
- Arbiter applies automated repairs using a library of AST rewrite rules and re-submits.

3.3 Plan refinement strategies
- Provider pinning: choose EU-only providers; pick deterministic modes when required.
- Effect narrowing: restrict domains/methods/paths per diagnostics.
- Guard insertion: add pre/postconditions and runtime schema checks.
- Compensations: wrap effectful mutations with step.with-compensation.

3.4 Example plan (SRE) after refinement
See Showcase §1; includes effect/resource annotations, approvals, compensations.

---

## 4) Worked Samples (Intent + Plan Excerpts)

4.1 Regulated Analytics (HIPAA/GDPR)
Intent
```clojure
(intent
  :type :rtfs.core:v2.0:intent
  :intent-id "intent-reg-001"
  :goal "Aggregate outcomes for ClinicGroup-A, train a DP model, produce a physician report; no PII leaves org; EU GPUs; ε ≤ 1.0; budget $300."
  :created-by "user:data"
  :constraints {:max_cost_usd 300.0
                :data_locality :eu_only
                :privacy_policy :gdpr
                :privacy_budget {:epsilon 1.0}
                :token_budget 2e6}
  :preferences {:notify ["#analytics"]}
  :success-criteria (fn [r]
    (and (contains? r :model_metrics)
         (contains? r :report)
         (<= (:epsilon_used r) 1.0))) 
  :status :active)
```

Plan excerpt (see 15-showcase-scenarios §2)
- Uses :pii.redact, :ml.dp-train, :llm.local.summarize
- IFC enforcement and DLP scan
- Effects/resources declared; GK-approved.

4.2 Trading with Quorum
Intent
```clojure
(intent
  :type :rtfs.core:v2.0:intent
  :intent-id "intent-trade-001"
  :goal "Rebalance portfolio within 0.5% slippage and fees ≤ $200; require Strategy/Ethics/Risk quorum; market-hours only."
  :created-by "user:pm"
  :constraints {:max_cost_usd 200.0
                :max_slippage 0.5
                :risk_tier :high}
  :policy {:quorum [:arbiter.strategy :arbiter.ethics :arbiter.risk]}
  :success-criteria (fn [r]
    (and (<= (:fees_usd r) 200.0)
         (<= (:slippage_pct r) 0.5)
         (= (:status r) :executed))) 
  :status :active)
```

Plan excerpt (see 15-showcase-scenarios §3)
- Dry-run simulation step
- Idempotent order placement + compensation
- Time-window guard enforced by GK.

4.3 Supply-Chain Patch
Intent
```clojure
(intent
  :type :rtfs.core:v2.0:intent
  :intent-id "intent-sec-001"
  :goal "Patch CVE-XXXX, attest build, canary 10%, full rollout if SLOs stable, rollback on regression."
  :created-by "user:secops"
  :constraints {:risk_tier :high
                :max_time_ms 7200000}
  :success-criteria (fn [r]
    (and (= (:rollout_status r) :completed)
         (<= (:error_budget_delta r) 0))) 
  :status :active)
```

Plan excerpt (see 15-showcase-scenarios §4)
- SBOM scan → SLSA build → Sigstore sign → canary → progressive rollout
- Compensation rollback; Causal Chain anchors attestations.

---

## 5) Bootstrapping a Non-RTFS-Fluent Arbiter

5.1 Building blocks the Arbiter uses
- Intent templates with typed fields and examples per domain.
- Plan templates with parameterized sections and effect/resource “defaults”.
- AST rewrite library:
  - add-effects, narrow-network, add-guard, add-compensation, pin-provider, add-approval
- Compiler hints protocol:
  - Machine-readable diagnostics with suggested edits (JSON) to apply programmatically.

5.2 Progressive proficiency
- Phase 1: Arbiter uses coarse templates; high GK feedback; many iterations.
- Phase 2: Learns provider-specific constraints; fewer GK rejections; starts effect narrowing proactively.
- Phase 3: Produces near-admissible plans; GK acts mainly as backstop; cache reuse increases.

5.3 Safety rails during bootstrapping
- GK in “strict” mode: deny broad effects, require contracts and attestations.
- ORCH with aggressive enforcement: DLP, locality, budgets.
- Human-in-the-loop approvals for high-risk actions until Arbiter earns trust score.

---

## 6) How Typed Constraints Are Guessed and Refined

6.1 Extraction → Hypothesis
- NLP extractors map phrases to constraint schema; each with confidence score.
- Low-confidence → Intent includes conservative bound + needs-confirmation true.

6.2 Discovery → Feasibility bounds
- Marketplace contracts provide feasible ranges (e.g., token budgets, timeouts, locality).
- Arbiter intersects user constraints with provider capabilities to refine bounds.

6.3 Compiler diagnostics → Tightening
- Missing/misaligned bounds produce specific errors (e.g., “add :max_length 50k for field :text”).
- Arbiter applies edits via AST rewrites and re-submits for admission.

6.4 Runtime feedback → Learned defaults
- ORCH actuals vs envelope update priors for future extractions (e.g., typical costs/tokens for similar goals).

---

## 7) Interface Contracts (APIs between components)

7.1 Arbiter → Compiler (admission)
- Input: Intent, Plan draft
- Output: TypeReport, EffectReport, ContractReport, ResourceEnvelope draft, Diagnostics (machine-readable)

7.2 GK → Arbiter
- Approve/Reject with reasons; policy-required edits (e.g., require compensations)
- Approval requirements (quorum, human sign-offs)

7.3 ORCH → Causal Chain/Telemetry
- Step actions with effect profiles, debits, seeds, versions
- Guard outcomes; errors with typed variants matching contracts

---

## 8) Scenario Snippets with Intents

8.1 Add Intents to 15-showcase-scenarios.md
- Each scenario now should be paired with an Intent example and a Plan excerpt (as above). Cross-reference this spec for the synthesis rules and bootstrapping path.

---

## 9) Acceptance Criteria

- Arbiter can generate RTFS Intents from NL goals with typed constraint fields and confidence scores.
- For each showcase scenario, we have:
  - A concrete Intent example
  - A matching Plan excerpt that passes admission with minimal iterations
- Compiler diagnostics are machine-readable with suggested edits; Arbiter can auto-apply common rewrites.
- GK can enforce strict mode for bootstrapping; ORCH enforces runtime guards consistent with admitted envelope.

---

Changelog
- v0.1 (incoming): Defines end-to-end goal→intent→plan process, with examples, diagnostics-driven refinement, and bootstrapping path for non-RTFS-fluent Arbiters.
