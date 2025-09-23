# RTFS 2.0 Incoming Spec: Behavioral Contracts and Compensations

Status: Proposed (specs-incoming)  
Audience: RTFS compiler/runtime, Orchestrator, Governance Kernel, Capability authors  
Related: specs-incoming/07-effect-system.md, specs-incoming/08-concurrency-and-determinism.md, specs-incoming/09-capability-contracts.md, docs/rtfs-2.0/specs/01-language-features.md, 05-native-type-system.md, docs/ccos/specs/002-plans-and-orchestration.md, 014-step-special-form-design.md

## 1. Overview

This document specifies first-class Behavioral Contracts (pre/postconditions and invariants) and Compensations (saga semantics) for RTFS plans and steps. These features increase safety, enable precise policy controls, and make failure recovery auditable and reliable.

Goals
- Allow precise pre/post conditions on steps, functions, and plans using pure predicates with refinements.
- Provide a uniform saga construct (step.with-compensation) for defining compensating actions for effectful steps.
- Integrate with the Effect System (07) and Concurrency (08) to ensure contracts and compensations are enforced under least privilege and deterministic replay.
- Provide strong observability via the Causal Chain for both contract checks and compensation execution.

Non-Goals
- General-purpose runtime assertion language (contracts are restricted, pure, and evaluable).
- Transactional ACID across arbitrary external systems (sagas implement best-effort compensations).

## 2. Terminology

- Contract: A set of pure predicates that constrain allowed inputs/state before a step (pre) and validate outputs/state after a step (post).
- Invariant: A predicate that must hold before and after an entire scope (function/plan), not just a single step.
- Compensation: A corrective effectful action intended to semantically undo a preceding effectful step (saga pattern).
- Idempotency: A property ensuring re-executing the same operation has no additional effect; used with retries and compensations.

## 3. Syntax

Contracts are attached via metadata to steps, functions, and plans. Compensation is a special orchestration form.

3.1 Contracts on steps/functions/plans
```clojure
(step "TransferFunds"
  ^{:pre  (fn [ctx]
            (and (contains? ctx :src) (contains? ctx :dst)
                 (pos? (:amount ctx))
                 (>= (get-in ctx [:balances (:src ctx)]) (:amount ctx))))
    :post (fn [ctx ctx']
            (and (= (- (get-in ctx  [:balances (:src ctx)]) (:amount ctx))
                    (get-in ctx' [:balances (:src ctx)]))
                 (= (+ (get-in ctx  [:balances (:dst ctx)]) (:amount ctx))
                    (get-in ctx' [:balances (:dst ctx)]))))}
  (call :bank.transfer {:from (:src ctx) :to (:dst ctx) :amount (:amount ctx)}))
```

Contracts on defn and plan:
```clojure
(defn safe-withdraw
  ^{:pre  (fn [acct amt] (and (pos? amt) (>= (:balance acct) amt)))
    :post (fn [acct amt result] (>= (:balance result) 0))}
  [acct amt]
  (call :accts.withdraw {:id (:id acct) :amount amt}))

(plan
  ^{:invariant (fn [state state'] (>= (:risk-budget state') 0))}
  :program
  (do ...))
```

Rules
- :pre takes the current context/value(s).
- :post takes the pre-state/context/value(s) and post-state/context/value(s).
- :invariant takes the entry and exit states of a scope (function/plan).
- All predicates MUST be pure: no (call ...) or side-effects allowed; only pure ops, type/refinement checks, and structural queries.

3.2 Compensation special form
```clojure
(step.with-compensation
  (step "CreateOrder"
    ^{:effects [[:network {:domains ["orders.example.com"] :methods [:POST]}]]
      :idempotency {:key (str "order:" (:user ctx) ":" (:cart-hash ctx)) :scope :intent}}
    (call :orders.create {:user (:user ctx) :items (:items ctx)}))
  (step "CancelOrder"
    ^{:effects [[:network {:domains ["orders.example.com"] :methods [:POST]}]]
      :idempotency {:key (str "order-cancel:" (:user ctx) ":" (:cart-hash ctx)) :scope :intent}}
    (call :orders.cancel {:order_id (:order-id ctx)})))
```

