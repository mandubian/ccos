# CCOS Specification 013: Working Memory

**Status:** Proposed
**Version:** 1.0
**Date:** 2025-07-20
**Related:**
- [SEP-000: System Architecture](./000-ccos-architecture.md)
- [SEP-003: Causal Chain](./003-causal-chain.md)
- [SEP-009: Context Horizon](./009-context-horizon.md)

## 1. Abstract

This specification defines the **Working Memory** component, a high-performance caching and indexing layer that sits between the immutable `Causal Chain` and the cognitive components of the CCOS. It addresses the need for efficient, summarized recall of past interactions, which is critical for the Arbiter's performance and the relevance of the `Context Horizon`. The Causal Chain is the perfect, auditable long-term memory, while the Working Memory is the fast, structured short-term memory.

## 2. Core Problem

The `Causal Chain` is designed for perfect, immutable auditability. It is an append-only log, which makes it poorly suited for the rapid, complex queries required to build a relevant context for the Arbiter (e.g., "find all of the user's stated preferences from the last month"). Forcing the Arbiter or Context Horizon to scan the entire raw chain for every interaction would be computationally expensive and slow, hindering the system's conversational ability.

The Working Memory solves this by providing a digested, indexed, and query-optimized representation of the data stored in the Causal Chain.

## 3. Architecture

The Working Memory architecture consists of two new elements and one modified existing component.

```mermaid
graph TD
    subgraph "Data & State Layer"
        CC[Causal Chain (Source of Truth)];
        MI["Memory Ingestor (Async Subscriber)"];
        WM[Working Memory (Pluggable Indexed Store)];
    end

    subgraph "Orchestration Layer"
        CH[Context Horizon];
    end
    
    subgraph "Cognitive Layer"
        Arbiter[Arbiter];
    end

    CC -- "1. Actions appended" --> Orch((Orchestrator));
    Orch -- "2. Ingestor subscribes to append feed" --> MI;
    MI -- "3. Derives & persists distilled entries" --> WM;
    CH -- "4. Queries for relevant wisdom" --> WM;
    CH -- "5. Builds token-aware payload" --> Arbiter;
```

### 3.1. The Memory Ingestor

-   The **Memory Ingestor** is a background, asynchronous subscriber to the Causal Chain append feed.
-   On each `Action` committed, it derives one or more `WorkingMemoryEntry` items with strong provenance.
-   Responsibilities:
    - Normalize action data and optionally summarize long content.
    - Derive tags from action type (e.g., `capability-call`, `plan-lifecycle`, `policy-violation`) and domain.
    - Populate `meta` with provenance: `action_id`, `plan_id`, `intent_id`, optional `step_id`, `provider`, `attestation_hash`, `content_hash`.
    - Compute `approx_tokens` for budget-aware retrieval and compaction.
    - Persist via a pluggable `WorkingMemoryBackend`.
-   Delivery semantics:
    - At-least-once; idempotency ensured by combining `(action_id, content_hash, timestamp)`.
    - Rebuild mode: replay full Causal Chain from genesis to reconstruct WM.

### 3.2. The Working Memory Store

-   Logical component with pluggable backends specialized for query patterns.
-   Backends:
    - In-memory + JSONL append-only persistence (default, rebuildable).
    - Vector backend (optional) for semantic retrieval over embeddings.
    - Graph backend (optional) for relationship traversal.
    - Time-series backend (optional) for temporal analytics.
-   Query semantics:
    - Time-windowed queries, tag-based selection (extendable to AND/NOT), recency-based ranking; future scoring/time-decay.
-   Compaction:
    - Enforce `max_entries_in_memory` and `max_tokens_in_memory` with oldest-first eviction.
-   Rebuildability:
    - Entire WM can be reconstructed by re-running the Memory Ingestor over the Causal Chain.

## 4. Impact on System Components

-   Causal Chain (`SEP-003`): Unchanged as the immutable source of truth.
-   Context Horizon (`SEP-009`): Becomes the primary consumer of WM. It queries WM for relevant distilled wisdom within defined boundaries (e.g., time window, token budgets), merges with any fresh distillation, deduplicates, then reduces to fit token constraints. It avoids direct complex queries over the raw chain.
-   Arbiter (`SEP-006`): Receives faster, more relevant, and token-constrained context, improving quality and efficiency.
-   System Architecture (`SEP-000`): The Data & State Layer includes `Memory Ingestor` and `Working Memory` as first-class components with pluggable backends. Governance can enforce provenance and attestation checks on WM derivations.
