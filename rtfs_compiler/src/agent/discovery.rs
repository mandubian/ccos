// RTFS Agent Discovery Client - Handles communication with agent discovery registries
// Implements the client side of the JSON-RPC protocol defined in agent_discovery.md

use std::time::Duration;
use reqwest::Client;
use serde_json::{json, Value as JsonValue};
use uuid::Uuid;

use crate::runtime::error::{RuntimeError, RuntimeResult};
use super::types::*;

/// Client for communicating with agent discovery registries
pub struct AgentDiscoveryClient {
    http_client: Client,
    default_registry_uri: Option<String>,
    default_timeout: Duration,
}

impl AgentDiscoveryClient {
    /// Create a new discovery client
    pub fn new() -> Self {
        Self {
            http_client: Client::new(),
            default_registry_uri: None,
            default_timeout: Duration::from_secs(10),
        }
    }
    
    /// Create a new discovery client with default registry
    pub fn with_registry(registry_uri: String) -> Self {
        Self {
            http_client: Client::new(),
            default_registry_uri: Some(registry_uri),
            default_timeout: Duration::from_secs(10),
        }
    }
    
    /// Set the default registry URI
    pub fn set_default_registry(&mut self, registry_uri: String) {
        self.default_registry_uri = Some(registry_uri);
    }
    
    /// Set the default timeout
    pub fn set_default_timeout(&mut self, timeout: Duration) {
        self.default_timeout = timeout;
    }
    
    /// Discover agents based on query criteria
    /// Implements the (discover-agents ...) special form functionality
    pub async fn discover_agents(
        &self, 
        query: &DiscoveryQuery, 
        options: Option<&DiscoveryOptions>
    ) -> RuntimeResult<Vec<AgentCard>> {
        let registry_uri = self.get_registry_uri(options)?;
        let timeout = self.get_timeout(options);
        
        let request_id = Uuid::new_v4().to_string();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "rtfs.registry.discover".to_string(),
            params: self.query_to_json(query)?,
            id: request_id.clone(),
        };
        
