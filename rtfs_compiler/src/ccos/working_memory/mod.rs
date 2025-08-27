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

pub mod types;
pub mod backend;
pub mod backend_inmemory;
pub mod facade;
pub mod ingestor;
pub mod boundaries;

pub use types::{WorkingMemoryId, WorkingMemoryEntry, WorkingMemoryMeta};
pub use backend::{WorkingMemoryBackend, WorkingMemoryError, QueryParams, QueryResult};
pub use backend_inmemory::InMemoryJsonlBackend;
pub use facade::WorkingMemory;
pub use ingestor::{MemoryIngestor, DerivedEntry, WorkingMemorySink};
pub use boundaries::{Boundary, BoundaryType, ReductionStrategy};
