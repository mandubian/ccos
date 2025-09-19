use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::runtime::capability_marketplace::types::{CapabilityManifest, ProviderType, A2ACapability};
use crate::runtime::capability_marketplace::types::CapabilityDiscovery;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::timeout;
use std::any::Any;

/// A2A Agent configuration for discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2AAgentConfig {
    /// Agent name/identifier
    pub name: String,
    /// Agent endpoint (e.g., "http://localhost:8080" or "ws://localhost:8081")
    pub endpoint: String,
    /// Authentication token if required
    pub auth_token: Option<String>,
    /// Request timeout in seconds
    pub timeout_seconds: u64,
    /// A2A protocol version
    pub protocol_version: String,
    /// Agent capabilities (if known statically)
    pub known_capabilities: Option<Vec<String>>,
}

impl Default for A2AAgentConfig {
    fn default() -> Self {
        Self {
            name: "default_a2a_agent".to_string(),
            endpoint: "http://localhost:8080".to_string(),
            auth_token: None,
            timeout_seconds: 30,
            protocol_version: "1.0".to_string(),
            known_capabilities: None,
        }
    }
}

/// A2A Capability definition from agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2ACapabilityDefinition {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Option<serde_json::Value>,
    pub output_schema: Option<serde_json::Value>,
    pub parameters: Option<Vec<A2AParameter>>,
}

/// A2A Parameter definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2AParameter {
    pub name: String,
    pub description: Option<String>,
    pub required: bool,
    pub schema: Option<serde_json::Value>,
}

/// A2A Agent response for capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2ACapabilitiesResponse {
    pub capabilities: Vec<A2ACapabilityDefinition>,
    pub error: Option<String>,
}

/// A2A Agent response for status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2AStatusResponse {
    pub status: String,
    pub version: Option<String>,
    pub capabilities: Option<Vec<String>>,
    pub error: Option<String>,
}

/// A2A Discovery Provider for discovering A2A agents and their capabilities
pub struct A2ADiscoveryProvider {
    config: A2AAgentConfig,
    client: reqwest::Client,
}

