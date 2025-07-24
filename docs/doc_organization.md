# CCOS Documentation & Specification Hub

**Status:** Revised---

## 7. Project Tracking & Issue Summaries

This section provides links to the high-level project plans and the corresponding GitHub issue summaries, connecting the architecture to the active development work.

| Plan / Summary | Description | Link to Summary | Link to Plan |
| :--- | :--- | :--- | :--- |
| **CCOS Migration** | Tracks the implementation of the full CCOS architecture. | [`./ccos/CCOS_ISSUE_SUMMARY.md`](./ccos/CCOS_ISSUE_SUMMARY.md) | [`./archive/ccos/CCOS_MIGRATION_TRACKER.md`](./archive/ccos/CCOS_MIGRATION_TRACKER.md) |
| **Compiler Stabilization** | Tracks the work to bring the RTFS compiler to a production-ready state. | [`./ccos/COMPILER_ISSUE_SUMMARY.md`](./ccos/COMPILER_ISSUE_SUMMARY.md) | [`./ccos/COMPILER_COMPLETION_PLAN.md`](./ccos/COMPILER_COMPLETION_PLAN.md) |

## 8. Language Specifications (RTFS)

This section contains the technical specifications for the RTFS language itself.

-   **Browse all CCOS/RTFS 2.0 specifications**: [`./ccos/specs/`](./ccos/specs/)
-   **Legacy RTFS 1.0 Archive**: [`./rtfs-1.0/`](./rtfs-1.0/) - For historical context and reference. July 24, 2025

## 1. Introduction: Vision & High-Level Architecture

This document is the central hub for navigating the documentation of the **Cognitive Computing Operating System (CCOS)**. It provides a hierarchical map from the highest-level vision to the detailed specifications of every component.

-   **Core Vision**: [`vision/SENTIENT_RUNTIME_VISION.md`](./vision/SENTIENT_RUNTIME_VISION.md) - The foundational philosophy and long-term goals. The recommended starting point.
-   **Overall Architecture**: [`./ccos/specs/000-ccos-architecture.md`](./ccos/specs/000-ccos-architecture.md) - An overview of the complete system architecture and how its components interact.

---

## 2. Core Data & State Layer

This layer is concerned with the fundamental, persistent data structures that represent the system's knowledge, history, and goals.

| Concept | Description | Specification |
| :--- | :--- | :--- |
| **Intent Graph** | The "why." A persistent, graph-based structure of user goals and their relationships. | [`./ccos/specs/001-intent-graph.md`](./ccos/specs/001-intent-graph.md) |
| **Plan Archive** | A content-addressable, immutable storage for all executed `Plans`. | [`./ccos/specs/002-plans-and-orchestration.md`](./ccos/specs/002-plans-and-orchestration.md) |
| **Causal Chain** | The "what happened." An immutable, auditable ledger of every action and its reasoning. | [`./ccos/specs/003-causal-chain.md`](./ccos/specs/003-causal-chain.md) |

---

## 3. Orchestration & Execution Layer

This layer is responsible for taking goals from the Data Layer and turning them into actions. It discovers and invokes capabilities to execute plans.

| Concept | Description | Specification |
| :--- | :--- | :--- |
| **Plans & Orchestration** | The "how." The structure of executable `Plans` and the `step` special form. | [`./ccos/specs/002-plans-and-orchestration.md`](./ccos/specs/002-plans-and-orchestration.md) |
| **Capabilities & Marketplace** | The "who." A dynamic, economic ecosystem for offering and selecting services. | [`./ccos/specs/004-capabilities-and-marketplace.md`](./ccos/specs/004-capabilities-and-marketplace.md) |
| **Global Function Mesh (GFM)** | The universal, decentralized naming and discovery system for all capabilities. | [`./ccos/specs/007-global-function-mesh.md`](./ccos/specs/007-global-function-mesh.md) |
| **Delegation Engine** | The mechanism that routes tasks to the appropriate local or remote capability provider. | [`./ccos/specs/008-delegation-engine.md`](./ccos/specs/008-delegation-engine.md) |

---

## 4. Cognitive Control & Reasoning Layer

This is the "mind" of the CCOS. It governs execution, manages the finite resource of attention, and makes intelligent decisions.

| Concept | Description | Specification |
| :--- | :--- | :--- |
| **Arbiter & Cognitive Control** | The central "consciousness" that orchestrates all execution and translates intent to plans. | [`./ccos/specs/006-arbiter-and-cognitive-control.md`](./ccos/specs/006-arbiter-and-cognitive-control.md) |
| **Task Context & Security** | Defines how contextual information is securely managed and propagated through tasks. | [`./ccos/specs/005-security-and-context.md`](./ccos/specs/005-security-and-context.md) |
| **Context Horizon** | The strategy for managing the Arbiter's limited context window. | [`./ccos/specs/009-context-horizon.md`](./ccos/specs/009-context-horizon.md) |
| **Working Memory** | The system for distilling the Causal Chain into actionable wisdom for the Arbiter. | [`./ccos/specs/013-working-memory.md`](./ccos/specs/013-working-memory.md) |

---

## 5. Security & Governance Layer

This layer ensures the system operates safely, transparently, and ethically.

| Concept | Description | Specification |
| :--- | :--- | :--- |
| **Ethical Governance** | The "constitution" and rule engine that bounds the Arbiter's behavior. | [`./ccos/specs/010-ethical-governance.md`](./ccos/specs/010-ethical-governance.md) |
| **Capability Attestation** | The framework for verifying the identity, security, and provenance of capabilities. | [`./ccos/specs/011-capability-attestation.md`](./ccos/specs/011-capability-attestation.md) |
| **Intent Sanitization** | The process for validating and securing user-provided intents before execution. | [`./ccos/specs/012-intent-sanitization.md`](./ccos/specs/012-intent-sanitization.md) |

---

## 6. Language Specifications (RTFS)

This section contains the technical specifications for the RTFS language itself.

-   **Browse all RTFS 2.0 specifications**: [`./rtfs-2.0/specs/`](./rtfs-2.0/specs/)
-   **Legacy RTFS 1.0 Archive**: [`./rtfs-1.0/`](./rtfs-1.0/) - For historical context and reference.
