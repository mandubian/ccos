//! Recursive capability discovery and generation engine
//!
//! This module implements the discovery pipeline for finding and synthesizing
//! capabilities needed to fulfill user goals. It follows a recursive approach
//! where missing capabilities trigger their own refinement cycles.

pub mod capability_matcher;
pub mod config;
pub mod cycle_detector;
pub mod embedding_service;
pub mod engine;
pub mod intent_transformer;
pub mod introspection_cache;
pub mod local_synthesizer;
pub mod need_extractor;
pub mod recursive_synthesizer;

pub use capability_matcher::*;
pub use config::*;
pub use cycle_detector::*;
pub use embedding_service::*;
pub use engine::*;
pub use intent_transformer::*;
pub use introspection_cache::*;
pub use local_synthesizer::*;
pub use need_extractor::*;
pub use recursive_synthesizer::*;
