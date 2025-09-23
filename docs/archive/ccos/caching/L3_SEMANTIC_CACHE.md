# Caching Architecture: L3 Semantic Cache

_Status: **SPECIFICATION** – Ready for implementation_

---

## 1. Purpose

The **Layer 3 Semantic Cache** represents a significant leap in caching intelligence, moving beyond the exact-match limitations of the L1 and L2 caches. Its primary purpose is to increase the overall cache hit rate by identifying and reusing past LLM inferences that are **semantically equivalent** to new requests, even if the prompts are worded differently.

This layer makes the entire caching system more resilient to trivial variations in user input (e.g., whitespace changes, rephrasing, different examples) and dramatically improves performance and cost-efficiency for a wider range of queries.

## 2. Core Architecture

The L3 cache functions as a lookup service that sits between the L2 cache and the LLM provider. It uses vector embeddings to represent the semantic "intent" of a request and a specialized vector database to find close matches.

-   **Key:** A high-dimensional vector embedding generated from the core content of an `InferenceRequest`.
-   **Value:** A reference (the 64-bit hash) to the corresponding `InferenceResponse` stored in the L2 Cold Cache.
-   **Technology:**
    -   **Vector Database:** A dedicated system optimized for high-speed similarity searches on vector data (e.g., Weaviate, Pinecone, Milvus, or PostgreSQL with the `pgvector` extension).
    -   **Embedding Model:** A sentence-transformer model responsible for converting text prompts into dense vector embeddings. The choice of model is critical for balancing speed, cost, and semantic accuracy.
    -   **Similarity Metric:** Cosine Similarity is the standard for comparing the angle between two vectors to determine their semantic closeness.

### Architectural Diagram

```
+-----------------+ 1. L2 Cache Miss +-------------------+ | L2 Cache Client | ----------------------> | L3 Semantic Cache | +-----------------+ +-------------------+ | | 2. Generate Embedding | +----------------v----------------+ | Embedding Model | +----------------^----------------+ | | 3. Query Vector DB | +----------------v----------------+ | Vector Database | +---------------------------------+ | | 4. High-Similarity Match Found (>0.95) | +----------------v----------------+ | L2 Cold Cache (S3/DB) | <--- 5. Fetch by Hash --- (L3 returns hash) +---------------------------------+ | | 6. Return Response & Promote to L1/L2
```

## 3. Implementation Plan

### Milestone 1: Infrastructure Setup

**Goal:** Integrate the necessary components for generating and storing embeddings.

1.  **Select and Deploy Vector Database:**
    *   Provision a vector database instance. For initial development, a managed service or a Docker container (e.g., Weaviate) is recommended.
    *   Define the database schema: a collection/index that stores a vector and its corresponding L2 cache hash.
2.  **Integrate Embedding Model:**
    *   Choose an initial embedding model (e.g., a fast and effective model like `all-MiniLM-L6-v2` from Sentence-Transformers).
    *   Create an `EmbeddingService` within the CCOS that can take a string of text and return a vector embedding. This service should be configurable to support different models in the future.
3.  **Establish Configuration:**
    *   Add configuration parameters to the CCOS for the vector database endpoint, API keys, and the selected embedding model.

### Milestone 2: The Dual-Write Path

**Goal:** Populate the vector database whenever a new inference is added to the L2 cache.

1.  **Modify L2 Cold Cache Write Logic:**
    *   Extend the asynchronous "fire-and-forget" write to the L2 Cold Cache.
    *   After successfully writing the inference to blob storage, the process now also triggers the `EmbeddingService`.
2.  **Generate and Store Embedding:**
    *   The service generates an embedding from the most salient part of the `InferenceRequest` prompt (typically the user's direct query, excluding generic system prompts).
    *   The resulting vector and the L2 request hash are written to the vector database. This must be a robust process with error handling and retries.

### Milestone 3: The Semantic Search Path

**Goal:** Query the vector database on an L2 cache miss to find a semantic match.

1.  **Update L2 Cache Miss Logic:**
    *   When a request misses both the L1 Hot and L2 Cold caches, the `InferenceCache` client now calls the `L3 Semantic Cache` service before dispatching to the LLM provider.
2.  **Generate Query Embedding:**
    *   The L3 service generates a new embedding for the incoming request's prompt using the same `EmbeddingService`.
3.  **Perform Similarity Search:**
    *   The service queries the vector database with the new embedding, searching for the nearest neighbors with a similarity score above a configurable threshold.
    *   **Similarity Threshold:** This is a critical parameter. Start with a high value (e.g., `0.98`) to ensure high-quality matches and prevent false positives. It must be tunable.
4.  **Handle Hits and Misses:**
    *   **On a semantic hit,** the vector database returns the L2 hash of a similar past request. The L3 service passes this hash back to the L2 client, which then fetches the full response from the Cold Cache and promotes it to the Hot Cache.
    *   **On a semantic miss,** the L3 service signals to proceed with the external LLM API call.

### Milestone 4: Advanced - Response Adaptation (Future Work)

This is a forward-looking concept not intended for the ainitial implementation.

-   **Concept:** For matches that are close but below the high-confidence threshold (e.g., similarity between 0.90 and 0.98), the retrieved response could be used as a "scaffold."
-   **Process:** Instead of returning the cached response directly, the system would feed both the original prompt and the retrieved similar response into a smaller, faster, and cheaper LLM. The prompt would instruct this smaller model to "adapt the provided answer to fit the new question."
-   **Benefit:** This could further increase the effective hit rate and reduce costs, while tailoring the response more precisely to the user's specific query.

## 4. Performance & Governance

-   **P99 Latency (L3 Hit):** < 250ms (includes embedding generation and vector search).
-   **Target Hit Rate Lift:** Achieve an additional 15-25% cache hit rate on top of L1/L2 exact matches for diverse workloads.
-   **Embedding Cost:** The cost of generating embeddings must be tracked, but it is expected to be less than 5% of the cost of a full LLM inference.
-   **Governance & Monitoring:**
    *   All L3 cache hits (query, matched query, similarity score, and final response) **must be logged** for analysis.
    *   A dashboard should be created to monitor the L3 hit rate and the distribution of similarity scores. This data is essential for tuning the similarity threshold and evaluating the effectiveness of the embedding model.
