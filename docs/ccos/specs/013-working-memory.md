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
        MI["Memory Ingestor (Async Process)"];
        WM[Working Memory (Indexed Store)];
    end

    subgraph "Orchestration Layer"
        CH[Context Horizon];
    end
    
    subgraph "Cognitive Layer"
        Arbiter[Arbiter];
    end

    CC -- "1. Raw Actions are written to" --> Orch((Orchestrator));
    Orch -- "2. Ingestor reads from" --> MI;
    MI -- "3. Processes & populates" --> WM;
    CH -- "4. Queries for relevant context" --> WM;
    CH -- "5. Builds payload for" --> Arbiter;
```

### 3.1. The Memory Ingestor

-   The **Memory Ingestor** is a background, asynchronous system process.
-   It subscribes to the `Causal Chain` and processes new `Action` entries as they are committed.
-   Its role is to transform the raw log data into structured, indexed formats suitable for the Working Memory store. For example, it might extract user preferences, summarize conversations, or calculate relationship strengths in the Intent Graph.

### 3.2. The Working Memory Store

-   This is not a single database but a logical component that may be implemented using multiple specialized databases optimized for different types of queries.
-   **Key-Value Store**: For storing discrete facts like user settings (e.g., `user:jane.prefers_dark_mode = true`).
-   **Vector Database**: For storing embeddings of conversations and documents, enabling fast semantic search and retrieval of relevant memories.
-   **Graph Database**: For maintaining an indexed and easily traversable copy of the `Living Intent Graph`, including inferred relationships.
-   **Time-Series Database**: For tracking resource usage and system metrics over time.

The Working Memory is considered ephemeral and rebuildable. If the store is corrupted or lost, it can be completely reconstructed by re-running the Memory Ingestor over the entire Causal Chain.

## 4. Impact on System Components

-   **Causal Chain (`SEP-003`)**: Its role is unchanged. It remains the immutable, permanent source of truth for the system's history.
-   **Context Horizon (`SEP-009`)**: Its role is significantly clarified. The Context Horizon is now the primary *consumer* of the Working Memory. Its main function is to query the various stores within the Working Memory to assemble a concise, relevant, and token-aware context payload for the Arbiter. It no longer needs to perform complex queries on the raw Causal Chain.
-   **Arbiter (`SEP-006`)**: The Arbiter benefits from much faster and more relevant context, leading to higher quality and more efficient planning and interaction.
-   **System Architecture (`SEP-000`)**: The master architecture must be updated to include the `Memory Ingestor` and `Working Memory` as key components of the Data & State Layer.
