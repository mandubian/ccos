# RTFS 2.0 Incoming Spec: Concurrency and Determinism

Status: Proposed (specs-incoming)
Audience: RTFS compiler/runtime, Orchestrator, Governance Kernel, Capability Marketplace
Related: docs/rtfs-2.0/specs/01-language-features.md, 05-native-type-system.md, specs-incoming/07-effect-system.md, docs/ccos/specs/002-plans-and-orchestration.md, 014-step-special-form-design.md

## 1. Overview

This document proposes formal concurrency and determinism semantics for RTFS. It defines how parallel composition behaves, how results are combined, how non-determinism is controlled, and how retries/idempotency interact with the Causal Chain.

Goals
- Provide deterministic-by-construction semantics for parallel execution where feasible.
- Define happens-before and join semantics for step.parallel.
- Specify seeding and environmental capture to enable exact replay.
- Define idempotency and retry behavior (failure domains) for robust orchestration.
- Integrate with effects and resources to allow admission-time and runtime enforcement.

Non-Goals
- Scheduling policy specification beyond required determinism and safety.
- Replacing plan-level policy decisions (e.g., approval) handled by the Governance Kernel.

## 2. Constructs

- step.parallel: Executes a fixed set of child steps concurrently; returns an aggregate result with deterministic shape/order.
- step.loop: Repeated execution with a condition; each iteration is sequential within the loop body unless explicitly nested in step.parallel.
- do: Sequential composition.

Note: This spec focuses on step.parallel; loop and other constructs are referenced for interaction only.

## 3. Determinism Model

Determinism is a property of an execution under a given environment digest. RTFS plans may carry determinism metadata:

^{:determinism {:seed "0xDEADBEEF"
                :model_ver "v1.2.3"
                :cap_versions {:com.a:v1 "sha256:.."}
                :env_digest "sha256:..." }}

- seed: Root random seed for deterministic sources (:random, seeded LLM modes).
- model_ver/cap_versions: Pinning for model and capability versions.
- env_digest: Measured environment state (e.g., container image, stdlib hash, config).
- Branch seeds: Derived deterministically as HMAC(seed, "parallel:" || plan-id || step-id || branch-index).

Capabilities must declare whether they can operate deterministically under supplied seeds. Non-deterministic capabilities are allowed but must be recorded; GK policies may restrict them.

## 4. step.parallel Semantics

Syntax (illustrative)

(step.parallel
  (step "A" exprA)
  (step "B" exprB)
  (step "C" exprC))

Return Value
- The result is a vector [resA resB resC] ordered by the lexical order of branches.
- If a branch is named (via step "name"), the orchestrator may also produce a map { :A resA :B resB :C resC } as an auxiliary artifact for observability, but the RTFS value is the vector.

Happens-Before
- Within each branch, standard sequential semantics apply.
- Across branches, no ordering is guaranteed until the join point.
- The join occurs when all non-cancelled branches reach a terminal state (success/failure/cancel).

Failure Propagation
- Default: fail-fast. A hard failure in any branch cancels all unfinished sibling branches, executes compensations (if defined), and fails the overall step.parallel.
- Configurable policy (future extension): allow best-effort completion of all branches; return partial results with error envelopes. Admission by GK required.

Cancellation Semantics
- Cancel signals propagate to branches cooperatively; branches must be idempotent and/or define compensations.

Timeouts
- A step.parallel may carry a timeout; on expiry, remaining branches are cancelled, compensations executed, and the step.parallel returns a failure (or partial per policy).

## 5. Effects and Resources in Concurrency

Effects
- The effect row of step.parallel is the union (normalized) of branch effect rows.
- At runtime, each branch executes with least privilege derived from its branch effect subset (not the union of all branches).

Resources
- Admission-time: GK validates sum or partitioned resource usage (e.g., token_budget split across branches or capped globally).
- Runtime: ORCH enforces per-branch quotas and global ceilings; exceeding triggers cancel and compensations per policy.

Conflict Control
- Shared resources (e.g., :fs with overlapping paths) require explicit read/write modes. Conflicting write/write attempts within a parallel group are rejected at admission or serialized by ORCH if policy allows.

## 6. Idempotency, Retries, and Compensations

Idempotency Keys
- Each step may carry ^{:idempotency {:key string :scope [:plan|:intent|:global]}}.
- ORCH deduplicates replays/retries using the key scope; duplicate completions return the first successful result recorded in the Causal Chain.

Retries
- Per-step retry policy: ^{:retry {:max 3 :backoff_ms [100 300 900] :retry_on [:error/network :timeout]}}
- step.parallel can retry failing branches independently, subject to global budget/time ceilings.

