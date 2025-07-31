//! Intent Graph Module
//!
//! This module implements the Living Intent Graph - a dynamic, multi-layered data structure
//! that stores and manages user intents with their relationships and lifecycle.

pub mod config;
pub mod storage;
pub mod virtualization;
pub mod search;
pub mod processing;
pub mod core;
pub mod query;

// Re-export main types for convenience
pub use config::*;
pub use storage::*;
pub use virtualization::*;
pub use search::*;
pub use processing::*;
pub use core::*;
pub use query::*;

#[cfg(test)]
mod tests;
