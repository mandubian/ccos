//! Intent Graph Module
//!
//! This module implements the Living Intent Graph - a dynamic, multi-layered data structure
//! that stores and manages user intents with their relationships and lifecycle.

pub mod config;
pub mod core;
pub mod processing;
pub mod query;
pub mod search;
pub mod storage;
pub mod virtualization;

// Re-export main types for convenience
pub use config::*;
pub use core::*;
pub use processing::*;
pub use query::*;
pub use search::*;
pub use storage::*;
pub use virtualization::*;

#[cfg(test)]
mod tests;
