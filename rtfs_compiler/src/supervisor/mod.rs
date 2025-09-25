//! Supervisor module for RTFS/CCOS
//!
//! This module provides functionality for supervising MicroVM deployments,
//! including spec synthesis and Firecracker integration.

pub mod spec_synth;

pub use spec_synth::*;
