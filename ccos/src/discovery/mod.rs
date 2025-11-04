//! Recursive capability discovery and generation engine
//!
//! This module implements the discovery pipeline for finding and synthesizing
//! capabilities needed to fulfill user goals. It follows a recursive approach
//! where missing capabilities trigger their own refinement cycles.

pub mod need_extractor;
pub mod engine;
pub mod cycle_detector;
pub mod intent_transformer;
pub mod recursive_synthesizer;
pub mod introspection_cache;
pub mod capability_matcher;
pub mod embedding_service;
pub mod local_synthesizer;

pub use need_extractor::*;
pub use engine::*;
pub use cycle_detector::*;
pub use intent_transformer::*;
pub use recursive_synthesizer::*;
pub use introspection_cache::*;
pub use capability_matcher::*;
pub use embedding_service::*;
pub use local_synthesizer::*;

