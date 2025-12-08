//! Inline RTFS adapters for bridging schema mismatches
//!
//! This module provides utilities for detecting when tool outputs
//! don't match downstream capability inputs, and generates inline
//! RTFS expressions to bridge the gap.

mod schema_bridge;

pub use schema_bridge::{load_capability_sample, AdapterKind, SchemaBridge};
