# CCOS Specification 002: Plans and Orchestration

**Status:** Proposed
**Version:** 1.0
**Date:** 2025-07-20
**Related:**
- [SEP-000: System Architecture](./000-ccos-architecture.md)
- [SEP-003: Causal Chain](./003-causal-chain.md)
- [Intent Event Sink (Audit) Specification](./intent_event_sink.md)
- [Worktree summary: wt/orch-core-status-context](./worktree-orch-core-status-context.md)

## 1. Abstract

This specification defines the structure of a `Plan` and the role of the **Orchestrator** in executing it. A `Plan` is not merely a script to be run; it is a declarative program that defines the high-level orchestration flow of a task. The Orchestrator is the active component that interprets this flow, manages state, and interacts with all other parts of the CCOS.

## 2. The `Plan` Object

A `Plan` is a structured RTFS object that defines the "how" for achieving an `Intent`.

### 2.1. Fields

-   `plan_id` (String, UUID): A unique identifier for the plan instance.
-   `name` (String, Optional): A human-readable symbolic name.
-   `intent_ids` (Vec<IntentId>): A list of one or more intents this plan is designed to fulfill.
-   `language` (Enum): The language of the plan's body. While other languages are possible, this spec focuses on `Rtfs20`.
-   `body` (Value): The executable content of the plan. For RTFS, this is a single RTFS expression, typically `(do ...)` or a `(ccos.steps/execute ...)` block.
-   `status` (Enum): The lifecycle status (`Running`, `Paused`, `Completed`, `Aborted`).
-   `metadata` (Map<String, Value>): Open-ended map for additional context, such as the plan's generation source (e.g., which LLM).

### 2.2. Plan Lifecycle and Archival

A `Plan` is not a long-lived, mutable object; it is an immutable script that is archived upon creation.

1.  **Generation**: The Arbiter generates the `body` of the plan.
2.  **Archival**: Before execution, the complete `Plan` object, with its new `plan_id`, is serialized and stored permanently in the **Plan Archive**. This is a content-addressable or key-value store dedicated to preserving the exact code that was proposed for execution.
3.  **Execution**: The Orchestrator receives the `plan_id` and retrieves the immutable plan from the archive to execute it.
4.  **Auditing**: The `plan_id` stored in every `Action` in the Causal Chain now serves as a permanent, verifiable foreign key, allowing any action to be traced back to the exact line of code in the archived plan that caused it.

This ensures a complete and auditable record, separating the "what was intended" (Intent), "what was proposed" (Plan Archive), and "what actually happened" (Causal Chain).

## 3. Orchestration Primitives in RTFS

To enable orchestration, RTFS is extended with a conceptual library of special forms. The Orchestrator has specific knowledge of these forms and treats them as instructions for itself.

### 3.1. `(step <name> <body>)`

-   **Purpose**: Defines a major, observable milestone in the plan.
-   **Orchestrator Behavior**:
    1.  Logs a `PlanStepStarted` action to the Causal Chain. The `name` is recorded in the action.
    2.  Executes the `<body>` expression.
    3.  If the body executes successfully, logs a `PlanStepCompleted` action with the result.
    4.  If the body fails, logs a `PlanStepFailed` action with the error and proceeds based on the retry/error handling policy.

### 3.2. `(step-if <condition> <then-branch> <else-branch>)`

-   **Purpose**: Defines a major strategic branch in the plan.
-   **Orchestrator Behavior**:
    1.  Evaluates the `<condition>` expression.
    2.  Based on the result, it dynamically injects the steps from either the `<then-branch>` or `<else-branch>` into its execution queue.
    3.  The branch itself is logged as an action, providing a clear record of the decision point.

### 3.3. Other Primitives

The `step` model is extensible to other control flow structures, such as:
   `(step-loop <condition> <body>)`: Loops through a block of steps.
   `(step-parallel <step1> <step2> ...)`: Executes a set of steps concurrently, waiting for all to complete.

## 4. The Orchestrator

The Orchestrator is the stateful engine that drives plan execution.

### 4.1. Responsibilities

-   **Execution Context Stack**: Maintains a stack of active `ActionId`s. When a step begins, its ID is pushed. When it ends, it's popped. This stack provides the `parent_action_id` for all nested actions, ensuring a correct hierarchy in the Causal Chain.
-   **State Management**: Manages the RTFS environment for the plan, ensuring that variables defined in one step are available to subsequent steps.
-   **Error and Retry Logic**: Implements the retry behavior. When a step fails, the Orchestrator logs the failure and, based on the plan's policy, can log a `PlanStepRetrying` action and re-execute the step.
-   **Security Enforcement**: Before executing a step (especially one containing a `(call ...)`), it consults the current `Runtime Context` to ensure the operation is permitted.
-   **Lifecycle Management**: Logs all lifecycle events (`PlanStarted`, `PlanAborted`, etc.) to the Causal Chain.

## 5. Execution Example

Consider the plan:
```lisp
(do
  (step "Prepare" (let [x 10]))
  (step "Execute" (call :my-cap x))
)
```

The Orchestrator would:
1.  Log `PlanStarted`. Push `plan-exec-1` to stack.
2.  See `(step "Prepare" ...)`
3.  Log `PlanStepStarted` (name: "Prepare", parent: `plan-exec-1`). Push `step-1` to stack.
4.  Execute `(let [x 10])`.
5.  Log `PlanStepCompleted` (result: `10`). Pop `step-1` from stack.
6.  See `(step "Execute" ...)`
7.  Log `PlanStepStarted` (name: "Execute", parent: `plan-exec-1`). Push `step-2` to stack.
8.  Execute `(call :my-cap x)`.
    -   The Capability Provider logs `CapabilityCall` (name: ":my-cap", parent: `step-2`).
9.  Log `PlanStepCompleted` (result of call). Pop `step-2` from stack.
10. Log `PlanCompleted`. Pop `plan-exec-1` from stack.