Compensations
- Use step.with-compensation to pair a primary step and a compensating step. If a branch fails after partially applying effects, compensation is executed. If any compensation fails, ORCH logs the failure and escalates per GK policy (quarantine/abort).

## 7. Replay and Audit

Causal Chain
- ORCH emits PlanStepStarted/Completed for each branch with:
  - deterministic seed used
  - branch effect profile
  - resource debits
  - capability versions
  - inputs/outputs hashes (content-addressed)
- The join step emits a summary action referencing child action IDs.

Replay
- Using plan determinism metadata and CC records, ORCH can re-execute branches with the same seeds and versions. If non-deterministic capabilities were used, replay is flagged as “best-effort”.

## 8. Compiler Obligations

- Verify that step.parallel child expressions are well-typed and effect rows are composable.
- Compute the aggregate effect row and validate annotations on enclosing constructs (plan/step).
- Surface diagnostics for:
  - Conflicting resource annotations across branches
  - Non-intersectable effect parameters after normalization
  - Missing idempotency where policy demands it
- Emit metadata stubs for ORCH/GK (branch indexes, names, idempotency keys if present).

## 9. Governance Kernel (GK) Admission

- Validate aggregate effect/resource rows against policy.
- Check risk tiers and determine if parallelism increases risk (e.g., multiplied egress exposure).
- Enforce policies:
  - Disallow non-deterministic LLM calls in high-determinism plans
  - Require compensations for effectful branches that modify external state
  - Set concurrency limits (max parallel branches) by tenant/project
- Bind policy decisions into the admitted plan envelope (stored in CC).

## 10. Orchestrator (ORCH) Execution

- Derive per-branch sandbox profiles from branch effect subsets.
- Allocate per-branch budgets (time, tokens, cost) and enforce at runtime.
- Generate branch seeds deterministically from plan seed.
- Handle retries with dedup and idempotency; execute compensations as required.
- Emit structured telemetry (OpenTelemetry) correlating branch spans to plan spans.

## 11. Examples

A) Deterministic fan-out fetch with budget

(plan
  ^{:determinism {:seed "0xFEED" :model_ver "local-7b-v3"}
    :resources   {:token_budget 500000 :max_time_ms 10000}}
  :program
  (let [urls ["https://a.com/x" "https://b.eu/x" "https://c.eu/x"]]
    (step.parallel
      (step "FetchA"
        ^{:effects [[:network {:domains ["a.com"] :methods [:GET]}]]}
        (call :com.http:get {:url (get urls 0)}))
      (step "FetchB"
        ^{:effects [[:network {:domains ["b.eu"] :methods [:GET]}]]}
        (call :com.http:get {:url (get urls 1)}))
      (step "FetchC"
        ^{:effects [[:network {:domains ["c.eu"] :methods [:GET]}]]}
        (call :com.http:get {:url (get urls 2)})))))

; Result is a vector [resA resB resC] with fixed order.

B) Parallel with compensation and retry

(step.parallel
  (step.with-compensation
    (step "ProvisionUser"
      ^{:retry {:max 3 :backoff_ms [100 300 900]}
        :effects [[:network {:domains ["idp.example.com"] :methods [:POST]}]]}
      (call :idp.provision {:user user}))
    (step "RollbackProvision"
      (call :idp.deprovision {:user user})))
  (step "WelcomeEmail"
    ^{:effects [[:network {:domains ["mail.example.com"] :methods [:POST]}]]}
    (call :email.send {:to (:email user) :template "welcome"})))

## 12. Open Questions

- Partial success contract: should a canonical result shape for mixed success/failure be standardized?
- Concurrency limits at language level: should max parallelism be annotatable on step.parallel?
- Deterministic LLM: formalize the conditions under which LLM calls can be declared deterministic (tokenizer version, temperature=0, beam settings, etc.).

## 13. Acceptance Criteria

Compiler
- Computes and validates effect/resource aggregation for step.parallel.
- Emits actionable diagnostics for conflicts and missing annotations.
- Preserves determinism/resource/idempotency metadata for ORCH/GK.

Governance Kernel
- Enforces policies for parallel execution (risk, quotas, compensations).
- Records admission envelope into the Causal Chain.

Orchestrator
- Runs branches under least-privilege profiles with deterministic seeds.
- Enforces budgets and retries; executes compensations appropriately.
- Emits per-branch and join-level actions to the Causal Chain and telemetry.

---
This is an incoming specification intended for review and iteration. Once stabilized, it should be merged into the core RTFS 2.0 specs and cross-referenced from Plans & Orchestration and the Effect System documents.
