//! Recursive capability discovery and generation engine
//!
//! This module implements the discovery pipeline for finding and synthesizing
//! capabilities needed to fulfill user goals. It follows a recursive approach
//! where missing capabilities trigger their own refinement cycles.

pub mod apis_guru;
pub mod capability_matcher;
pub mod config;
pub mod cycle_detector;
pub mod discovery_agent;
pub mod embedding_service;
pub mod engine;
pub mod goal_discovery;
pub mod intent_transformer;
pub mod introspection_cache;
pub mod llm_discovery;
// Note: LocalSynthesizer removed - synthesis now handled by LLM-based RecursiveSynthesizer
pub mod need_extractor;
// Note: RecursiveSynthesizer removed from discovery - synthesis delegated to planner
// pub mod recursive_synthesizer;
pub mod registry_search;

pub use apis_guru::*;
pub use capability_matcher::*;
pub use config::*;
pub use cycle_detector::*;
pub use discovery_agent::*;
pub use embedding_service::*;
pub use engine::*;
pub use goal_discovery::*;
pub use intent_transformer::*;
pub use introspection_cache::*;
pub use llm_discovery::*;
// pub use local_synthesizer::*; // Removed
pub use need_extractor::*;
// pub use recursive_synthesizer::*; // Removed
pub use registry_search::*;
