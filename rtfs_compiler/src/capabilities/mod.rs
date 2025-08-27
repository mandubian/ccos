//! Backward-compat shim for capabilities.
//! Please migrate imports to `crate::runtime::capabilities::prelude::*` or
//! `crate::runtime::capabilities::providers::*`.

pub use crate::runtime::capabilities::providers::{
	GitHubMCPCapability,
	WeatherMCPCapability,
};

// Optionally re-export provider traits and types for ergonomics
pub use crate::runtime::capability_provider::{
	CapabilityProvider, CapabilityDescriptor, SecurityRequirements, Permission,
	NetworkAccess, ResourceLimits, HealthStatus, ProviderConfig, ProviderMetadata,
	ExecutionContext,
};
