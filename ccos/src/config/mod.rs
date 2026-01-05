//! Configuration module for CCOS agent runtime
//!
//! This module contains configuration types for the CCOS agent runtime.
//! These types were migrated from `rtfs::config::types` as part of issue #166
//! to properly separate language concerns (RTFS) from runtime concerns (CCOS).

pub mod types;

pub use types::*;

