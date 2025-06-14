// RTFS Agent System - Core module for agent discovery, communication, and management
// Implements the complete agent system as specified in agent_discovery.md

pub mod types;
pub mod registry;
pub mod discovery;
pub mod communication;
pub mod profile;
pub mod discovery_traits;

pub use types::*;
pub use registry::AgentRegistry;
pub use discovery::AgentDiscoveryClient;
pub use communication::AgentCommunicationClient;
pub use profile::AgentProfileManager;

// Re-export discovery traits with specific names to avoid conflicts
pub use discovery_traits::{
    AgentDiscovery, AgentDiscoveryFactory, NoOpAgentDiscovery,
    SimpleDiscoveryQuery, SimpleDiscoveryOptions, SimpleAgentCard,
    SimpleCachePolicy, AgentDiscoveryError
};
