# CCOS/RTFS Showcase Scenarios (Incoming)

Status: Proposed (specs-incoming)
Audience: Readers of README, architects, product stakeholders
Related:
- Overview: docs/rtfs-2.0/specs-incoming/00-rtfs-2.0-overview.md
- Language Guide: docs/rtfs-2.0/specs-incoming/02-language-guide.md
- Effect System: docs/rtfs-2.0/specs-incoming/07-effect-system.md
- Concurrency & Determinism: docs/rtfs-2.0/specs-incoming/08-concurrency-and-determinism.md
- Capability Contracts: docs/rtfs-2.0/specs-incoming/09-capability-contracts.md
- Admission-time Compilation: docs/rtfs-2.0/specs-incoming/12-admission-time-compilation-and-caching.md
- Resource Estimation: docs/rtfs-2.0/specs-incoming/13-resource-estimation-and-envelopes.md
- Compiler Enhancement Plan: docs/rtfs-2.0/specs-incoming/14-compiler-enhancement-plan-effects-and-types.md

This document contains longer-form narratives that correspond to the one-line scenarios listed in the README. Each scenario references CCOS/RTFS features, governance policies, and example RTFS snippets that illustrate how the architecture works end-to-end.

---

## 1) Autonomous Incident Response (SRE Self-Healing)

Goal
“Detect and mitigate a production latency spike in service X; stay under $100 budget; keep all operations within EU; notify on-call for high-risk changes.”

Outline
1. Detect incident via telemetry (capability: :telemetry.query).
2. Generate mitigation options with compensations:
   - roll back last config (compensation: re-apply)
   - temporary autoscale (compensation: scale-in)
   - partial traffic shift (compensation: revert routes)
3. Enforce approvals for traffic shifts > 20%.
4. Execute in parallel with bounded budgets.
5. Produce postmortem pack anchored to Causal Chain.

Key features
- Effect/resource typing for safe ops
- step.parallel orchestration, deterministic join
- Human-in-the-loop approvals (GK)
- Compensations (saga pattern)
- Immutable audit via Causal Chain

RTFS sketch
```clojure
(plan
  ^{:policy {:risk_tier :high :requires_approvals 2}
    :resources {:max_cost_usd 100 :data_locality :eu_only}}
  :program
  (let [incident (step "Query SLO breach"
                   (call :com.telemetry.eu:v1.query {:service "X" :window "10m"}))]
    (step.parallel
      (step.with-compensation
        (step "Rollback last config"
          (call :com.config:v1.rollback {:service "X" :version (:last_good incident)}))
        (step "Re-apply config"
          (call :com.config:v1.apply {:service "X" :version (:current incident)})))
      (step.with-compensation
        (step "Autoscale up"
          (call :com.cluster:v1.scale {:service "X" :replicas (+ (:replicas incident) 3)}))
        (step "Scale in"
          (call :com.cluster:v1.scale {:service "X" :replicas (:replicas incident)})))
      (step "Shift 15% traffic"
        ^{:policy {:requires_approvals 1}}
        (call :com.gateway:v1.traffic-shift {:service "X" :percent 15})))
    (step "Validate" (call :com.synthetics:v1.run {:service "X"}))))
```

---

## 2) Regulated Analytics with IFC/DP (HIPAA/GDPR-grade)

Goal
“Aggregate patient outcomes for ClinicGroup-A; train a model; produce a physician report. No PII leaves org; EU-only compute; DP budget ε ≤ 1.0; budget $300.”

Outline
1. Ingest labeled medical data (labels: pii, confidential, eu_only).
2. Redact and de-identify before any LLM use.
3. Train with DP; enforce ε tracking and per-call budgets.
4. Draft a report with local models; DLP scan outbound.
5. Require declassification approvals for any edge cases.

Key features
- IFC labels and declassification gates
- DP budget accounting
- Data locality enforcement
- DLP post-filtering and provenance

