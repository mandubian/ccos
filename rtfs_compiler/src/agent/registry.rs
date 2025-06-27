// RTFS Agent Registry - Service for agent registration and discovery
// Implements the server side of the JSON-RPC protocol defined in agent_discovery.md

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH, Duration};

use crate::runtime::error::{RuntimeError, RuntimeResult};
use super::types::*;

/// Agent Registry - manages agent registration and discovery
pub struct AgentRegistry {
    /// Registered agents indexed by agent_id
    agents: Arc<RwLock<HashMap<String, RegisteredAgent>>>,
    
    /// Default TTL for agent registrations (in seconds)
    default_ttl: u64,
    
    /// Maximum number of agents that can be registered
    max_agents: usize,
}

/// Internal representation of a registered agent
#[derive(Debug, Clone)]
struct RegisteredAgent {
    agent_card: AgentCard,
    endpoint_url: String,
    registered_at: u64,
    expires_at: u64,
    last_health_check: Option<u64>,
    health_status: HealthStatus,
}

/// Health status of a registered agent
#[derive(Debug, Clone, PartialEq)]
enum HealthStatus {
    Unknown,
    Healthy,
    Unhealthy,
}

impl AgentRegistry {
    /// Create a new agent registry
    pub fn new() -> Self {
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
            default_ttl: 3600, // 1 hour default
            max_agents: 10000,  // Reasonable default limit
        }
    }
    
    /// Create a new agent registry with custom settings
    pub fn with_settings(default_ttl: u64, max_agents: usize) -> Self {
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
            default_ttl,
            max_agents,
        }
    }
    
    /// Register an agent with the registry
    pub fn register_agent(
        &self,
        agent_card: AgentCard,
        endpoint_url: String,
        ttl_seconds: Option<u64>
    ) -> RuntimeResult<RegistrationResponse> {
        let mut agents = self.agents.write().map_err(|_| {
            RuntimeError::InternalError("Failed to acquire write lock on agents".to_string())
        })?;
        
        // Check if we're at capacity
        if agents.len() >= self.max_agents && !agents.contains_key(&agent_card.agent_id) {
            return Err(RuntimeError::AgentDiscoveryError {
                message: format!("Registry at capacity ({} agents)", self.max_agents),
                registry_uri: "local".to_string(),
            });
        }
        
        let now = current_timestamp();
        let ttl = ttl_seconds.unwrap_or(self.default_ttl);
        let expires_at = now + ttl;
        
        let registered_agent = RegisteredAgent {
            agent_card: agent_card.clone(),
            endpoint_url: endpoint_url.clone(),
            registered_at: now,
            expires_at,
            last_health_check: None,
            health_status: HealthStatus::Unknown,
        };
        
        agents.insert(agent_card.agent_id.clone(), registered_agent);
        
        Ok(RegistrationResponse {
            status: "registered".to_string(),
            agent_id: agent_card.agent_id.clone(),
            expires_at: format_timestamp(expires_at),
        })
    }
    
    /// Unregister an agent from the registry
    pub fn unregister_agent(&self, agent_id: &str) -> RuntimeResult<()> {
        let mut agents = self.agents.write().map_err(|_| {
            RuntimeError::InternalError("Failed to acquire write lock on agents".to_string())
        })?;
        
        if agents.remove(agent_id).is_some() {
            Ok(())
        } else {
            Err(RuntimeError::AgentDiscoveryError {
                message: format!("Agent {} not found", agent_id),
                registry_uri: "local".to_string(),
            })
        }
    }
    
    /// Discover agents based on query criteria
    pub fn discover_agents(&self, query: &DiscoveryQuery) -> RuntimeResult<Vec<AgentCard>> {
        let agents = self.agents.read().map_err(|_| {
            RuntimeError::InternalError("Failed to acquire read lock on agents".to_string())
        })?;
        
        // Clean up expired agents first
        let now = current_timestamp();
        let active_agents: Vec<&RegisteredAgent> = agents
            .values()
            .filter(|agent| agent.expires_at > now)
            .collect();
        
        // Apply filters
        let mut matching_agents: Vec<&RegisteredAgent> = active_agents;
        
        // Filter by agent_id if specified
        if let Some(agent_id) = &query.agent_id {
            matching_agents.retain(|agent| agent.agent_card.agent_id == *agent_id);
        }
        
        // Filter by capability_id if specified
        if let Some(capability_id) = &query.capability_id {
            matching_agents.retain(|agent| {
                agent.agent_card.has_capability(capability_id)
            });
        }
        
        // Filter by discovery tags if specified
        if let Some(query_tags) = &query.discovery_tags {
            matching_agents.retain(|agent| {
                query_tags.iter().all(|query_tag| {
                    agent.agent_card.discovery_tags.contains(query_tag)
                })
            });
        }
        
        // Apply version constraint if specified
        if let Some(version_constraint) = &query.version_constraint {
            if let Some(capability_id) = &query.capability_id {
                matching_agents.retain(|agent| {
                    if let Some(capability) = agent.agent_card.get_capability(capability_id) {
                        if let Some(cap_version) = &capability.version {
                            // Simple version checking - in production would use semver crate
                            check_version_constraint(cap_version, version_constraint)
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                });
            }
        }
        
        // Apply discovery_query filters if specified
        if let Some(discovery_query) = &query.discovery_query {
            matching_agents.retain(|agent| {
                apply_discovery_query_filters(&agent.agent_card, discovery_query)
            });
        }
        
        // Apply limit if specified
        let limit = query.limit.unwrap_or(u32::MAX) as usize;
        if matching_agents.len() > limit {
            matching_agents.truncate(limit);
        }
        
        // Convert to AgentCard vector
        let result: Vec<AgentCard> = matching_agents
            .into_iter()
            .map(|agent| agent.agent_card.clone())
            .collect();
            
        Ok(result)
    }
    
    /// Get statistics about the registry
    pub fn get_stats(&self) -> RuntimeResult<RegistryStats> {
        let agents = self.agents.read().map_err(|_| {
            RuntimeError::InternalError("Failed to acquire read lock on agents".to_string())
        })?;
        
        let now = current_timestamp();
        let total_agents = agents.len();
        let active_agents = agents.values()
            .filter(|agent| agent.expires_at > now)
            .count();
        let expired_agents = total_agents - active_agents;
        
        let healthy_agents = agents.values()
            .filter(|agent| agent.health_status == HealthStatus::Healthy)
            .count();
            
        Ok(RegistryStats {
            total_agents,
            active_agents,
            expired_agents,
            healthy_agents,
            max_capacity: self.max_agents,
        })
    }
    
    /// Clean up expired agents
    pub fn cleanup_expired(&self) -> RuntimeResult<usize> {
        let mut agents = self.agents.write().map_err(|_| {
            RuntimeError::InternalError("Failed to acquire write lock on agents".to_string())
        })?;
        
        let now = current_timestamp();
        let before_count = agents.len();
        
        agents.retain(|_, agent| agent.expires_at > now);
        
        let removed_count = before_count - agents.len();
        Ok(removed_count)
    }
    
    /// Health check for all registered agents
    pub async fn health_check_all(&self) -> RuntimeResult<usize> {
        // This would implement health checking logic
        // For now, just return the count of agents that would be checked
        let agents = self.agents.read().map_err(|_| {
            RuntimeError::InternalError("Failed to acquire read lock on agents".to_string())
        })?;
        
        Ok(agents.len())
    }
    
    /// Get a specific agent by ID
    pub fn get_agent(&self, agent_id: &str) -> RuntimeResult<Option<AgentCard>> {
        let agents = self.agents.read().map_err(|_| {
            RuntimeError::InternalError("Failed to acquire read lock on agents".to_string())
        })?;
        
        if let Some(registered_agent) = agents.get(agent_id) {
            let now = current_timestamp();
            if registered_agent.expires_at > now {
                Ok(Some(registered_agent.agent_card.clone()))
            } else {
                Ok(None) // Expired
            }
        } else {
            Ok(None)
        }
    }
    
    /// List all active agents
    pub fn list_agents(&self) -> RuntimeResult<Vec<AgentCard>> {
        let agents = self.agents.read().map_err(|_| {
            RuntimeError::InternalError("Failed to acquire read lock on agents".to_string())
        })?;
        
        let now = current_timestamp();
        let active_agents: Vec<AgentCard> = agents
            .values()
            .filter(|agent| agent.expires_at > now)
            .map(|agent| agent.agent_card.clone())
            .collect();
            
        Ok(active_agents)
    }
}

/// Registry statistics
#[derive(Debug, Clone)]
pub struct RegistryStats {
    pub total_agents: usize,
    pub active_agents: usize,
    pub expired_agents: usize,
    pub healthy_agents: usize,
    pub max_capacity: usize,
}

// Helper functions

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_secs()
}