        let response = self.http_client
            .post(&registry_uri)
            .json(&request)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| RuntimeError::AgentDiscoveryError {
                message: format!("Failed to connect to registry: {}", e),
                registry_uri: registry_uri.clone(),
            })?;
            
        if !response.status().is_success() {
            return Err(RuntimeError::AgentDiscoveryError {
                message: format!("Registry returned error status: {}", response.status()),
                registry_uri,
            });
        }
        
        let rpc_response: JsonRpcResponse = response.json().await
            .map_err(|e| RuntimeError::AgentDiscoveryError {
                message: format!("Failed to parse registry response: {}", e),
                registry_uri: registry_uri.clone(),
            })?;
            
        if let Some(error) = rpc_response.error {
            return Err(RuntimeError::AgentDiscoveryError {
                message: format!("Registry error: {} (code: {})", error.message, error.code),
                registry_uri,
            });
        }
        
        let result = rpc_response.result.ok_or_else(|| RuntimeError::AgentDiscoveryError {
            message: "Registry response missing result".to_string(),
            registry_uri: registry_uri.clone(),
        })?;
        
        let discovery_response: DiscoveryResponse = serde_json::from_value(result)
            .map_err(|e| RuntimeError::AgentDiscoveryError {
                message: format!("Failed to parse discovery response: {}", e),
                registry_uri,
            })?;
            
        Ok(discovery_response.agents)
    }
    
    /// Register an agent with the registry
    pub async fn register_agent(
        &self,
        agent_card: &AgentCard,
        endpoint_url: String,
        ttl_seconds: Option<u64>,
        registry_uri: Option<String>
    ) -> RuntimeResult<RegistrationResponse> {
        let registry_uri = registry_uri
            .or_else(|| self.default_registry_uri.clone())
            .ok_or_else(|| RuntimeError::AgentDiscoveryError {
                message: "No registry URI specified".to_string(),
                registry_uri: "unknown".to_string(),
            })?;
            
        let request_id = Uuid::new_v4().to_string();
        let registration_request = RegistrationRequest {
            agent_card: agent_card.clone(),
            endpoint_url,
            ttl_seconds,
        };
        
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "rtfs.registry.register".to_string(),
            params: serde_json::to_value(registration_request)
                .map_err(|e| RuntimeError::AgentDiscoveryError {
                    message: format!("Failed to serialize registration request: {}", e),
                    registry_uri: registry_uri.clone(),
                })?,
            id: request_id,
        };
        
        let response = self.http_client
            .post(&registry_uri)
            .json(&request)
            .timeout(self.default_timeout)
            .send()
            .await
            .map_err(|e| RuntimeError::AgentDiscoveryError {
                message: format!("Failed to connect to registry: {}", e),
                registry_uri: registry_uri.clone(),
            })?;
            
        if !response.status().is_success() {
            return Err(RuntimeError::AgentDiscoveryError {
                message: format!("Registry returned error status: {}", response.status()),
                registry_uri,
            });
        }
        
        let rpc_response: JsonRpcResponse = response.json().await
            .map_err(|e| RuntimeError::AgentDiscoveryError {
                message: format!("Failed to parse registry response: {}", e),
                registry_uri: registry_uri.clone(),
            })?;
            
        if let Some(error) = rpc_response.error {
            return Err(RuntimeError::AgentDiscoveryError {
                message: format!("Registry error: {} (code: {})", error.message, error.code),
                registry_uri,
            });
        }
        
        let result = rpc_response.result.ok_or_else(|| RuntimeError::AgentDiscoveryError {
            message: "Registry response missing result".to_string(),
            registry_uri: registry_uri.clone(),
        })?;
        
        let registration_response: RegistrationResponse = serde_json::from_value(result)
            .map_err(|e| RuntimeError::AgentDiscoveryError {
                message: format!("Failed to parse registration response: {}", e),
                registry_uri,
            })?;
            
        Ok(registration_response)
    }
    
    /// Unregister an agent from the registry
    pub async fn unregister_agent(
        &self,
        agent_id: String,
        registry_uri: Option<String>
    ) -> RuntimeResult<()> {
        let registry_uri = registry_uri
            .or_else(|| self.default_registry_uri.clone())
            .ok_or_else(|| RuntimeError::AgentDiscoveryError {
                message: "No registry URI specified".to_string(),
                registry_uri: "unknown".to_string(),
            })?;
            
        let request_id = Uuid::new_v4().to_string();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "rtfs.registry.unregister".to_string(),
            params: json!({"agent_id": agent_id}),
            id: request_id,
        };
        
        let response = self.http_client
            .post(&registry_uri)
            .json(&request)
            .timeout(self.default_timeout)
            .send()
            .await
            .map_err(|e| RuntimeError::AgentDiscoveryError {
                message: format!("Failed to connect to registry: {}", e),
                registry_uri: registry_uri.clone(),
            })?;
            
        if !response.status().is_success() {
            return Err(RuntimeError::AgentDiscoveryError {
                message: format!("Registry returned error status: {}", response.status()),
                registry_uri,
            });
        }
        
        let rpc_response: JsonRpcResponse = response.json().await
            .map_err(|e| RuntimeError::AgentDiscoveryError {
                message: format!("Failed to parse registry response: {}", e),
                registry_uri: registry_uri.clone(),
            })?;
            
        if let Some(error) = rpc_response.error {
            return Err(RuntimeError::AgentDiscoveryError {
                message: format!("Registry error: {} (code: {})", error.message, error.code),
                registry_uri,
            });
        }
        
        Ok(())
    }
    
    /// Health check for registry connectivity
    pub async fn health_check(&self, registry_uri: Option<String>) -> RuntimeResult<bool> {
        let registry_uri = registry_uri
            .or_else(|| self.default_registry_uri.clone())
            .ok_or_else(|| RuntimeError::AgentDiscoveryError {
                message: "No registry URI specified".to_string(),
                registry_uri: "unknown".to_string(),
            })?;
            
        let request_id = Uuid::new_v4().to_string();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "rtfs.registry.health".to_string(),
            params: json!({}),
            id: request_id,
        };
        
        let response = self.http_client
            .post(&registry_uri)
            .json(&request)
            .timeout(Duration::from_secs(5)) // Shorter timeout for health checks
            .send()
            .await;
            
        match response {
            Ok(resp) if resp.status().is_success() => Ok(true),
            _ => Ok(false),
        }
    }
    
    // Helper methods
    
    fn get_registry_uri(&self, options: Option<&DiscoveryOptions>) -> RuntimeResult<String> {
        options
            .and_then(|opts| opts.registry_uri.clone())
            .or_else(|| self.default_registry_uri.clone())
            .ok_or_else(|| RuntimeError::AgentDiscoveryError {
                message: "No registry URI specified and no default configured".to_string(),
                registry_uri: "unknown".to_string(),
            })
    }
    
    fn get_timeout(&self, options: Option<&DiscoveryOptions>) -> Duration {
        options
            .and_then(|opts| opts.timeout_ms)
            .map(Duration::from_millis)
            .unwrap_or(self.default_timeout)
    }
    
    fn query_to_json(&self, query: &DiscoveryQuery) -> RuntimeResult<JsonValue> {
        let mut params = serde_json::Map::new();
        
        if let Some(capability_id) = &query.capability_id {
            params.insert("capability_id".to_string(), JsonValue::String(capability_id.clone()));
        }
        
        if let Some(version_constraint) = &query.version_constraint {
            params.insert("version_constraint".to_string(), JsonValue::String(version_constraint.clone()));
        }
        
        if let Some(agent_id) = &query.agent_id {
            params.insert("agent_id".to_string(), JsonValue::String(agent_id.clone()));
        }
        
        if let Some(discovery_tags) = &query.discovery_tags {
            let tags: Vec<JsonValue> = discovery_tags.iter()
                .map(|tag| JsonValue::String(tag.clone()))
                .collect();
            params.insert("discovery_tags".to_string(), JsonValue::Array(tags));
        }
        
        if let Some(discovery_query) = &query.discovery_query {
            for (key, value) in discovery_query {
                params.insert(key.clone(), value.clone());
            }
        }
        
        if let Some(limit) = query.limit {
            params.insert("limit".to_string(), JsonValue::Number(limit.into()));
        }
        
        Ok(JsonValue::Object(params))
    }
}

impl Default for AgentDiscoveryClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_discovery_query_creation() {
        let query = DiscoveryQuery::new()
            .with_capability_id("test/capability".to_string())
            .with_tags(vec!["tag1".to_string(), "tag2".to_string()])
            .with_limit(10);
            
        assert_eq!(query.capability_id, Some("test/capability".to_string()));
        assert_eq!(query.discovery_tags, Some(vec!["tag1".to_string(), "tag2".to_string()]));
        assert_eq!(query.limit, Some(10));
    }
    
    #[test]
    fn test_query_to_json() {
        let client = AgentDiscoveryClient::new();
        let query = DiscoveryQuery::new()
            .with_capability_id("test/capability".to_string())
            .with_limit(5);
            
        let json = client.query_to_json(&query).unwrap();
        
        assert!(json.get("capability_id").is_some());
        assert!(json.get("limit").is_some());
        assert_eq!(json["capability_id"], "test/capability");
        assert_eq!(json["limit"], 5);
    }
}