impl A2ADiscoveryProvider {
    /// Create a new A2A discovery provider
    pub fn new(config: A2AAgentConfig) -> RuntimeResult<Self> {
        let mut client_builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_seconds));

        // Add A2A-specific headers
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_static("CCOS-A2A-Discovery/1.0"),
        );
        headers.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static("application/json"),
        );
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_static("application/json"),
        );

        client_builder = client_builder.default_headers(headers);

        let client = client_builder
            .build()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create A2A HTTP client: {}", e)))?;

        Ok(Self { config, client })
    }

    /// Discover capabilities from the A2A agent
    pub async fn discover_capabilities(&self) -> RuntimeResult<Vec<CapabilityManifest>> {
        let capabilities_url = format!("{}/capabilities", self.config.endpoint);
        
        let mut request_builder = self.client.get(&capabilities_url);

        // Add authentication if provided
        if let Some(token) = &self.config.auth_token {
            request_builder = request_builder.header("Authorization", format!("Bearer {}", token));
        }

        let request = request_builder.build().map_err(|e| {
            RuntimeError::Generic(format!("Failed to build A2A capabilities request: {}", e))
        })?;

        // Execute request with timeout
        let response = timeout(
            Duration::from_secs(self.config.timeout_seconds),
            self.client.execute(request),
        )
        .await
        .map_err(|_| RuntimeError::Generic("A2A capabilities request timeout".to_string()))?
        .map_err(|e| RuntimeError::Generic(format!("A2A capabilities request failed: {}", e)))?;

        // Check status code
        if !response.status().is_success() {
            return Err(RuntimeError::Generic(format!(
                "A2A capabilities HTTP error: {} - {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }

        // Parse response
        let response_text = response.text().await.map_err(|e| {
            RuntimeError::Generic(format!("Failed to read A2A capabilities response: {}", e))
        })?;

        let capabilities_response: A2ACapabilitiesResponse = serde_json::from_str(&response_text).map_err(|e| {
            RuntimeError::Generic(format!("Failed to parse A2A capabilities response: {}", e))
        })?;

        if let Some(error) = capabilities_response.error {
            return Err(RuntimeError::Generic(format!("A2A agent error: {}", error)));
        }

        // Convert A2A capabilities to capability manifests
        let mut capabilities = Vec::new();
        for capability_def in capabilities_response.capabilities {
            let capability = self.convert_capability_to_manifest(capability_def);
            capabilities.push(capability);
        }

        Ok(capabilities)
    }

    /// Get agent status and basic information
    pub async fn get_agent_status(&self) -> RuntimeResult<A2AStatusResponse> {
        let status_url = format!("{}/status", self.config.endpoint);
        
        let mut request_builder = self.client.get(&status_url);

        // Add authentication if provided
        if let Some(token) = &self.config.auth_token {
            request_builder = request_builder.header("Authorization", format!("Bearer {}", token));
        }

        let request = request_builder.build().map_err(|e| {
            RuntimeError::Generic(format!("Failed to build A2A status request: {}", e))
        })?;

        // Execute request with timeout
        let response = timeout(
            Duration::from_secs(self.config.timeout_seconds),
            self.client.execute(request),
        )
        .await
        .map_err(|_| RuntimeError::Generic("A2A status request timeout".to_string()))?
        .map_err(|e| RuntimeError::Generic(format!("A2A status request failed: {}", e)))?;

        // Check status code
        if !response.status().is_success() {
            return Err(RuntimeError::Generic(format!(
                "A2A status HTTP error: {} - {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }

        // Parse response
        let response_text = response.text().await.map_err(|e| {
            RuntimeError::Generic(format!("Failed to read A2A status response: {}", e))
        })?;

        serde_json::from_str(&response_text).map_err(|e| {
            RuntimeError::Generic(format!("Failed to parse A2A status response: {}", e))
        })
    }

    /// Convert an A2A capability definition to a capability manifest
    fn convert_capability_to_manifest(&self, capability_def: A2ACapabilityDefinition) -> CapabilityManifest {
        let capability_id = format!("a2a.{}.{}", self.config.name, capability_def.name);
        
        CapabilityManifest {
            id: capability_id.clone(),
            name: capability_def.name.clone(),
            description: capability_def.description.unwrap_or_else(|| format!("A2A capability: {}", capability_def.name)),
            provider: ProviderType::A2A(A2ACapability {
                agent_id: self.config.name.clone(),
                endpoint: self.config.endpoint.clone(),
                protocol: self.config.protocol_version.clone(),
                timeout_ms: self.config.timeout_seconds * 1000,
            }),
            version: "1.0.0".to_string(),
            input_schema: capability_def.input_schema,
            output_schema: capability_def.output_schema,
            attestation: None,
            provenance: Some(crate::runtime::capability_marketplace::types::CapabilityProvenance {
                source: "a2a_discovery".to_string(),
                version: Some("1.0.0".to_string()),
                content_hash: format!("a2a_{}_{}", self.config.name, capability_def.name),
                custody_chain: vec!["a2a_discovery".to_string()],
                registered_at: chrono::Utc::now(),
            }),
            permissions: vec![],
            metadata: {
                let mut metadata = HashMap::new();
                metadata.insert("a2a_agent".to_string(), self.config.name.clone());
                metadata.insert("a2a_protocol_version".to_string(), self.config.protocol_version.clone());
                metadata.insert("capability_type".to_string(), "a2a_capability".to_string());
                
                // Add parameter information if available
                if let Some(parameters) = capability_def.parameters {
                    let param_names: Vec<String> = parameters.iter().map(|p| p.name.clone()).collect();
                    metadata.insert("parameters".to_string(), param_names.join(","));
                    
                    let required_params: Vec<String> = parameters.iter()
                        .filter(|p| p.required)
                        .map(|p| p.name.clone())
                        .collect();
                    metadata.insert("required_parameters".to_string(), required_params.join(","));
                }
                
                metadata
            },
        }
    }

    /// Discover capabilities using static configuration if dynamic discovery fails
    pub async fn discover_static_capabilities(&self) -> RuntimeResult<Vec<CapabilityManifest>> {
        if let Some(known_caps) = &self.config.known_capabilities {
            let mut capabilities = Vec::new();
            
            for cap_name in known_caps {
                let capability_def = A2ACapabilityDefinition {
                    name: cap_name.clone(),
                    description: Some(format!("Static A2A capability: {}", cap_name)),
                    input_schema: None,
                    output_schema: None,
                    parameters: None,
                };
                
                let capability = self.convert_capability_to_manifest(capability_def);
                capabilities.push(capability);
            }
            
            Ok(capabilities)
        } else {
            Ok(vec![])
        }
    }

    /// Health check for the A2A agent
    pub async fn health_check(&self) -> RuntimeResult<bool> {
        let health_url = format!("{}/health", self.config.endpoint);
        
        let request = self.client
            .get(&health_url)
            .build()
            .map_err(|e| RuntimeError::Generic(format!("Failed to build A2A health check request: {}", e)))?;

        match timeout(
            Duration::from_secs(5), // Shorter timeout for health checks
            self.client.execute(request),
        )
        .await
        {
            Ok(Ok(response)) => Ok(response.status().is_success()),
            Ok(Err(e)) => Err(RuntimeError::Generic(format!("A2A health check failed: {}", e))),
            Err(_) => Err(RuntimeError::Generic("A2A health check timeout".to_string())),
        }
    }
}

#[async_trait]
impl CapabilityDiscovery for A2ADiscoveryProvider {
    async fn discover(&self) -> RuntimeResult<Vec<CapabilityManifest>> {
        let mut all_capabilities = Vec::new();
        
        // Try dynamic discovery first
        match self.discover_capabilities().await {
            Ok(capabilities) => {
                eprintln!("Discovered {} A2A capabilities from agent: {}", capabilities.len(), self.config.name);
                all_capabilities.extend(capabilities);
            }
            Err(e) => {
                eprintln!("A2A dynamic discovery failed for agent {}: {}", self.config.name, e);
                
                // Fall back to static discovery
                match self.discover_static_capabilities().await {
                    Ok(static_capabilities) => {
                        eprintln!("Using {} static A2A capabilities for agent: {}", static_capabilities.len(), self.config.name);
                        all_capabilities.extend(static_capabilities);
                    }
                    Err(static_e) => {
                        eprintln!("A2A static discovery also failed for agent {}: {}", self.config.name, static_e);
                    }
                }
            }
        }
        
        Ok(all_capabilities)
    }

    fn name(&self) -> &str {
        "A2ADiscovery"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Builder for A2A discovery configuration
pub struct A2ADiscoveryBuilder {
    config: A2AAgentConfig,
}

impl A2ADiscoveryBuilder {
    pub fn new() -> Self {
        Self {
            config: A2AAgentConfig::default(),
        }
    }

    pub fn name(mut self, name: String) -> Self {
        self.config.name = name;
        self
    }

    pub fn endpoint(mut self, endpoint: String) -> Self {
        self.config.endpoint = endpoint;
        self
    }

    pub fn auth_token(mut self, token: String) -> Self {
        self.config.auth_token = Some(token);
        self
    }

    pub fn timeout_seconds(mut self, timeout: u64) -> Self {
        self.config.timeout_seconds = timeout;
        self
    }

    pub fn protocol_version(mut self, version: String) -> Self {
        self.config.protocol_version = version;
        self
    }

    pub fn known_capabilities(mut self, capabilities: Vec<String>) -> Self {
        self.config.known_capabilities = Some(capabilities);
        self
    }

    pub fn build(self) -> RuntimeResult<A2ADiscoveryProvider> {
        A2ADiscoveryProvider::new(self.config)
    }
}

impl Default for A2ADiscoveryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_a2a_discovery_config_default() {
        let config = A2AAgentConfig::default();
        assert_eq!(config.timeout_seconds, 30);
        assert_eq!(config.protocol_version, "1.0");
        assert!(config.auth_token.is_none());
    }

    #[test]
    fn test_a2a_discovery_builder() {
        let provider = A2ADiscoveryBuilder::new()
            .name("test_agent".to_string())
            .endpoint("http://localhost:8080".to_string())
            .timeout_seconds(60)
            .protocol_version("1.0".to_string())
            .known_capabilities(vec!["test_capability".to_string()])
            .build();

        assert!(provider.is_ok());
    }

    #[test]
    fn test_convert_capability_to_manifest() {
        let config = A2AAgentConfig {
            name: "test_agent".to_string(),
            endpoint: "http://localhost:8080".to_string(),
            auth_token: None,
            timeout_seconds: 30,
            protocol_version: "1.0".to_string(),
            known_capabilities: None,
        };
        
        let provider = A2ADiscoveryProvider::new(config).unwrap();
        
        let capability_def = A2ACapabilityDefinition {
            name: "test_capability".to_string(),
            description: Some("A test A2A capability".to_string()),
            input_schema: None,
            output_schema: None,
            parameters: None,
        };
        
        let capability = provider.convert_capability_to_manifest(capability_def);
        
        assert_eq!(capability.id, "a2a.test_agent.test_capability");
        assert_eq!(capability.name, "test_capability");
        assert_eq!(capability.metadata.get("capability_type").unwrap(), "a2a_capability");
    }

    #[tokio::test]
    async fn test_discover_static_capabilities() {
        let config = A2AAgentConfig {
            name: "test_agent".to_string(),
            endpoint: "http://localhost:8080".to_string(),
            auth_token: None,
            timeout_seconds: 30,
            protocol_version: "1.0".to_string(),
            known_capabilities: Some(vec!["static_cap1".to_string(), "static_cap2".to_string()]),
        };
        
        let provider = A2ADiscoveryProvider::new(config).unwrap();
        
        let capabilities = provider.discover_static_capabilities().await.unwrap();
        
        assert_eq!(capabilities.len(), 2);
        assert_eq!(capabilities[0].id, "a2a.test_agent.static_cap1");
        assert_eq!(capabilities[1].id, "a2a.test_agent.static_cap2");
    }
}
