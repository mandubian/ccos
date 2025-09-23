# CCOS Specification 003: Causal Chain

**Status:** Proposed
**Version:** 1.0
**Date:** 2025-07-20
**Related:**
- [SEP-000: System Architecture](./000-ccos-architecture.md)
- [SEP-002: Plans and Orchestration](./002-plans-and-orchestration.md)

## 1. Abstract

The Causal Chain is the immutable, append-only ledger that serves as the system's definitive record of "what happened." It provides a complete, verifiable, and hierarchical audit trail of all significant events, enabling debugging, analysis, and autonomous learning.

## 2. Core Principles

-   **Immutability**: Once an action is recorded in the chain, it can never be altered or deleted.
-   **Verifiability**: The integrity of the chain is protected by a hash chain. Each action is hashed, and that hash is combined with the hash of the previous action to create a new chain hash. Any modification to a past event would invalidate the entire subsequent chain.
-   **Hierarchy**: The chain is not a flat list. It is a tree structure that mirrors the nested execution of plans, steps, and capability calls, linked via a `parent_action_id`.

## 3. The `Action` Object

An `Action` is a record of a single, significant event.

### 3.1. Fields

-   `action_id` (String, UUID): A unique identifier for this specific action.
-   `parent_action_id` (String, UUID, Optional): The ID of the parent action, which links this action into the execution hierarchy. A top-level plan execution action would have a `null` parent.
-   `plan_id` (String, UUID): The ID of the top-level plan this action is associated with.
-   `intent_id` (String, UUID): The ID of the primary intent this action is helping to fulfill.
-   `action_type` (Enum): The specific type of event being recorded.
-   `function_name` (String): A human-readable name for the action (e.g., the step name, the capability ID).
-   `arguments` (Vec<Value>, Optional): The inputs to the action, if any.
-   `result` (Value, Optional): The output of the action, if any.
-   `success` (Boolean): `true` if the action completed successfully, `false` otherwise.
-   `error_message` (String, Optional): Details of the failure if `success` is `false`.
-   `cost` (Float): The calculated cost of the action (e.g., LLM tokens, API fees).
-   `duration_ms` (Integer): The time taken to execute the action.
-   `timestamp` (Timestamp): The time the action was recorded.
-   `metadata` (Map<String, Value>): An open-ended map for additional context, including a cryptographic signature.

### 3.2. Action Types (`action_type`)

This enum categorizes the event being recorded.

-   **Plan Lifecycle**:
    -   `PlanStarted`: The Orchestrator has begun executing a plan.
    -   `PlanCompleted`: The plan finished successfully.
    -   `PlanAborted`: The plan was stopped before completion.
    -   `PlanPaused` / `PlanResumed`: The plan's execution was paused or resumed.
-   **Step Lifecycle**:
    -   `PlanStepStarted`: A `(step ...)` has begun.
    -   `PlanStepCompleted`: A step finished successfully.
    -   `PlanStepFailed`: A step failed.
    -   `PlanStepRetrying`: The orchestrator is re-attempting a failed step.
-   **Execution**:
    -   `CapabilityCall`: A `(call ...)` to a capability was made.
    -   `InternalStep`: A fine-grained event from within the RTFS evaluator (optional, for deep debugging).

## 4. Causal Chain API

The Causal Chain component must provide an API for:

-   **Appending Actions**: A method to add a new action to the chain. This is the *only* way to modify the chain. The implementation must handle hash calculation and signing internally.
-   **Querying**: Methods to retrieve actions based on:
    -   `action_id`
    -   `plan_id`
    -   `intent_id`
-   **Traversal**: Methods to reconstruct the execution tree:
    -   `get_children(action_id)`: Returns all actions whose `parent_action_id` matches the given ID.
    -   `get_parent(action_id)`: Returns the parent action.
-   **Integrity Verification**: A method to traverse the entire chain and verify that all hash links are valid, confirming that the ledger has not been tampered with.

## 5. Core Feature: The Causal Chain of Thought

The Causal Chain is not merely a log of *actions*; it is a verifiable record of the system's entire reasoning process. It must not only record *what* happened but provide an unforgeable, cryptographic link to *why* it happened. This is a core, mandatory feature of the CCOS security model.

To achieve this, the `Action` object's `metadata` field **must** be enriched with direct, verifiable links to the specific context that led to the action:

-   **`intent_id`**: A direct link to the version of the `Intent` object that was active when the action was taken.
-   **`constitutional_rule_id`**: For any action validated by the Governance Kernel, a link to the specific `ConstitutionalRule(s)` that permitted the action. This is a critical security feature.
-   **`delegation_decision_id`**: A link to the `DelegationInfo` object from the Delegation Engine, explaining why a particular execution target was chosen.
-   **`capability_attestation_id`**: A link to the `Attestation` of the `Capability` that was executed. This proves that the code that ran was the code that was approved.

This transforms the Causal Chain from a simple log into a rich, queryable record of the system's reasoning process. It makes behavior transparent and provides the necessary data for high-stakes auditing, ensuring that every action can be traced back to a specific human-approved rule and a specific, verified piece of code.
