# The Cognitive Assistant Metaplan: A Vision for Self-Evolving AI

This document outlines the vision, targets, and milestones for transforming the CCOS Smart Assistant from a proof-of-concept into a fully cognitive, self-evolving system. The core idea is to "dogfood" the CCOS/RTFS architecture: we will use the system's own cognitive tools to build and refine itself.

The current implementation, while functional, relies on hardcoded Rust logic and ad-hoc heuristics. This new vision replaces that rigid structure with a dynamic, adaptable framework where the process of building and running the assistant is itself defined as a high-level RTFS plan—the `metaplan.rtfs`.

## Vision: The System Builds the System

Imagine a development process where the primary artifact is not source code, but a high-level plan that the CCOS Arbiter can understand and execute. An LLM, acting as a cognitive partner, can reason about this plan, propose changes, synthesize new capabilities, and even repair broken logic, all within the safe, auditable confines of the RTFS language.

This approach treats the assistant's architecture as data, which can be queried, transformed, and optimized by the system itself. The development loop becomes a cognitive loop:

1.  **Goal**: A human provides a high-level goal (e.g., "Implement a more efficient discovery algorithm").
2.  **Intent & Planning**: The Arbiter, guided by an LLM, breaks this goal down into a series of steps, modifying the `metaplan.rtfs`.
3.  **Capability Synthesis**: For steps that lack existing tools, the system prompts an LLM to synthesize a new capability in RTFS, using a library of secure, reusable primitives.
4.  **Validation & Repair**: The synthesized RTFS is automatically parsed, analyzed for safety, tested, and even repaired if it fails validation.
5.  **Execution**: The Arbiter executes the plan, calling upon the newly defined capabilities.
6.  **Refinement**: The system can analyze its own plans and automatically optimize them, for example, by pushing filtering logic closer to the data source.

This creates a powerful flywheel effect: the more the system is used, the more capable and optimized it becomes.

## Core Targets

To realize this vision, we will focus on the following high-level targets, which are derived from the `ccos-smart-assistant-generalization-plan.md` document.

1.  **Generalize with Primitives**: Decompose complex logic into a small, powerful set of generic, typed primitives (e.g., `filter`, `map`, `sort`, `join`). These become the universal building blocks for all synthesized capabilities.
2.  **Embrace RTFS as the Source of Truth**: All cognitive processes—from planning to capability implementation—will be defined in RTFS. Rust code will be relegated to the secure runtime and the implementation of the core primitives.
3.  **Make Everything Configurable**: Move all decision-making logic (e.g., how to discover a capability, what optimization to apply) out of the code and into data-driven configuration files.
4.  **Prioritize Safety and Auditability**: All dynamically generated code will be executed in a restricted, sandboxed RTFS runtime that prevents side effects and guarantees determinism. Every decision made by the system will be captured in an auditable trace.
5.  **Empower the LLM as a System Developer**: The LLM is not just a tool user; it is a core part of the development team, capable of writing, testing, and repairing the system's own logic.

## Milestones & Work Packages

The journey is broken down into four key milestones, with each step corresponding to a work package in the `metaplan.rtfs`. This plan is not just a document; it is the executable roadmap that the CCOS system will follow.

### Milestone 1: Foundations for Synthesis & Safe Execution

This milestone lays the groundwork for a dynamic system by creating the building blocks for synthesis and a secure environment to run them in.

-   **WP1: LocalSynth Framework**: Implement the core registry of typed data primitives.
-   **WP2: Safe RTFS Execution**: Build the restricted runtime to safely execute synthesized code.
-   **WP6: Canonical RTFS Loader**: Unify all RTFS parsing to ensure consistency and remove legacy heuristics.

### Milestone 2: Dynamic Discovery & Orchestration

With the foundations in place, this milestone focuses on making the system's decision-making processes more intelligent and adaptable.

-   **WP3: Configurable Discovery**: Externalize the logic for finding capabilities, making it data-driven.
-   **WP7: I/O Aliaser**: Decouple plan-specific data names from the canonical names used by primitives.
-   **WP4: Orchestrator Rewrite**: Implement a plan optimization engine to improve performance automatically.

### Milestone 3: LLM-Driven Evolution & Usability

This is where the system truly becomes "cognitive," with the ability to generate its own capabilities and provide clear feedback.

-   **WP5: LLM Synthesis Mode**: Grant the LLM the ability to write, test, and repair new RTFS capabilities on demand.
-   **WP8: Tracing and UX**: Improve the observability and debuggability of the entire cognitive process.

### Milestone 4: Documentation & Testing

A robust system requires comprehensive testing and clear documentation to ensure it is reliable and maintainable.

-   **WP9: Test Suite**: Build a full suite of unit, integration, and safety tests.
-   **WP10: Documentation**: Create user and developer guides for the new, dynamic framework.

By executing this metaplan, the CCOS Smart Assistant will evolve from a specific application into a general-purpose cognitive architecture—a system that can not only solve problems but can also improve its own ability to solve them.
