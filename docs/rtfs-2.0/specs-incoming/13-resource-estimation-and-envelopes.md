````markdown
# RTFS 2.0 Incoming Spec: Resource Estimation and Plan Envelopes

Status: Proposed (specs-incoming)  
Audience: Governance Kernel, Orchestrator, Arbiter, Compiler/Runtime engineers  
Related:
- Overview: docs/rtfs-2.0/specs-incoming/00-rtfs-2.0-overview.md
- Language Guide: docs/rtfs-2.0/specs-incoming/02-language-guide.md
- Effect System: docs/rtfs-2.0/specs-incoming/07-effect-system.md
- Concurrency & Determinism: docs/rtfs-2.0/specs-incoming/08-concurrency-and-determinism.md
- Capability Contracts: docs/rtfs-2.0/specs-incoming/09-capability-contracts.md
- Information Flow & Declassification: docs/rtfs-2.0/specs-incoming/11-information-flow-and-declassification.md
- Admission-Time Compilation & Caching: docs/rtfs-2.0/specs-incoming/12-admission-time-compilation-and-caching.md
- CCOS Orchestration: docs/ccos/specs/002-plans-and-orchestration.md
- Causal Chain: docs/ccos/specs/003-causal-chain.md

---

## 1) Purpose

Define how the compiler and Governance Kernel (GK) estimate resource consumption of RTFS plans at admission-time, produce a conservative Plan Resource Envelope, and how the Orchestrator (ORCH) enforces it at runtime, with feedback loops from telemetry for continuous improvement.

Resources tracked:
- Monetary cost (API/provider fees)
- Time (latency, wall-clock)
- Tokens (LLM input/output)
- Compute/memory bands (CPU/GPU/VRAM/RAM, coarse)
- Egress/ingress (bytes, request counts)
- Privacy/data-locality budgets (e.g., DP epsilon, cross-boundary exposure counts)

---

## 2) Inputs to Estimation

2.1 Capability Contracts (primary source) — see 09-capability-contracts.md
- Pricing: base_fee, per-unit fees (per token/MB/request)
- Performance: p50/p95 latencies, warmup/queueing penalties
- Tokenization: tokenizer id, expansion ratios for generation
- Egress: expected request/response sizes and protocol overheads
- Compute/memory: coarse band requirements (small/medium/large GPU, RAM range)
- Calibration: optional empirical traces and recent SLA metrics
- Versioned and signed; semver governs compatibility

2.2 Static Program Analysis (AST/IR)
- Call graph: number and types of calls in sequential/conditional/parallel regions
- Dataflow bounds: infer upper bounds on sizes via TypeExpr refinements:
  - Strings: [:max-length N]
  - Vectors: [:max-count N] with element bounds
  - Maps: field-wise bounds; wildcard fallback
- Control-flow:
  - Sequential: additive aggregation
  - Conditional (if/match): worst-case unless GK allows risk-weighted expectation
  - Parallel: aggregate cost sum; time as max(branch times)
  - Loops: require static bound N; otherwise reject or demand cap
- Composition: sum/maximum rules per resource type

2.3 Historical Telemetry (secondary source)
- Causal Chain + telemetry store distributions per capability and plan template
- Estimator blends contract priors with empirical posteriors (e.g., EWMA or Bayesian update)
- GK may weight empirical data higher than provider-declared values for risk control

---

## 3) Estimation Pipeline (Admission-Time)

1) Normalize and type-check plan; bind provider versions/contracts; validate effects (07) and concurrency (08).
2) For each capability call:
   - Derive input size bounds from TypeExpr refinements and constants known at admission.
   - For unknown inputs, require explicit upper bounds or use template priors; insert runtime guards to enforce bounds.
   - Compute per-call Resource Vector:
     - cost_usd
     - time_p50/time_p95
     - tokens_in/tokens_out_est
     - egress_bytes_in/out
     - compute_band/memory_band
3) Aggregate across control flow:
   - Sequential: sum costs/tokens/egress; time via sum (conservative: p95-sum)
   - Conditional: worst-case branch; or expected value if policy allows (with risk flag)
   - Parallel: cost/tokens/egress sum; time as max of branches; compute/memory aggregated per policy (sum or max)
   - Loops: multiply by iteration cap N; if no cap, reject or require Arbiter to supply one
4) Apply safety margins:
   - GK policy multipliers (e.g., +30% cost buffer, +2x time buffer, +20% tokens)
   - Enforce intent constraints (e.g., :max-cost, :token_budget, :max_time_ms)
