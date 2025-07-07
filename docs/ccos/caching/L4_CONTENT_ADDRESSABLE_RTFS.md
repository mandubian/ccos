# Caching Architecture: L4 Content-Addressable RTFS

_Status: **SPECIFICATION** – Ready for implementation_

---

## 1. Purpose

The **Layer 4 Content-Addressable RTFS** is the deepest and most powerful layer of the caching hierarchy. It fundamentally redefines what a "cache hit" is. Instead of caching the *results* of an LLM's reasoning (like L1-L3), this layer caches the **compiled, executable output** of that reasoning: the RTFS bytecode itself.

Its purpose is to completely bypass the entire LLM reasoning and code generation pipeline for previously solved problems. If a request can be satisfied by an existing, verified RTFS module, the system can execute it directly, offering unparalleled performance, cost savings, and reliability.

This layer transforms the CCOS from a system that *generates* code to one that *retrieves and reuses* code, treating its own compiled artifacts as a library of trusted, reusable components.

## 2. Core Architecture

The L4 cache is a content-addressable storage system where the key is derived from a semantic understanding of the task and the value is the compiled RTFS bytecode.

-   **Key:** A composite key, including:
    1.  **Semantic Hash:** A vector embedding of the task description, similar to the L3 cache, but potentially using a more sophisticated model trained to understand computational intent.
    2.  **Interface Signature:** A stable hash of the required inputs and expected outputs (the function signature).
-   **Value:** The compiled, verified, and signed RTFS bytecode module.
-   **Technology:**
    -   **Primary Storage:** A highly durable and available blob storage (e.g., S3, Azure Blob Storage) for the RTFS bytecode.
    -   **Metadata & Indexing Database:** A robust database (e.g., PostgreSQL, DynamoDB) to store the mapping between the composite key (semantic hash, interface signature) and the storage location of the bytecode.
    -   **Code Signing:** A mechanism to cryptographically sign verified RTFS modules to ensure their integrity and provenance.

### Architectural Diagram

```
+-----------------+ 1. L3 Cache Miss +-------------------+ | L3 Cache Client | ----------------------> | L4 RTFS Cache | +-----------------+ +-------------------+ | | 2. Generate Semantic Key | +----------------v----------------+ | Embedding & Hashing Service | +----------------^----------------+ | | 3. Query Metadata Index | +----------------v----------------+ | RTFS Metadata DB | +---------------------------------+ | | 4. High-Confidence Match Found | +----------------v----------------+ | RTFS Bytecode Store (S3) | <--- 5. Fetch Bytecode --- (DB returns location) +---------------------------------+ | | 6. Load & Execute in RTFS VM
```

## 3. Implementation Plan

### Milestone 1: The RTFS Module Repository

**Goal:** Establish the core infrastructure for storing and indexing compiled RTFS modules.

1.  **Setup Blob Storage:**
    *   Provision an S3 bucket or equivalent blob storage container. This will be the canonical store for all reusable RTFS bytecode.
2.  **Design Metadata Database Schema:**
    *   Create a database table (`rtfs_modules`) with columns for:
        *   `id` (Primary Key)
        *   `semantic_embedding` (Vector Type)
        *   `interface_hash` (VARCHAR/STRING)
        *   `storage_pointer` (e.g., S3 object key)
        *   `validation_status` (e.g., 'Verified', 'Pending')
        *   `signature` (VARCHAR/STRING)
        *   `creation_timestamp`, `last_used_timestamp`
3.  **Develop a `ModulePublisher` Service:**
    *   Create an internal service responsible for taking a newly compiled and verified RTFS module, generating its metadata, and publishing it to the repository.
    *   This service will be invoked at the end of a successful code generation and validation cycle.

### Milestone 2: The "Publish to L4" Path

**Goal:** Integrate the publishing process into the main CCOS workflow.

1.  **Trigger on Successful Execution:**
    *   After the CCOS successfully generates, compiles, and **verifies** a new RTFS program (e.g., through automated tests or even manual sign-off), a new step is added to the workflow.
2.  **Generate Composite Key:**
    *   The original `InferenceRequest` (or a summarized version of it) is used to generate the semantic embedding.
    *   The compiler provides the function signature of the generated code, which is then hashed to create the `interface_hash`.
3.  **Sign and Publish:**
    *   The compiled bytecode is cryptographically signed by a CCOS system key.
    *   The `ModulePublisher` service is called with the bytecode and its metadata. It uploads the bytecode to blob storage and creates a new entry in the metadata database.

### Milestone 3: The L4 Retrieval Path

**Goal:** Query the L4 cache before falling back to the full LLM code generation process.

1.  **Integrate into the Delegation Engine:**
    *   The Delegation Engine, upon receiving a task, will first query the L4 cache after an L3 miss.
2.  **Generate Query Key:**
    *   It generates a semantic embedding and interface hash from the incoming request, using the same methods as the publishing path.
3.  **Perform Hybrid Search:**
    *   The L4 cache service first queries the metadata database for an exact match on the `interface_hash`.
    *   If multiple modules share the same interface, it then performs a semantic similarity search on the `semantic_embedding` to find the best match above a high-confidence threshold (e.g., `0.99`).
4.  **Handle Hits and Misses:**
    *   **On a hit,** the service retrieves the `storage_pointer` and `signature` from the database. It fetches the bytecode from blob storage, verifies its signature, and then loads it directly into the RTFS VM for execution. The entire LLM pipeline is bypassed.
    *   **On a miss,** the request proceeds to the LLM for the standard code generation process.

## 4. Performance & Governance

-   **P99 Latency (L4 Hit):** < 100ms (includes metadata query, S3 fetch, and VM loading).
-   **Target Hit Rate:** Even a 5-10% hit rate for common, well-defined tasks represents a massive gain in efficiency and reliability.
-   **Security and Trust:**
    *   **Code Signing is non-negotiable.** Only modules with a valid signature from the CCOS authority can be executed from the L4 cache.
    *   **Immutable Storage:** Bytecode stored in the repository must be treated as immutable. Updates are handled by publishing new versions.
-   **Governance and Monitoring:**
    *   Every L4 hit must be logged, including the request, the ID of the retrieved module, and its execution outcome.
    *   A dashboard should track L4 usage statistics, identifying the most frequently used modules. This provides invaluable insight into common user tasks and can guide future development of pre-compiled standard libraries.
