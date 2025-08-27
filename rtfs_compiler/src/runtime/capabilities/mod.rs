//! Runtime Capabilities module
//!
//! This module groups runtime-level capability primitives and implementations (contracts, registry, providers).
//! It keeps execution/broker concerns in the runtime layer and leaves policy/governance to `ccos`.

pub mod provider;
pub mod capability;
pub mod registry;
pub mod providers;

// /// Common re-exports for ergonomic imports
// pub mod prelude {
//     // Provider implementations
//     pub use super::providers::{GitHubMCPCapability, WeatherMCPCapability, LocalLlmProvider};
//     // Core capability wrappers and helpers
//     pub use super::capability::{Capability, inject_capability};
//     pub use super::capability_provider::{
//         CapabilityProvider, CapabilityDescriptor, SecurityRequirements, Permission,
//         NetworkAccess, ResourceLimits, HealthStatus, ProviderConfig, ProviderMetadata,
//         ExecutionContext,
//     };
// }