Semantics summary
- The primary step runs first. If it fails, ORCH may execute compensation depending on failure point and policy.
- If the primary step succeeds but later plan logic fails or the plan is aborted/cancelled, compensation is executed to roll back.
- Compensation steps are regular steps with their own effects/resources/contracts and idempotency.

## 4. Semantics

4.1 Contracts
- Precondition evaluation occurs immediately before executing a step/function. Failure:
  - The step is NOT executed; ORCH emits a contract violation action and returns a typed error (e.g., :contract/precondition-failed) with predicate info.
- Postcondition evaluation occurs immediately after successful execution. Failure:
  - ORCH emits a contract violation action, triggers compensation if defined (see 4.3), and marks step as failed; GK policy decides escalation/quarantine.
- Invariants on scopes are checked at the scope boundaries; violations behave like postcondition failures.

4.2 Interaction with Effects
- Contracts are pure and cannot perform side-effects. They may inspect inputs/outputs and derived pure values.
- For effectful steps, postconditions run after effects have occurred; thus failures must rely on compensation to revert externally visible changes.

4.3 Compensation Execution Model (Saga)
- Trigger conditions:
  - Primary step succeeds but later plan failure requires rollback.
  - Postcondition/invariant failure for the primary or enclosing scope.
  - Plan cancellation/abort signals after primary completion.
- Ordering:
  - Compensations execute in reverse order of successful primary steps (LIFO) within the affected scope.
- Idempotency:
  - Compensation should be idempotent; ORCH deduplicates via idempotency keys when re-invoked.
- Failure of compensation:
  - ORCH emits failure to Causal Chain and escalates per GK policy (quarantine, human intervention, further mitigations).

4.4 Concurrency
- With step.parallel (see 08), each branch’s compensation stack is independent; on join failure, compensations are executed for branches that completed primaries.
- Cross-branch compensations MUST NOT assume ordering unless explicitly sequenced outside of step.parallel.

## 5. Idempotency and Retry Integration

- Steps MAY provide ^{:idempotency {:key string :scope [:plan|:intent|:global]}}.
- ORCH deduplicates retries/replays using the key and scope.
- GK may require idempotency for certain effect classes (e.g., financial operations, provisioning).
- For step.with-compensation, both primary and compensation SHOULD provide idempotency keys; these may be correlated (e.g., derived from a shared business key).

## 6. Compiler Obligations