5) Produce Plan Resource Envelope:
   - Per-step breakdown and plan-level totals
   - Confidence level (e.g., 90/95%) and list of assumptions
   - Hashable structure included in the signed Plan Envelope

---

## 4) Modeling Specific Resources

4.1 Cost
- cost(call) = base_fee + Σ unit_fee_i × units_i
- LLM units: input_tokens + output_tokens_est; output bound via k×input + b (contract)
- HTTP units: requests + bytes/MB
- Aggregate sequentially; include fixed overheads (e.g., marketplace routing fees) if applicable

4.2 Time
- Use contract p50/p95; sequential add (p95-sum), parallel max(branch p95) or policy-defined composition
- Include queueing penalties if concurrency > provider throughput

4.3 Tokens
- Infer input tokens via tokenizer on upper bounds
- Output tokens via expansion ratio; propagate context growth across steps if outputs feed future prompts

4.4 Compute/Memory
- Coarse bands; for parallel regions, ensure aggregate band ≤ tenant/host limits
- GK may force serialization to respect resource limits

4.5 Egress/Ingress
- Estimate request/response sizes; apply compression/encoding overheads
- Cross-check with IFC/data-locality policies

4.6 Privacy/Data Locality
- Track label exposure counts and DP epsilon budgets
- Admission fails if envelope violates policy limits (e.g., pii across non-EU egress)

---

## 5) Uncertainty Handling and Guards

- If bounds are missing, Arbiter must annotate via refinements (e.g., [:max-length 50k]); otherwise admission fails.
- Compiler inserts runtime guards to enforce bounds before capability calls; guard failures trigger compensations/quarantine.
- GK may require higher confidence or human approvals when estimates approach ceilings.

---

## 6) Plan Resource Envelope (Structure)

Example (illustrative):
```clojure
{:plan-id "..."
 :effects [...]
 :resources
  {:totals {:cost_usd 42.15 :time_p95_ms 180000 :tokens_in 1.2e6 :tokens_out 6.0e5
            :egress_bytes 45_000_000 :compute_band :gpu.medium :memory_gb 8}
   :per_step [{:step "Analyze" :cost_usd 12.3 :time_p95_ms 60000 ...}
              {:step "Draft"   :cost_usd 28.5 :time_p95_ms 120000 ...}]}
 :confidence 0.9
 :assumptions [{:call :com.local-llm:v1 :max_input_tokens 800k :k_ratio 0.5 :b 2000}
               {:call :http:get       :max_response_kb 512}]
 :policy_version "gk-2025-07-01"
 :contracts_digest "sha256:..."
 :determinism {:seed "0x..." :model_ver "local-7b-v3" :env_digest "sha256:..."}}
````

- Embedded in GK-signed Plan Envelope; hash anchored to Causal Chain.

---

## 7) Runtime Enforcement

- ORCH derives per-step budgets from the envelope; runs steps under least-privilege profiles.
- Hard stops when exceeding ceilings; execute compensations and log failure.
- Adaptive throttling/backoff on approaching limits; cancel parallel branches per fail-fast policy.
- Detailed PlanStepStarted/Completed actions include debits (cost/time/tokens/egress) for audit.

---

## 8) Feedback Loop and Continuous Improvement

- Actuals vs estimates stored in telemetry and Causal Chain.
- Estimator updates priors (per capability and plan template).
- GK can auto-tighten multipliers for volatile providers; relax for stable ones.
- Parametric templates store symbolic bounds; fast delta-admission on reuse.

---

## 9) Interaction with Effect System and Contracts

- Effects (07) constrain what can be called; resource estimates must be consistent with allowed effects.
- Contracts (09) provide pricing/perf/tokenization schemas and error variants for planning and typed catches.
- Admission fails on privilege broadening or missing contract attestation.

---

## 10) Acceptance Criteria

- Compiler admission API returns:

  - Per-call resource vectors; aggregated envelope; confidence and assumptions
  - Diagnostics listing missing bounds and suggested annotations

- GK:
  - Validates envelope against policy and intent ceilings; signs and records in Causal Chain

- ORCH:
  - Enforces per-step budgets; logs debits; triggers compensations on ceiling breach

- Telemetry:
  - Stores actuals; estimator consumes to refine future envelopes

---

## 11) Open Questions

- Standardize composition of p95 across long sequential chains (conservative sum vs modeled correlation)
- Vendor-declared vs empirical priors weighting strategy and governance overrides
- Expressing stochastic plans with expected-value admission under strict risk budgets

---

Changelog

- v0.1 (incoming): First specification for resource estimation and envelopes aligned with effect typing, contracts, and admission-time compilation.
