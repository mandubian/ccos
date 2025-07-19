// RTFS Capabilities Module
pub mod weather_mcp;

pub use weather_mcp::WeatherMCPCapability;

// Re-export core capability types from runtime
pub use crate::runtime::capability_provider::{
    CapabilityProvider, CapabilityDescriptor, SecurityRequirements, Permission, 
    NetworkAccess, ResourceLimits, HealthStatus, ProviderConfig, ProviderMetadata,
    ExecutionContext
};

// For MCP-specific capability types
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum CapabilityType {
    HTTP,
    MCP,
    A2A,
    Local,
}