fn format_timestamp(timestamp: u64) -> String {
    // In production, would use chrono for proper formatting
    format!("{}000", timestamp) // Simple epoch timestamp
}

fn check_version_constraint(version: &str, constraint: &str) -> bool {
    // Simplified version checking - in production would use semver crate
    // For now, just do basic string comparison
    if constraint.starts_with(">=") {
        let required = constraint.trim_start_matches(">=").trim();
        version >= required
    } else if constraint.starts_with("<=") {
        let required = constraint.trim_start_matches("<=").trim();
        version <= required
    } else if constraint.starts_with('>') {
        let required = constraint.trim_start_matches('>').trim();
        version > required
    } else if constraint.starts_with('<') {
        let required = constraint.trim_start_matches('<').trim();
        version < required
    } else {
        version == constraint
    }
}

fn apply_discovery_query_filters(agent_card: &AgentCard, discovery_query: &HashMap<String, serde_json::Value>) -> bool {
    for (key, value) in discovery_query {
        match key.as_str() {
            "name_contains" => {
                if let Some(search_term) = value.as_str() {
                    if !agent_card.name.to_lowercase().contains(&search_term.to_lowercase()) {
                        return false;
                    }
                }
            },
            "text_search" => {
                if let Some(search_term) = value.as_str() {
                    let search_term = search_term.to_lowercase();
                    if !(agent_card.name.to_lowercase().contains(&search_term) ||
                         agent_card.description.to_lowercase().contains(&search_term)) {
                        return false;
                    }
                }
            },
            "min_capabilities" => {
                if let Some(min_count) = value.as_u64() {
                    if (agent_card.capabilities.len() as u64) < min_count {
                        return false;
                    }
                }
            },
            _ => {
                // Unknown filter - ignore for now
            }
        }
    }
    true
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_agent_registration() {
        let registry = AgentRegistry::new();
        let agent_card = AgentCard::new(
            "test-agent-1".to_string(),
            "Test Agent".to_string(),
            "1.0.0".to_string(),
            "A test agent".to_string()
        );
        
        let result = registry.register_agent(
            agent_card,
            "http://localhost:8080".to_string(),
            Some(3600)
        );
        
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.agent_id, "test-agent-1");
        assert_eq!(response.status, "registered");
    }
    
    #[test]
    fn test_agent_discovery() {
        let registry = AgentRegistry::new();
        
        // Register a test agent
        let mut agent_card = AgentCard::new(
            "test-agent-1".to_string(),
            "Test Agent".to_string(),
            "1.0.0".to_string(),
            "A test agent".to_string()
        );
        agent_card.add_capability(AgentCapability::new(
            "test/capability".to_string(),
            "Test capability".to_string()
        ));
        
        registry.register_agent(
            agent_card,
            "http://localhost:8080".to_string(),
            Some(3600)
        ).unwrap();
        
        // Test discovery by capability
        let query = DiscoveryQuery::new()
            .with_capability_id("test/capability".to_string());
            
        let results = registry.discover_agents(&query).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].agent_id, "test-agent-1");
    }
    
    #[test]
    fn test_version_constraint() {
        assert!(check_version_constraint("1.2.0", ">=1.0.0"));
        assert!(!check_version_constraint("0.9.0", ">=1.0.0"));
        assert!(check_version_constraint("1.0.0", "1.0.0"));
    }
}