// RTFS Agent System - Core module for agent discovery, communication, and management
// Implements the complete agent system as specified in agent_discovery.md

pub mod types;
pub mod registry;
pub mod discovery;
pub mod communication;
pub mod profile;

pub use types::*;
pub use registry::AgentRegistry;
pub use discovery::AgentDiscoveryClient;
pub use communication::AgentCommunicationClient;
pub use profile::AgentProfileManager;
