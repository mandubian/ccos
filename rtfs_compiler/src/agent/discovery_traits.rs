// Agent Discovery Traits - Decoupled interface for agent discovery
// This module provides traits that allow the runtime to discover agents
// without circular dependencies on agent implementation types.

use serde_json::Value as JsonValue;
use crate::runtime::RuntimeError;

/// Query parameters for agent discovery (simplified version for trait interface)
#[derive(Debug, Clone)]
pub struct SimpleDiscoveryQuery {
    pub capability_id: Option<String>,
    pub version_constraint: Option<String>,
    pub agent_id: Option<String>,
    pub discovery_tags: Option<Vec<String>>,
    pub discovery_query: Option<String>,
    pub limit: Option<u32>,
}

/// Options for agent discovery behavior (simplified version for trait interface)
#[derive(Debug, Clone)]
pub struct SimpleDiscoveryOptions {
    pub timeout_ms: Option<u64>,
    pub cache_policy: Option<SimpleCachePolicy>,
    pub include_offline: Option<bool>,
    pub max_results: Option<u32>,
}

/// Cache policy for agent discovery (simplified version for trait interface)
#[derive(Debug, Clone)]
pub enum SimpleCachePolicy {
    UseCache,
    NoCache,
    RefreshCache,
}

/// Simplified agent card representation using JSON values
/// This avoids circular dependencies while maintaining all necessary data
#[derive(Debug, Clone)]
pub struct SimpleAgentCard {
    pub agent_id: String,
    pub name: Option<String>,
    pub version: Option<String>,
    pub capabilities: Vec<String>,
    pub endpoint: Option<String>,
    pub metadata: JsonValue,
}

/// Error types specific to agent discovery
#[derive(Debug, Clone)]
pub enum AgentDiscoveryError {
    NetworkError(String),
    TimeoutError(String),
    ParseError(String),
    RegistryUnavailable(String),
    InvalidQuery(String),
}

impl From<AgentDiscoveryError> for RuntimeError {
    fn from(err: AgentDiscoveryError) -> Self {
        match err {
            AgentDiscoveryError::NetworkError(msg) => RuntimeError::AgentDiscoveryError {
                message: format!("Network error: {}", msg),
                registry_uri: "unknown".to_string(),
            },
            AgentDiscoveryError::TimeoutError(msg) => RuntimeError::AgentDiscoveryError {
                message: format!("Timeout: {}", msg),
                registry_uri: "unknown".to_string(),
            },
            AgentDiscoveryError::ParseError(msg) => RuntimeError::AgentDiscoveryError {
                message: format!("Parse error: {}", msg),
                registry_uri: "unknown".to_string(),
            },
            AgentDiscoveryError::RegistryUnavailable(msg) => RuntimeError::AgentDiscoveryError {
                message: format!("Registry unavailable: {}", msg),
                registry_uri: "unknown".to_string(),
            },
            AgentDiscoveryError::InvalidQuery(msg) => RuntimeError::AgentDiscoveryError {
                message: format!("Invalid query: {}", msg),
                registry_uri: "unknown".to_string(),
            },
        }
    }
}

/// Main trait for agent discovery functionality
/// This trait can be implemented by different discovery backends
pub trait AgentDiscovery: Send + Sync {
    /// Discover agents matching the given query and options
    fn discover_agents(
        &self,
        query: &SimpleDiscoveryQuery,
        options: Option<&SimpleDiscoveryOptions>,
    ) -> Result<Vec<SimpleAgentCard>, AgentDiscoveryError>;

    /// Check if the discovery service is available
    fn is_available(&self) -> bool;

    /// Get the name/identifier of this discovery service
    fn service_name(&self) -> &str;
}

/// Factory trait for creating agent discovery services
pub trait AgentDiscoveryFactory {
    /// Create a new agent discovery service
    fn create_discovery_service(&self) -> Result<Box<dyn AgentDiscovery>, AgentDiscoveryError>;
}

/// Default implementation that returns no agents (for testing/fallback)
pub struct NoOpAgentDiscovery;

impl AgentDiscovery for NoOpAgentDiscovery {
    fn discover_agents(
        &self,
        _query: &SimpleDiscoveryQuery,
        _options: Option<&SimpleDiscoveryOptions>,
    ) -> Result<Vec<SimpleAgentCard>, AgentDiscoveryError> {
        Ok(vec![])
    }

    fn is_available(&self) -> bool {
        true
    }

    fn service_name(&self) -> &str {
        "no-op"
    }
}

impl AgentDiscoveryFactory for NoOpAgentDiscovery {
    fn create_discovery_service(&self) -> Result<Box<dyn AgentDiscovery>, AgentDiscoveryError> {
        Ok(Box::new(NoOpAgentDiscovery))
    }
}
