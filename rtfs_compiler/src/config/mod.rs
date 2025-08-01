//! Configuration management for RTFS agents
//! 
//! This module provides configuration structures and validation for RTFS agents,
//! including MicroVM deployment profiles and security policies.

pub mod validation_microvm;
pub mod types;

pub use types::*;
pub use validation_microvm::*; 