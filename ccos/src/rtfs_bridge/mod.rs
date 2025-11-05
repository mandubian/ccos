//! RTFS Bridge - CCOS layer for extracting and managing CCOS objects from RTFS expressions
//!
//! This module provides the bridge between RTFS and CCOS, allowing CCOS objects like
//! Plans and Intents to be represented as standard RTFS expressions (FunctionCall or Map)
//! and then extracted and validated at the CCOS layer.

pub mod canonical_schemas;
pub mod converters;
pub mod effects_propagation;
pub mod errors;
pub mod extractors;
pub mod graph_interpreter;
pub mod language_utils;
pub mod normalizer;
pub mod plan_as_capability;
pub mod validators;

pub use canonical_schemas::*;
pub use converters::*;
pub use effects_propagation::*;
pub use errors::*;
pub use extractors::*;
pub use graph_interpreter::*;
pub use language_utils::*;
pub use normalizer::*;
pub use plan_as_capability::*;
pub use validators::*;