- Validate that contract bodies (:pre/:post/:invariant) are syntactically pure (no (call ...), no effectful special forms).
- Type-check predicates against available bindings; leverage refinement types for static checks where possible.
- Ensure contract arity matches the annotated construct:
  - step pre: (fn [ctx] ...)
  - step post: (fn [ctx ctx'] ...)
  - defn pre/post: arity matches function parameters and result
  - plan invariant: (fn [state state'] ...)
- Emit structured diagnostics for:
  - Non-pure contract bodies
  - Type mismatches or non-total predicates (where detectable)
  - Missing contracts where policy requires them (as a warning/error based on compiler mode)

## 7. Governance Kernel (GK) Obligations

- Admission Policy:
  - Require contracts on steps in specified risk tiers (e.g., money movement, PII handling).
  - Require compensations for effectful steps that mutate external state and are not provably idempotent.
  - Validate that compensation steps’ effects/resources are allowed and sufficiently narrow.
- Enforcement:
  - Reject plans that lack mandatory contracts/compensations.
  - Configure escalation behavior for contract/compensation failures (quarantine, human approval).
- Audit:
  - Ensure contract admissions and policy decisions are recorded in the Causal Chain.

## 8. Orchestrator (ORCH) Obligations

- Evaluate pre/post/invariant predicates at runtime with captured contexts; enforce time/resource limits to avoid pathological contracts.
- Prevent side-effects from contracts (execute in a pure interpreter).
- Manage compensation stacks per scope; on failure/abort, execute compensations in reverse success order.
- Enforce idempotency deduplication using declared keys.
- Emit Causal Chain actions:
  - ContractCheckStarted/Completed with result and predicate metadata (hash or redacted form as policy requires)
  - CompensationStarted/Completed with linkage to the primary step
  - Detailed failure reasons and policy outcomes

## 9. Examples

A) Contracts + Compensation for external mutation
```clojure
(step.with-compensation
  (step "Debit"
    ^{:pre  (fn [ctx] (<= (:amount ctx) (get-in ctx [:balances (:src ctx)])))
      :post (fn [ctx ctx'] (= (- (get-in ctx [:balances (:src ctx)]) (:amount ctx))
                              (get-in ctx' [:balances (:src ctx)])))
      :effects [[:network {:domains ["bank.example.com"] :methods [:POST]}]]
      :idempotency {:key (str "debit:" (:src ctx) ":" (:tid ctx)) :scope :intent}}
    (call :bank.debit {:account (:src ctx) :amount (:amount ctx)}))
  (step "CreditReversal"
    ^{:effects [[:network {:domains ["bank.example.com"] :methods [:POST]}]]
      :idempotency {:key (str "reversal:" (:src ctx) ":" (:tid ctx)) :scope :intent}}
    (call :bank.credit {:account (:src ctx) :amount (:amount ctx)})))
```

B) Plan invariant with risk budget
```clojure
(plan
  ^{:invariant (fn [s s'] (>= (:risk-budget s') 0))
    :resources {:max_cost_usd 25.0}}
  :program
  (do
    (step "Analyze" (call :llm.analyze {:docs docs}))
    (step "Notify" (call :notify.slack {:channel "#ops" :msg "done"}))))
```

C) Function-level contracts used inside steps
```clojure
(defn normalize-email
  ^{:pre  (fn [s] (and (string? s) (<= (count s) 254)))
    :post (fn [s s'] (re-matches #".+@.+\\..+" s'))}
  [s] (lower-case (trim s)))

(step "CreateUser"
  (call :user.create {:email (normalize-email (:email ctx))}))
```

## 10. Security Considerations

- Contracts are pure; sandboxed evaluations are mandatory (no ambient capabilities).
- Predicate complexity/time is bounded; failures to complete in time result in contract evaluation failure and policy-driven handling.
- Compensation execution is auditable and must follow effect/resource policies; improper compensations are grounds for quarantine.

## 11. Backward Compatibility & Migration

- Existing steps without contracts continue to work unless GK policy requires contracts.
- Compiler can provide a “contract-suggest” mode that generates scaffolds based on schemas and refinement types.
- Compensation introduction can be incremental: warn first, enforce later via policy.

## 12. Open Questions

- Canonical representation of predicate logic for better static analysis (restricted DSL vs. general fn).
- Standard result shapes for partial rollbacks in complex multi-step sagas.
- Minimum set of standard invariants GK may impose automatically (e.g., “budget remains ≥ 0”).

## 13. Acceptance Criteria

Compiler
- Enforces purity and type checking of contracts; validates arity and shapes; warnings/errors per policy.
- Emits metadata for ORCH/GK contract enforcement.

Governance Kernel
- Policy gates for mandatory contracts/compensations by risk tier/effect class.
- Admission fails for missing or non-conforming contracts/compensations.

Orchestrator
- Executes pre/post/invariant checks; manages compensation stacks with idempotency.
- Emits full audit trail for contract evaluations and compensations to the Causal Chain.

---
This is an incoming specification intended for review and iteration. Once stabilized, it should be merged into the core RTFS 2.0 specs and cross-referenced from the Effect System and Concurrency documents.
