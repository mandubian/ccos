// Agent Discovery Traits under ccos::agent
use crate::ccos::agent::types::{SimpleAgentCard, SimpleDiscoveryOptions, SimpleDiscoveryQuery};
use crate::runtime::error::{RuntimeError, RuntimeResult};

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

pub trait AgentDiscovery: Send + Sync {
    fn discover_agents(
        &self,
        query: &SimpleDiscoveryQuery,
        options: Option<&SimpleDiscoveryOptions>,
    ) -> RuntimeResult<Vec<SimpleAgentCard>>;
    fn is_available(&self) -> bool;
    fn service_name(&self) -> &str;
}

pub trait AgentDiscoveryFactory {
    fn create_discovery_service(&self) -> RuntimeResult<Box<dyn AgentDiscovery>>;
}

pub struct NoOpAgentDiscovery;

impl AgentDiscovery for NoOpAgentDiscovery {
    fn discover_agents(
        &self,
        _query: &SimpleDiscoveryQuery,
        _options: Option<&SimpleDiscoveryOptions>,
    ) -> RuntimeResult<Vec<SimpleAgentCard>> {
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
    fn create_discovery_service(&self) -> RuntimeResult<Box<dyn AgentDiscovery>> {
        Ok(Box::new(NoOpAgentDiscovery))
    }
}
