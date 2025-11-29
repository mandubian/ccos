//! Dialogue-based capability synthesis.
//!
//! This module contains the dialogue synthesis pipeline for creating
//! capabilities from conversation interactions:
//! - Parameter schema extraction from conversations
//! - Collector and planner artifact generation
//! - Skill extraction from dialogue
//! - Preference schema handling

pub mod artifact_generator;
pub mod capability_synthesizer;
pub mod preference_schema;
pub mod schema_builder;
pub mod skill_extractor;

// Re-export commonly used types
pub use artifact_generator::{generate_collector, generate_planner};
pub use capability_synthesizer::CapabilitySynthesizer;
pub use preference_schema::{extract_with_metrics, ParamType};
pub use schema_builder::{extract_param_schema, ParamSchema};
pub use skill_extractor::*;
