pub mod config_mcp_discovery;
pub mod discovery;
pub mod executors;
pub mod marketplace;
pub mod mcp_discovery;
pub mod resource_monitor;
pub mod types;
pub mod versioning;

// Export types and CapabilityMarketplace struct (but not its impl from types.rs)
pub use types::{
    CapabilityAttestation, CapabilityDiscovery, CapabilityExecutor, CapabilityIsolationPolicy,
    CapabilityManifest, CapabilityMarketplace, CapabilityProvenance, NamespacePolicy,
    NetworkRegistryConfig, ProviderType, ResourceConstraints, ResourceType, ResourceUsage,
    ResourceViolation, StreamCapabilityImpl, TimeConstraints,
};
// Note: marketplace implementation lives in `marketplace.rs` and provides
// the actual method implementations for CapabilityMarketplace
