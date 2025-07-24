# The CCOS Roadmap: From RTFS to a Living Architecture

**Date:** June 23, 2025
**Status:** Strategic Plan

This document outlines the strategic, phased evolution from the current RTFS implementation to the visionary Cognitive Computing Operating System (CCOS). The key principle is that each phase should build upon the last and deliver a more capable and intelligent system.

---

### **Cross-Cutting Concern: Addressing the Context Horizon**

A critical engineering challenge for the CCOS is the finite context window of the core Arbiter LLM. The Intent Graph, Causal Chain, and generated Plans can grow to a size that exceeds this limit. Our strategy is not to assume infinite context, but to build a system for **Cognitive Resource Management**, treating the Arbiter's attention as a precious resource.

This will be achieved through three key mechanisms, developed progressively across the phases:

1.  **Intent Graph Virtualization (Phase 3+):**
    *   **Mechanism:** The Arbiter will not hold the entire Intent Graph in its active context. The graph will be stored in a dedicated database (e.g., a vector or graph database).
    *   **Implementation:** The Arbiter will use semantic search and graph traversal to load only the most relevant nodes and branches of the Intent Graph into its context for a given task. Older, completed, or irrelevant branches will be automatically summarized by a background process and archived, replaced by a compact "memory" node.

2.  **Causal Chain Distillation (Phase 3 & 4):**
    *   **Mechanism:** The Arbiter does not consume the raw, verbose Causal Chain ledger. This is the role of the **Subconscious**.
    *   **Implementation:** The Subconscious (V1 and V2) will be responsible for the offline analysis of the complete ledger. Its output will be highly-distilled, low-token summaries and heuristics (e.g., updated agent reliability scores, new failure patterns, optimized strategies). It is this *distilled wisdom*, not the raw data, that is fed back into the Arbiter's context, ensuring it learns from history without being burdened by it.

3.  **Plan & Data Abstraction (Phase 2+):**
    *   **Mechanism:** Plans will be structured hierarchically, and large data payloads will be referenced by handles, not included directly.
    *   **Implementation:** The Arbiter will be trained to generate abstract plans that call other functions (which may themselves be complex plans). For large data, plans will operate on handles (e.g., `(file-handle "/path/to/data.bin")`). The function implementation, not the Arbiter, is responsible for resolving the handle and processing the data, often using streaming to avoid loading everything into memory at once. This keeps the plan itself concise and focused on logic, not data marshalling.

By implementing these strategies, we ensure the CCOS can scale to handle long-running, complex tasks and histories without being constrained by the physical limitations of its core LLM.

---

### **Phase 0: Foundation (Current State)**

We begin from a position of strength. We have a robust, performant RTFS implementation including:
*   A complete parser, AST, and IR.
*   An advanced, multi-level IR optimizer.
*   A functional module system for code organization.
*   A foundational, trait-based system for agent discovery.
*   This is the bedrock upon which we will build.

---

### **Phase 1: The Proto-Arbiter & The Service Layer**

**Goal:** Evolve the runtime from a simple executor into a basic orchestrator and formalize the concept of "capabilities" as network-accessible services.

**Key Steps:**
1.  **Implement `(llm-execute)`:** Create the first, explicit bridge allowing an RTFS plan to delegate a task back to a core LLM. This introduces the hybrid execution model in its simplest form.
    *   **Specification:** [`docs/ccos/specs/006-arbiter-and-cognitive-control.md`](./ccos/specs/006-arbiter-and-cognitive-control.md)
2.  **Launch the Agent Registry:** Implement a real, network-accessible service where agents can register their function signatures. This moves beyond the current `NoOpAgentDiscovery` and makes the agent system real.
    *   **Specification:** [`docs/ccos/specs/004-capabilities-and-marketplace.md`](./ccos/specs/004-capabilities-and-marketplace.md)
3.  **Develop the `ArbiterV1`:** Refactor the core runtime. Before executing any function, it will first check if the function is a local, native one or if it exists in the Agent Registry. This is the birth of dynamic delegation.
    *   **Specification:** [`docs/ccos/specs/008-delegation-engine.md`](./ccos/specs/008-delegation-engine.md)
4.  **Formalize the RTFS Task Protocol:** Define a standard RTFS data structure for tasks that can be sent to agents, including required fields like `:on-success` and `:on-failure` callbacks.
    *   **Specification:** [`docs/ccos/specs/002-plans-and-orchestration.md`](./ccos/specs/002-plans-and-orchestration.md)

---

### **Phase 2: The Economic & Intent-Aware Runtime**

**Goal:** Infuse the Arbiter with economic and semantic awareness, allowing it to make *intelligent* choices, not just mechanical ones.

**Key Steps:**
1.  **Launch the Capability Marketplace V1:** Upgrade the Agent Registry. Agents now register a rich RTFS object containing not just a function name, but basic SLA metadata: `cost`, `provider-id`, `expected-speed`, etc.
    *   **Specification:** [`docs/ccos/specs/004-capabilities-and-marketplace.md`](./ccos/specs/004-capabilities-and-marketplace.md)
