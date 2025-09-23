# Caching Architecture: L2 Inference Cache

_Status: **SPECIFICATION** – Ready for implementation_

---

## 1. Purpose

The **Layer 2 Inference Cache** is designed to reduce latency and API costs by memoizing the results of expensive Large Language Model (LLM) inference calls. The `Arbiter` and other AI-driven components rely on these inferences, and caching them is critical for achieving interactive performance and operational efficiency.

## 2. Core Architecture

The L2 cache is a multi-stage key-value store that intercepts requests sent from the CCOS to external or locally-hosted LLMs.

-   **Key:** A 64-bit hash of an `InferenceRequest` struct. This key must be deterministic and capture all inputs that could alter the LLM's output, including:
    -   The Model ID (e.g., `claude-3-opus-20240229`).
    -   The full prompt content, including any system prompts, few-shot examples, and user queries.
    -   Core sampling parameters (e.g., `temperature`, `top_p`, `max_tokens`).
-   **Value:** The `InferenceResponse` struct, containing the generated text, token usage statistics, stop reason, and any other metadata returned by the provider.
-   **Technology:** A hybrid storage approach is required to balance speed and persistence.
    -   **Hot Cache (In-Memory):** A Redis instance for extremely fast lookups of recent or frequently used inferences.
    -   **Cold Cache (Persistent Storage):** A scalable, persistent object store (e.g., S3, MinIO, or a dedicated database) to act as the long-term, auditable source of truth for all historical inferences.

### Architectural Diagram

```
+-----------------+ 1. Inference Request +-------------------+ | Arbiter/Agent | ----------------------> | L2 Cache Client | +-----------------+ +-------------------+ | | 2. Check Hot Cache | +----------------v----------------+ | L2 Hot Cache (Redis) | +----------------^----------------+ | | 6. Write to Hot Cache (TTL) | 3. Cache Miss | +----------------v----------------+ | L2 Cold Cache (S3/DB) | +----------------^----------------+ | | 5. Write to Cold Cache | 4. Cache Miss | +----------------v----------------+ | LLM Provider API | +---------------------------------+
```

## 3. Implementation Plan

### Milestone 1: Hot Cache (Redis)

**Goal:** Implement a basic, high-speed cache to handle exact-match lookups.

1.  **Create `InferenceCache` Client:**
    *   Develop a client within the CCOS's LLM service abstraction layer.
    *   Add the `redis-rs` crate as a dependency.
    *   The client will connect to a Redis instance defined in the CCOS configuration.
2.  **Implement Hashing:**
    *   Create a stable hashing function for the `InferenceRequest` struct. Ensure the hash is consistent across different CCOS nodes.
3.  **Modify Inference Logic:**
    *   Before dispatching a request to an LLM provider, the client hashes the request and queries Redis.
    *   **On a cache hit,** the client immediately returns the stored `InferenceResponse`, bypassing the LLM provider entirely.
    *   **On a cache miss,** the client proceeds with the API call. Upon receiving a successful response, it writes the `(request_hash, response)` pair to Redis with a configurable TTL (e.g., 24 hours).
4.  **Add Metrics:**
    *   Implement counters for `l2_cache_hits` and `l2_cache_misses`.

### Milestone 2: Cold Cache (Persistent Storage)

**Goal:** Add a long-term, persistent storage layer for auditability, fine-tuning, and resilience.

1.  **Introduce Blob Storage Client:**
    *   Add a dependency for a blob storage provider (e.g., `aws-sdk-s3` for S3/MinIO).
    *   Create a `PersistentCacheProvider` that can write to and read from a configured bucket.
2.  **Update Cache Logic:**
    *   The `InferenceCache` client's logic is now multi-layered:
        1.  Check Hot Cache (Redis). On hit, return.
        2.  On miss, check Cold Cache (S3). On hit, return the object and write it back to the Hot Cache to promote it.
        3.  On miss from both, call the LLM provider.
3.  **Asynchronous Writes:**
    *   Writes to the Cold Cache should be asynchronous (fire-and-forget) to avoid adding latency to the inference path. The successful response from the LLM is first written to the Hot Cache and returned to the user, while the write to persistent storage happens in the background.

### Milestone 3: Semantic Cache (Vector Search)

**Goal:** Go beyond exact-match lookups by finding and reusing semantically similar, but not identical, past inferences.

1.  **Introduce Vector Database:**
    *   Integrate a vector database client (e.g., PGVector, Weaviate, Pinecone).
    *   Integrate a sentence-transformer model for generating embeddings.
2.  **Dual-Write Process:**
    *   When a new inference is stored in the Cold Cache, the system also generates a vector embedding from the core *intent* of the prompt.
    *   This embedding is stored in the vector database, linked to the hash/ID of the full inference in the Cold Cache.
3.  **Semantic Search on Miss:**
    *   If a request misses both the Hot and Cold caches, the client generates an embedding for the new prompt and performs a similarity search in the vector database.
    *   If a result is found with a similarity score above a certain threshold (e.g., >0.95), the system can retrieve the corresponding response from the Cold Cache and return it directly. This makes the cache resilient to minor prompt variations (e.g., whitespace, rephrasing).

## 4. Performance & Cost Goals

-   **P99 Latency (Hot Cache Hit):** < 10ms
-   **P99 Latency (Cold Cache Hit):** < 50ms
-   **Target API Cost Reduction:** > 40% for typical, repetitive workloads.
-   **Target Hit Rate (Exact Match):** > 60%
-   **Target Hit Rate (Semantic Match):** Additional 15-20% lift over exact match.
