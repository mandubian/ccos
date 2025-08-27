//! Runtime Capabilities module
//!
//! This module groups runtime-level capability implementations (providers, HTTP, etc.).
//! It keeps execution/broker concerns in the runtime layer and leaves policy/governance to `ccos`.

pub mod providers;

/// Common re-exports for ergonomic imports
pub mod prelude {
    // Provider implementations
    pub use super::providers::{GitHubMCPCapability, WeatherMCPCapability, LocalLlmProvider};
    // Core capability wrappers and helpers
    pub use crate::runtime::capability::{Capability, inject_capability};
    pub use crate::runtime::capability_provider::{
        CapabilityProvider, CapabilityDescriptor, SecurityRequirements, Permission,
        NetworkAccess, ResourceLimits, HealthStatus, ProviderConfig, ProviderMetadata,
        ExecutionContext,
    };
}
