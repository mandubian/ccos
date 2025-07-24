# CCOS Specification 004: Capabilities and Marketplace

**Status:** Proposed
**Version:** 1.0
**Date:** 2025-07-20
**Related:** [SEP-000: System Architecture](./000-ccos-architecture.md), [SEP-007: Global Function Mesh](./007-global-function-mesh.md)

## 1. Abstract

This document defines how CCOS extends its functionality through `Capabilities`. A Capability is a versioned, verifiable, and discoverable function or service that can be invoked by a `Plan`. This specification also outlines the evolution from a simple capability registry to a dynamic **Generative Capability Marketplace**.

## 2. Core Concepts

### 2.1. Capability

A `Capability` is a formal description of a service that can be executed. It includes:

-   **Identifier**: A universal, unique name (e.g., `:com.acme.database:v1.query`).
-   **Schema**: A formal definition of the expected inputs and outputs.
-   **Provider**: The concrete agent or module that implements the capability.
-   **Metadata**: Additional information like cost, latency, or security classification.

### 2.2. The `(call)` Primitive

Capabilities are invoked from within a `Plan` using the `(call ...)` primitive:

```rtfs
(call :com.acme.database:v1.query {:table "users" :id 123})
```

### 2.3. Discovery via the Global Function Mesh (GFM)

The GFM (see SEP-007) is responsible for resolving a capability identifier to a list of available providers.

## 3. The Capability Registry

In its initial implementation, CCOS uses a simple **Capability Registry**. This is a local or federated database that maps capability identifiers to provider implementations. The Orchestrator queries this registry via the GFM to find a valid provider for a given `(call)`.

## 4. Future Vision: The Generative Capability Marketplace

The long-term vision is to evolve the simple registry into a dynamic, economic ecosystem. This **Generative Capability Marketplace** will be a core component of the CCOS, enabling advanced, autonomous behavior.

### 4.1. Capabilities as Service Level Agreements (SLAs)

In the Marketplace, providers don't just register a function; they offer a service with a rich SLA, including:

-   **Cost**: Price per call or per token.
-   **Speed**: Average latency metrics.
-   **Confidence**: A score representing the likely accuracy of the result.
-   **Data Provenance**: Information about the origin of the data the capability uses.
-   **Ethical Alignment Profile**: A declaration of the ethical principles the capability adheres to.

The Arbiter will use this rich metadata to act as a broker, selecting the provider that best matches the `constraints` and `preferences` defined in the active `Intent`.

### 4.2. Generative Capabilities

The Arbiter itself will be able to create and publish new capabilities to the marketplace. If it requires a function that does not exist, it can:

1.  Find constituent capabilities on the marketplace.
2.  Compose them into a new RTFS function.
3.  Wrap this new function in a formal capability definition.
4.  Publish the new "generative capability" back to the marketplace for itself and others to use.

This allows the CCOS to learn, grow, and autonomously expand its own skillset over time, transforming it from a static tool into a living, evolving system.
5.  The result is returned to the RTFS runtime and the `CapabilityCall` action in the Causal Chain is updated with the outcome.

## 5. Delegation

Delegation is the process of using a capability to assign a sub-task to another cognitive agent, which could be a specialized LLM, another CCOS instance, or even a human.

### 5.1. The Delegation Pattern

Delegation is not a different mechanism; it is a pattern of using capabilities. A typical delegation capability might be `llm.generate-plan` or `human.approve-transaction`.

The following example shows how a plan can use one step to generate a sub-plan and a second step to execute it. The outer `(let ...)` block ensures that the result of the first step (`generated_plan`) is available to the second step.

```lisp
(let [
  ;; Step 1: Generate a sub-plan. The resulting plan object is bound to the `generated_plan` variable.
  generated_plan (step "Generate a sub-plan"
    (call :llm.generate-plan {:goal "Analyze user sentiment data" :constraints ...}))
]
  ;; Step 2: Execute the plan that was created in the previous step.
  (step "Execute the sub-plan"
    (call :ccos.execute-plan generated_plan))
)
```

> **Note on `(let)` and `(step)` Patterns**
>
> There are two primary ways to combine `let` and `step`, each for a different purpose:
>
> 1.  **`(let [var (step ...)] ...)` (Sequencing):** As shown above, this is the standard pattern for creating a sequence of dependent steps. The `let` creates a scope, and the result of one step is stored in a variable that can be used by subsequent steps.
> 2.  **`(step ... (let ...))` (Encapsulation):** This pattern is used when a single step requires complex internal logic or temporary variables to prepare for its main `(call)`. The `let` is contained entirely *within* the step and its variables are not visible to other steps.

### 5.2. Key Features

-   **Recursive CCOS**: A delegated call to another CCOS instance creates its own nested Causal Chain. The parent action in the calling system can store the `plan_id` of the sub-plan, creating a verifiable link between the two execution histories.
-   **Human-in-the-Loop**: A call to a human-in-the-loop capability (e.g., `human.ask-question`) would pause the plan's execution (`PlanPaused` action) until the human provides a response, at which point a `PlanResumed` action is logged and execution continues.
-   **Specialized Agents**: Allows the main orchestrator to act as a generalist that delegates complex, domain-specific tasks to expert agents (e.g., a coding agent, a data analysis agent).