2.  **Implement the Global Function Mesh V1:** Create the universal naming system. This can start as a centralized service that maps universal function names (e.g., `image-processing/sharpen`) to active listings in the Capability Marketplace.
    *   **Specification:** [`docs/ccos/specs/007-global-function-mesh.md`](./ccos/specs/007-global-function-mesh.md)
3.  **Develop the Language of Intent V1:** Create a simple data structure to be passed with each plan, containing user preferences like `(priority :speed)` or `(constraint :max-cost 5)`.
    *   **Specification:** [`docs/ccos/specs/001-intent-graph.md`](./ccos/specs/001-intent-graph.md)
4.  **Upgrade to `ArbiterV2`:** The Arbiter now uses the Intent data to select the *best* provider from the Marketplace based on the user's stated priorities, rather than just picking the first one it finds.
    *   **Specification:** [`docs/ccos/specs/006-arbiter-and-cognitive-control.md`](./ccos/specs/006-arbiter-and-cognitive-control.md)

---

### **Phase 3: The Constitutional & Reflective Mind**

**Goal:** Build the core safety, auditing, and learning frameworks. This is where the system becomes truly "cognitive" and trustworthy.

**Key Steps:**
1.  **Implement the Causal Chain of Thought V1:** The runtime now logs every significant action (the function call, the chosen agent, the result, the cost) to an immutable, append-only ledger. This provides perfect auditability and is the prerequisite for self-reflection.
    *   **Specification:** [`docs/ccos/specs/003-causal-chain.md`](./ccos/specs/003-causal-chain.md)
2.  **Introduce the Ethical Governance Framework V1:** Implement a "pre-flight check" in the Arbiter. Before executing any plan, it is validated against a set of hard-coded RTFS rules (the "constitution"). Execution is halted on any violation.
    *   **Specification:** [`docs/ccos/specs/010-ethical-governance.md`](./ccos/specs/010-ethical-governance.md)
3.  **Develop the "Subconscious" V1 (The Analyst):** Create a separate, offline process that reads the Causal Chain ledger. Its first job is simple: analysis and reporting. It can identify the most expensive functions, the least reliable agents, or patterns of failure, providing crucial insights to human developers.
    *   **Specification:** [`docs/ccos/specs/013-working-memory.md`](./ccos/specs/013-working-memory.md)

---

### **Phase 4: The Living & Learning Ecosystem**

**Goal:** Enable the system to grow, learn, and defend itself, moving towards true autonomy and resilience.

**Key Steps:**
1.  **Launch the Federation of Minds V1:** Instantiate multiple, specialized Arbiters (e.g., a `LogicArbiter` for deterministic code, a `CreativeArbiter` for generative tasks). A "meta-arbiter" routes incoming tasks to the appropriate specialist.
    *   **Specification:** [`docs/ccos/specs/006-arbiter-and-cognitive-control.md`](./ccos/specs/006-arbiter-and-cognitive-control.md)
2.  **Implement the Immune System V1:** Introduce basic security protocols. Agents on the Marketplace must register with a cryptographic signature. The Arbiter verifies these signatures and introduces basic anomaly detection to spot misbehaving agents.
    *   **Specification:** [`docs/ccos/specs/011-capability-attestation.md`](./ccos/specs/011-capability-attestation.md)
3.  **Upgrade the "Subconscious" V2 (The Optimizer):** The Subconscious can now do more than analyze. It can run `what-if` simulations on past events from the ledger. If it finds a provably better strategy, it can *suggest* a new rule or heuristic to the human developers.
    *   **Specification:** [`docs/ccos/specs/013-working-memory.md`](./ccos/specs/013-working-memory.md)
4.  **Develop the Persona V1:** The system begins building a simple user profile, storing key interactions and preferences to inform future decisions and provide a more personalized experience.
    *   **Specification:** [`docs/ccos/specs/005-security-and-context.md`](./ccos/specs/005-security-and-context.md)

---

### **Phase 5: Towards a Symbiotic Architecture**

**Goal:** The system becomes self-modifying, truly collaborative, and begins to embody the full CCOS vision.

**Key Steps:**
1.  **Self-Healing Runtimes:** The Subconscious gains the ability to not just suggest, but to *generate* new, more efficient RTFS or even native Rust code for a function, compile it, and propose a "hot-swap" to the live runtime (pending human approval via the Ethics Committee).
2.  **The Living Intent Graph:** The Language of Intent evolves into a fully interactive, collaborative tool. The Arbiter can now have dialogues with the user to co-create and refine the graph.
3.  **The Digital Ethics Committee:** Formalize the process for updating the Ethical Governance Framework, requiring a multi-signature approval from a designated group of trusted humans to amend the AI's constitution.
4.  **The Empathetic Symbiote:** Begin R&D on the multi-modal, ambient user interface, creating the first true cognitive partner.
