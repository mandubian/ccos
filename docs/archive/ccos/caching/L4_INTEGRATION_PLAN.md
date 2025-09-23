# L4 Cache Integration Plan

_Status: **PROPOSAL**_

This document outlines a strategy for integrating the L4 Content-Addressable RTFS Cache into the existing CCOS architecture with minimal disruption.

## Guiding Principles

1.  **Loose Coupling:** The L4 cache logic will be developed in a separate, self-contained module.
2.  **Incremental Implementation:** The integration will follow the phased approach from the original L4 specification.
3.  **Leverage Existing Seams:** We will use the `DelegationEngine` as the primary integration point, treating the L4 cache as a new, high-priority "execution target."

## Phase 1: Core Infrastructure

**Goal:** Establish the foundational components for the L4 cache service.

1.  **Create New Cache Module:**
    *   A new module will be created at `rtfs_compiler/src/ccos/caching/l4_cache.rs`.
    *   This module will encapsulate all logic for interacting with blob storage (e.g., S3) and the metadata database (e.g., PostgreSQL).
    *   It will expose a simple client interface: `L4CacheClient`.

2.  **Define Data Structures:**
    *   Inside the new module, define a Rust struct `RtfsModuleMetadata` that mirrors the database schema described in the L4 spec.

3.  **Implement a `ModulePublisher`:**
    *   Create a function within the `l4_cache` module: `publish_module(bytecode, metadata)`.
    *   This function will be responsible for uploading the bytecode to blob storage and inserting the corresponding metadata into the database. Initially, the storage and DB interactions can be stubbed out.

## Phase 2: The "Publish to L4" Path

**Goal:** Integrate the publishing mechanism into the CCOS workflow.

1.  **Identify the Trigger Point:**
    *   The publishing event should be triggered after a new RTFS module has been successfully generated, compiled, and verified. We need to locate this point in the higher-level orchestration code that manages the LLM-to-code pipeline.
    *   Once located, this code will call the `publish_module` function from Phase 1.

2.  **Composite Key Generation:**
    *   **Interface Hash:** The `rtfs_compiler` will be modified to expose a function that can generate a stable hash of a function's signature.
    *   **Semantic Embedding:** The process that initiates code generation from an `InferenceRequest` will be responsible for generating the semantic embedding and passing it down to the publishing function.

## Phase 3: The L4 Retrieval Path

**Goal:** Query the L4 cache before falling back to other delegation targets.

1.  **Introduce `ExecTarget::L4CacheHit`:**
    *   The `ExecTarget` enum in `rtfs_compiler/src/ccos/delegation.rs` will be extended with a new variant:
        ```rust
        pub enum ExecTarget {
            // ... existing targets
            L4CacheHit { storage_pointer: String, signature: String },
        }
        ```

2.  **Create a Decorator for the `DelegationEngine`:**
    *   To avoid modifying the existing `StaticDelegationEngine` directly, we will create a new struct that wraps it, implementing the `DelegationEngine` trait (Decorator Pattern).
        ```rust
        // In a new file: rtfs_compiler/src/ccos/delegation_l4.rs
        pub struct L4AwareDelegationEngine<DE: DelegationEngine> {
            l4_client: L4CacheClient,
            next_engine: DE,
        }

        impl<DE: DelegationEngine> DelegationEngine for L4AwareDelegationEngine<DE> {
            fn decide(&self, ctx: &CallContext) -> ExecTarget {
                // 1. Attempt to find a match in the L4 cache
                if let Some(metadata) = self.l4_client.query(ctx) {
                    return ExecTarget::L4CacheHit {
                        storage_pointer: metadata.storage_pointer,
                        signature: metadata.signature,
                    };
                }
                // 2. If it's a miss, fallback to the wrapped engine
                self.next_engine.decide(ctx)
            }
        }
        ```

3.  **Enhance `CallContext`:**
    *   The L4 cache requires a semantic hash for its lookup. The current `CallContext` does not provide this. We will augment it:
        ```rust
        pub struct CallContext<'a> {
            // ... existing fields
            pub semantic_hash: Option<Vec<f32>>,
        }
        ```
    *   The code that calls the `DelegationEngine` will be responsible for populating this new field.

4.  **Update the Runtime:**
    *   The `IrRuntime` and/or `evaluator` must be updated to handle the new `L4CacheHit` target.
    *   When this target is received, the runtime will use the `L4CacheClient` to:
        a.  Fetch the bytecode from blob storage via the `storage_pointer`.
        b.  Verify the `signature`.
        c.  Load the bytecode directly into the RTFS VM for execution, bypassing the rest of the pipeline.

This plan establishes a clear path forward. The next step would be to start implementing Phase 1. 