RTFS sketch
```clojure
(plan
  ^{:resources {:max_cost_usd 300 :data_locality :eu_only :privacy_budget {:epsilon 1.0}}}
  :program
  (let [raw (step "Ingest EHR" (call :ehr.ingest {:group "ClinicGroup-A"}))
        redacted (step "Redact PII"
                   (call :pii.redact {:data raw :policy :strict}))
        model (step "DP Train"
                 (call :ml.dp-train {:data redacted :epsilon 1.0 :epochs 5}))
        report (step "Draft Report"
                 (call :llm.local.summarize {:model "local-7b-eu" :docs [model] :style :clinical}))]
    (step "DLP outbound"
      (call :dlp.scan {:payload report :policy :no-sensitive-external}))
    report))
```

---

## 3) Trading with Quorum and Simulation

Goal
“Rebalance portfolio to targets; slippage ≤ 0.5%; fees ≤ $200; require quorum (Strategy/Ethics/Risk). Execute only during market hours.”

Outline
1. StrategyArbiter proposes orders; Ethics vetoes flagged venues; Resource reduces model cost.
2. GK enforces quorum and market-time guard.
3. Orchestrator runs dry-run simulation; only executes if predictions fit budget.
4. Idempotency keys for order placement; compensation for cancellation.

Key features
- Federation/quorum governance
- Dry-run + cost prediction
- Transactionality and idempotency
- Time-window execution guard

RTFS sketch
```clojure
(plan
  ^{:policy {:quorum [:arbiter.strategy :arbiter.ethics :arbiter.risk]}}
  :program
  (let [orders (call :strategy.rebalance {:targets {...}})]
    (step "Dry-run"
      (call :simulator.cost-latency {:orders orders :max_fees 200 :slippage 0.5}))
    (step.with-compensation
      (step "Place orders"
        ^{:idempotency {:key "rebalance-2025-08-01" :scope :intent}}
        (call :broker.place {:orders orders}))
      (step "Cancel unfilled" (call :broker.cancel {:orders orders})))))
```

---

## 4) Secure Supply-Chain Patch and Rollout

Goal
“Detect CVE-XXXX; patch, build, attest, canary deploy to 10%; expand to 100% if error budget stable; roll back on regression; notify security.”

Outline
1. SBOM scan; identify impacted services.
2. Build with SLSA attestation; Sigstore sign.
3. Canary deploy; progressive rollout guarded by SLO analysis.
4. Rollback compensation on regression.
5. Anchor attestations and rollout evidence to Causal Chain.

Key features
- SBOM/SLSA attestation and Sigstore
- Progressive delivery with SLO gates
- Automatic rollback via compensations
- Full audit trail for compliance

---

## 5) Legal Workflow with Human Approval and DLP

Goal
“Draft and send DPA updates; ensure jurisdictional clauses; counsel sign-off required; no PII leaves org.”

Outline
1. Draft with clause library + local LLM.
2. DLP post-filter; strip internal notes.
3. Show diff preview to counsel; capture signed approval.
4. Dispatch to vendors; track signatures; log provenance.

Key features
- Human-in-the-loop approvals and signed artifacts
- DLP post-filtering of outbound content
- Clause provenance for defensibility

---

## 6) Disaster Recovery Drill with Provable RPO/RTO

Goal
“Perform DR drill; fail to secondary region; validate RPO/RTO; fail back; reversible; EU-only data.”

Outline
1. Plan region failover; run synthetic load.
2. Measure RPO/RTO; assert against SLOs.
3. If unmet, revert; produce improvements report.

Key features
- Reversible operations with compensations
- Measured, provable objectives
- IFC locality constraints

---

## 7) Generative Capability Creation and Publish

Goal
“Create and publish a ‘CSV-to-Insights’ capability from existing tools; include contract, tests, conformance; sign and publish.”

Outline
1. Compose analytics pipeline into a capability.
2. Write capability contract: input/output/effects/resources/determinism.
3. Author conformance tests; run Marketplace CI; sign artifact.
4. Publish with versioning; future plans can discover/reuse.

Key features
- Generative capabilities + Marketplace lifecycle
- Contract-first design with tests and attestation
- Ecosystem compounding

---
