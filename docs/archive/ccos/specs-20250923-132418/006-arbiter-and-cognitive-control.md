# CCOS Specification 006: Arbiter and Cognitive Control

**Status:** Proposed
**Version:** 1.0
**Date:** 2025-07-20
**Related:**
- [SEP-000: System Architecture](./000-ccos-architecture.md)
- [SEP-002: Plans and Orchestration](./002-plans-and-orchestration.md)

## 1. Abstract

This specification defines the **Arbiter**, the primary cognitive component of a CCOS instance. The Arbiter is responsible for high-level reasoning, decision-making, and learning. It acts as the "mind" of the system, while the Orchestrator acts as the "engine." This separation allows for a clean distinction between deterministic, step-by-step execution and non-deterministic, AI-driven cognitive tasks.

## 2. The Arbiter's Role

The Arbiter is not a single model but a conceptual role that can be filled by various implementations, from a single powerful LLM to a **Federation of Arbiters** with specialized functions (e.g., Logic, Strategy, Ethics).

The Arbiter's core responsibilities include:

-   **Intent Formulation**: Interpreting ambiguous user requests or system goals and formalizing them into structured `Intent` objects in the Intent Graph.
-   **Plan Generation**: The Arbiter generates plans based on the user's `Intent`.
-   **Strategic Exception Handling**: When a plan fails in a way the Orchestrator cannot handle, the Arbiter is invoked to decide on a new course of action (e.g., generate a new plan, abandon the intent).
-   **Cognitive Execution**: For certain tasks that are inherently linguistic or creative, the Arbiter may execute them directly rather than delegating.

## 3. The Arbiter-Orchestrator Interaction Loop

The relationship between the Arbiter and the Orchestrator is the central loop of CCOS.

```mermaid
sequenceDiagram
    participant User
    participant Arbiter
    participant Orchestrator
    participant CausalChain

    User->>Arbiter: "I need to analyze customer sentiment."
    Arbiter->>Arbiter: Formulate Intent
    Arbiter->>Arbiter: Generate Plan
    Arbiter->>Orchestrator: Execute(Plan, Context)
    
    Orchestrator->>CausalChain: Log PlanStarted
    loop For each step in Plan
        Orchestrator->>CausalChain: Log PlanStepStarted
        Orchestrator->>Orchestrator: Execute step code
        alt On Failure
            Orchestrator->>CausalChain: Log PlanStepFailed
            Orchestrator->>Arbiter: ReportFailure(error)
            Arbiter->>Arbiter: Decide next action (e.g., new plan)
            Arbiter-->>Orchestrator: Abort() or ResumeWithNewPlan()
            break
        end
        Orchestrator->>CausalChain: Log PlanStepCompleted
    end
    Orchestrator->>CausalChain: Log PlanCompleted
    Orchestrator->>Arbiter: ReportSuccess(result)
    Arbiter->>User: "Sentiment is 'Positive'."
```

## 4. Architectural Constraints

To ensure safety and alignment, the Arbiter does not have unlimited power. It operates within a secure sandbox managed by the high-privilege **Governance Kernel** (see SEP-010). The Arbiter cannot bypass the ethical and security constraints defined in the system's `Constitution`. It proposes plans, but the Governance Kernel has the final authority to validate and execute them.

## 5. Arbiter Federation

For advanced CCOS implementations, the single Arbiter can be replaced by a federation of specialized agents that collaborate on decisions.

-   **Roles**: A federation might include a `LogicArbiter` for constraint satisfaction, a `StrategyArbiter` for long-term planning, and an `EthicsArbiter` for policy compliance.
-   **Workflow**: When a decision is needed (e.g., which plan to execute), a primary Arbiter can issue a request to the federation. The specialists can then "debate" by proposing, critiquing, and voting on alternatives.
-   **Causal Record**: The entire debate, including proposals, critiques, and dissenting opinions, can be recorded as a series of hierarchical actions in the Causal Chain, providing a complete audit trail of the decision-making process itself.

This federated model provides diversity of thought, robustness against single-model failures, and built-in checks and balances for critical decisions.
