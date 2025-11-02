//! Recursive capability discovery and generation engine
//!
//! This module implements the discovery pipeline for finding and synthesizing
//! capabilities needed to fulfill user goals. It follows a recursive approach
//! where missing capabilities trigger their own refinement cycles.

pub mod need_extractor;
pub mod engine;

pub use need_extractor::*;
pub use engine::*;

