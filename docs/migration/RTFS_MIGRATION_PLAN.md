# RTFS 1.0 to CCOS: A Phased Migration Plan

**Date:** June 23, 2025
**Status:** In Progress

## 1. Overview

This document outlines the detailed, phased migration plan for evolving RTFS from its current state (version 1.0) into the Cognitive Computing Operating System (CCOS) as envisioned in `docs/vision/SENTIENT_RUNTIME_VISION.md`.

The core principles of this migration are:
- **Evolution, not Revolution:** We will build upon the existing RTFS foundation, ensuring backward compatibility where possible and providing clear migration paths.
- **Phased Rollout:** The migration is broken down into distinct phases, allowing for incremental development, testing, and deployment.
- **Documentation First:** We will define and document the changes before implementing them in the codebase.

## 2. Current State (RTFS 1.0)

- **Language:** A monolithic `Task` object. Homoiconic, LISP-like syntax.
- **Runtime:** A compiler and interpreter for RTFS code (`rtfs_compiler`).
- **Documentation:** Located in `docs/specs`, `docs/implementation`, etc.

## 3. Target State (CCOS / RTFS 2.0)

- **Language (RTFS 2.0):** A modular protocol with first-class objects: `Intent`, `Plan`, `Action`, `Capability`, `Resource`. Features a formal namespacing and versioning system.
- **Architecture (CCOS):** An LLM-based Arbiter for dynamic execution, a Living Intent Graph, a Generative Capability Marketplace, and a Causal Chain of Thought for auditing.
- **Documentation:** Reorganized into `docs/rtfs-1.0` (legacy), `docs/rtfs-2.0` (new spec), `docs/vision`, `docs/roadmap`, and `docs/migration`.

---

## 4. Migration Phases

### Phase 1: Documentation & Project Reorganization (Completed)

*Goal: To create a clean, well-organized foundation for the migration.*

| Step | Task | Status | Owner | ETA |
| :--- | :--- | :--- | :--- | :--- |
| 1.1 | Create `docs/migration/` directory. | ✅ Done | AI | 2025-06-22 |
| 1.2 | Create this migration plan (`RTFS_MIGRATION_PLAN.md`). | ✅ Done | AI | 2025-06-23 |
| 1.3 | Create `docs/rtfs-1.0/` directory for legacy specs. | ✅ Done | AI | 2025-06-23 |
| 1.4 | Move existing RTFS 1.0 specifications from `docs/specs` to `docs/rtfs-1.0/specs`. | ✅ Done | AI | 2025-06-23 |
| 1.5 | Move existing implementation docs from `docs/implementation` to `docs/rtfs-1.0/implementation`. | ✅ Done | AI | 2025-06-23 |
| 1.6 | Create `docs/rtfs-2.0/` directory for future CCOS/RTFS 2.0 specifications. | ✅ Done | AI | 2025-06-23 |
| 1.7 | Draft the new root `README.md` to reflect the CCOS vision and migration plan. | ✅ Done | AI | 2025-06-23 |
| 1.8 | Update `docs/DOCUMENTATION_ORGANIZATION_SUMMARY.md` to reflect the new structure. | ✅ Done | AI | 2025-06-23 |

### Phase 2: Language Evolution (RTFS 2.0 Specification)

*Goal: To formally define the RTFS 2.0 language specification.*

| Step | Task | Status | Owner | ETA |
| :--- | :--- | :--- | :--- | :--- |
| 2.1 | Draft `docs/rtfs-2.0/specs/01-core-objects.md`: Define `Intent`, `Plan`, `Action`, `Capability`, `Resource`. | ⬜ To Do | | |
| 2.2 | Draft `docs/rtfs-2.0/specs/02-namespacing-and-versioning.md`: Define the `:ns:version:type` syntax and resolution rules. | ⬜ To Do | | |
| 2.3 | Draft `docs/rtfs-2.0/specs/03-data-model.md`: Define the full data model and serialization format. | ⬜ To Do | | |
| 2.4 | Draft `docs/rtfs-2.0/specs/04-backward-compatibility.md`: Define strategy for handling RTFS 1.0 `Task` objects. | ⬜ To Do | | |

### Phase 3: CCOS Architecture Specification

*Goal: To formally define the components of the Cognitive Computing Operating System.*

| Step | Task | Status | Owner | ETA |
| :--- | :--- | :--- | :--- | :--- |
| 3.1 | Draft `docs/rtfs-2.0/architecture/01-arbiter.md`: Define the Arbiter's role, decision-making logic, and execution delegation. | ⬜ To Do | | |
| 3.2 | Draft `docs/rtfs-2.0/architecture/02-intent-graph.md`: Define the structure, lifecycle, and management of the Living Intent Graph. | ⬜ To Do | | |
| 3.3 | Draft `docs/rtfs-2.0/architecture/03-capability-marketplace.md`: Define the Capability object, marketplace mechanics, and discovery via the Global Function Mesh. | ⬜ To Do | | |
| 3.4 | Draft `docs/rtfs-2.0/architecture/04-causal-chain.md`: Define the `Action` object structure and the immutable ledger for auditing. | ⬜ To Do | | |
| 3.5 | Draft `docs/rtfs-2.0/architecture/05-governance.md`: Define the Constitutional AI principles and the role of the Digital Ethics Committee. | ⬜ To Do | | |

### Phase 4: Implementation & Code Migration

*Goal: To build the CCOS and migrate the existing codebase.*

| Step | Task | Status | Owner | ETA |
| :--- | :--- | :--- | :--- | :--- |
| 4.1 | **Compiler:** Refactor `rtfs_compiler` to support RTFS 2.0 syntax, including namespacing and new core objects. | ⬜ To Do | | |
| 4.2 | **Compiler:** Implement a compatibility mode or transpiler for RTFS 1.0 `Task` objects. | ⬜ To Do | | |
| 4.3 | **Runtime:** Develop the core Arbiter prototype. Initially, it can be a simple rule-based engine. | ⬜ To Do | | |
| 4.4 | **Runtime:** Implement basic storage and retrieval for the Intent Graph and Causal Chain (e.g., using a local database). | ⬜ To Do | | |
| 4.5 | **Tooling:** Update IDE extensions and debugging tools for RTFS 2.0. | ⬜ To Do | | |
| 4.6 | **Testing:** Create a comprehensive test suite for all new components. | ⬜ To Do | | |
