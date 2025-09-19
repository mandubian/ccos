pub mod discovery;
pub mod executors;
pub mod mcp_discovery;
pub mod types;
pub mod marketplace;
pub mod resource_monitor;

// Export types and CapabilityMarketplace struct (but not its impl from types.rs)
pub use types::{CapabilityMarketplace, CapabilityManifest, CapabilityAttestation, CapabilityProvenance, 
                CapabilityIsolationPolicy, ResourceConstraints, 
                ResourceUsage, ResourceViolation, ResourceType, TimeConstraints, 
                NamespacePolicy, NetworkRegistryConfig, StreamCapabilityImpl, ProviderType,
                CapabilityDiscovery, CapabilityExecutor};
// Note: marketplace implementation lives in `marketplace.rs` and provides
// the actual method implementations for CapabilityMarketplace