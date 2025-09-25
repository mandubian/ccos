//! RTFS Bridge - CCOS layer for extracting and managing CCOS objects from RTFS expressions
//!
//! This module provides the bridge between RTFS and CCOS, allowing CCOS objects like
//! Plans and Intents to be represented as standard RTFS expressions (FunctionCall or Map)
//! and then extracted and validated at the CCOS layer.

pub mod converters;
pub mod errors;
pub mod extractors;
pub mod graph_interpreter;
pub mod validators;

pub use converters::*;
pub use errors::*;
pub use extractors::*;
pub use graph_interpreter::*;
pub use validators::*;
