//! Runtime resolution and trust management.
//!
//! This module contains runtime components for:
//! - Continuous resolution loop for auto-triggering resolution
//! - Server trust management and user interaction
//! - Web search discovery for finding capabilities

pub mod continuous_resolution;
pub mod server_trust;
pub mod web_search_discovery;

// Re-export commonly used types
pub use continuous_resolution::{ContinuousResolutionLoop, ResolutionConfig};
pub use server_trust::{ServerTrustInfo, ServerTrustRegistry, TrustLevel, TrustPolicy};
pub use web_search_discovery::WebSearchDiscovery;
