//! Working Memory module
//!
//! Purpose:
//! - Provide a compact, queryable recall layer derived from the Causal Chain.
//! - Act as the primary persistence and retrieval surface for distilled "wisdom"
//!   used by Context Horizon and Arbiter.
//!
//! Structure:
//! - types.rs: shared types (ids, entries, metadata), small and documented
//! - backend.rs: WorkingMemoryBackend trait and error types
//! - backend_inmemory.rs: default in-memory + JSONL append-only backend
//! - facade.rs: thin facade wrapping a boxed backend (stable surface for callers)
//! - ingestor.rs: MemoryIngestor skeleton (subscribe/replay + derivation)
//! - boundaries.rs: Boundary, BoundaryType, ReductionStrategy for CH integration
//!
//! Testing strategy:
//! - Unit tests colocated in each file for the specific component
//! - Integration tests placed under /tests for end-to-end flows

pub mod backend;
pub mod backend_inmemory;
pub mod boundaries;
pub mod facade;
pub mod ingestor;
pub mod types;

pub use backend::{QueryParams, QueryResult, WorkingMemoryBackend, WorkingMemoryError};
pub use backend_inmemory::InMemoryJsonlBackend;
pub use boundaries::{Boundary, BoundaryType, ReductionStrategy};
pub use facade::WorkingMemory;
pub use ingestor::{DerivedEntry, MemoryIngestor, WorkingMemorySink};
pub use types::{WorkingMemoryEntry, WorkingMemoryId, WorkingMemoryMeta};
