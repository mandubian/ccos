use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::runtime::capability_marketplace::types::{CapabilityManifest, ProviderType, MCPCapability};
use crate::runtime::capability_marketplace::types::CapabilityDiscovery;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::timeout;
use std::any::Any;

/// MCP Server configuration for discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPServerConfig {
    /// Server name/identifier
    pub name: String,
    /// Server endpoint (e.g., "http://localhost:3000" or "ws://localhost:3001")
    pub endpoint: String,
    /// Authentication token if required
    pub auth_token: Option<String>,
    /// Request timeout in seconds
    pub timeout_seconds: u64,
    /// MCP protocol version
    pub protocol_version: String,
}

impl Default for MCPServerConfig {
    fn default() -> Self {
        Self {
            name: "default_mcp_server".to_string(),
            endpoint: "http://localhost:3000".to_string(),
            auth_token: None,
            timeout_seconds: 30,
            protocol_version: "2024-11-05".to_string(),
        }
    }
}

/// MCP Tool definition from server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPTool {
    pub name: String,
    pub description: Option<String>,
    pub inputSchema: Option<serde_json::Value>,
    pub outputSchema: Option<serde_json::Value>,
}

/// MCP Server response for tools
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPToolsResponse {
    pub tools: Vec<MCPTool>,
    pub error: Option<String>,
}

/// MCP Server response for resources
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPResourcesResponse {
    pub resources: Vec<serde_json::Value>,
    pub error: Option<String>,
}

/// MCP Discovery Provider for discovering MCP servers and their tools
pub struct MCPDiscoveryProvider {
    config: MCPServerConfig,
    client: reqwest::Client,
}

impl MCPDiscoveryProvider {
    /// Create a new MCP discovery provider
    pub fn new(config: MCPServerConfig) -> RuntimeResult<Self> {
        let mut client_builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_seconds));

        // Add MCP-specific headers
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_static("CCOS-MCP-Discovery/1.0"),
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
            .map_err(|e| RuntimeError::Generic(format!("Failed to create MCP HTTP client: {}", e)))?;

        Ok(Self { config, client })
    }

    /// Discover tools from the MCP server
    pub async fn discover_tools(&self) -> RuntimeResult<Vec<CapabilityManifest>> {
        let tools_url = format!("{}/tools", self.config.endpoint);
        
        let mut request_builder = self.client.get(&tools_url);

        // Add authentication if provided
        if let Some(token) = &self.config.auth_token {
            request_builder = request_builder.header("Authorization", format!("Bearer {}", token));
        }

        let request = request_builder.build().map_err(|e| {
            RuntimeError::Generic(format!("Failed to build MCP tools request: {}", e))
        })?;

        // Execute request with timeout
        let response = timeout(
            Duration::from_secs(self.config.timeout_seconds),
            self.client.execute(request),
        )
        .await
        .map_err(|_| RuntimeError::Generic("MCP tools request timeout".to_string()))?
        .map_err(|e| RuntimeError::Generic(format!("MCP tools request failed: {}", e)))?;

        // Check status code
        if !response.status().is_success() {
            return Err(RuntimeError::Generic(format!(
                "MCP tools HTTP error: {} - {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }

        // Parse response
        let response_text = response.text().await.map_err(|e| {
            RuntimeError::Generic(format!("Failed to read MCP tools response: {}", e))
        })?;

        let tools_response: MCPToolsResponse = serde_json::from_str(&response_text).map_err(|e| {
            RuntimeError::Generic(format!("Failed to parse MCP tools response: {}", e))
        })?;

        if let Some(error) = tools_response.error {
            return Err(RuntimeError::Generic(format!("MCP server error: {}", error)));
        }

        // Convert MCP tools to capability manifests
        let mut capabilities = Vec::new();
        for tool in tools_response.tools {
            let capability = self.convert_tool_to_capability(tool);
            capabilities.push(capability);
        }

        Ok(capabilities)
    }

    /// Discover resources from the MCP server
    pub async fn discover_resources(&self) -> RuntimeResult<Vec<CapabilityManifest>> {
        let resources_url = format!("{}/resources", self.config.endpoint);
        
        let mut request_builder = self.client.get(&resources_url);

        // Add authentication if provided
        if let Some(token) = &self.config.auth_token {
            request_builder = request_builder.header("Authorization", format!("Bearer {}", token));
        }

        let request = request_builder.build().map_err(|e| {
            RuntimeError::Generic(format!("Failed to build MCP resources request: {}", e))
        })?;

        // Execute request with timeout
        let response = timeout(
            Duration::from_secs(self.config.timeout_seconds),
            self.client.execute(request),
        )
        .await
        .map_err(|_| RuntimeError::Generic("MCP resources request timeout".to_string()))?
        .map_err(|e| RuntimeError::Generic(format!("MCP resources request failed: {}", e)))?;

        // Check status code
        if !response.status().is_success() {
            return Err(RuntimeError::Generic(format!(
                "MCP resources HTTP error: {} - {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }

        // Parse response
        let response_text = response.text().await.map_err(|e| {
            RuntimeError::Generic(format!("Failed to read MCP resources response: {}", e))
        })?;

        let resources_response: MCPResourcesResponse = serde_json::from_str(&response_text).map_err(|e| {
            RuntimeError::Generic(format!("Failed to parse MCP resources response: {}", e))
        })?;

        if let Some(error) = resources_response.error {
            return Err(RuntimeError::Generic(format!("MCP server error: {}", error)));
        }

        // Convert MCP resources to capability manifests
        let mut capabilities = Vec::new();
        for resource in resources_response.resources {
            if let Ok(capability) = self.convert_resource_to_capability(resource) {
                capabilities.push(capability);
            }
        }

        Ok(capabilities)
    }

    /// Convert an MCP tool to a capability manifest
    fn convert_tool_to_capability(&self, tool: MCPTool) -> CapabilityManifest {
        let capability_id = format!("mcp.{}.{}", self.config.name, tool.name);
        
        CapabilityManifest {
            id: capability_id.clone(),
            name: tool.name.clone(),
            description: tool.description.unwrap_or_else(|| format!("MCP tool: {}", tool.name)),
            provider: ProviderType::MCP(MCPCapability {
                server_url: self.config.endpoint.clone(),
                tool_name: tool.name.clone(),
                timeout_ms: self.config.timeout_seconds * 1000,
            }),
            version: "1.0.0".to_string(),
            input_schema: tool.inputSchema,
            output_schema: tool.outputSchema,
            attestation: None,
            provenance: Some(crate::runtime::capability_marketplace::types::CapabilityProvenance {
                source: "mcp_discovery".to_string(),
                version: Some("1.0.0".to_string()),
                content_hash: format!("mcp_{}_{}", self.config.name, tool.name),
                custody_chain: vec!["mcp_discovery".to_string()],
                registered_at: chrono::Utc::now(),
            }),
            permissions: vec![],
            metadata: {
                let mut metadata = HashMap::new();
                metadata.insert("mcp_server".to_string(), self.config.name.clone());
                metadata.insert("mcp_protocol_version".to_string(), self.config.protocol_version.clone());
                metadata.insert("capability_type".to_string(), "mcp_tool".to_string());
                metadata
            },
        }
    }

    /// Convert an MCP resource to a capability manifest
    fn convert_resource_to_capability(&self, resource: serde_json::Value) -> RuntimeResult<CapabilityManifest> {
        let resource_name = resource
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RuntimeError::Generic("MCP resource missing name".to_string()))?;
        
        let capability_id = format!("mcp.{}.resource.{}", self.config.name, resource_name);
        
        Ok(CapabilityManifest {
            id: capability_id.clone(),
            name: resource_name.to_string(),
            description: resource
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or(&format!("MCP resource: {}", resource_name))
                .to_string(),
            provider: ProviderType::MCP(MCPCapability {
                server_url: self.config.endpoint.clone(),
                tool_name: format!("resource:{}", resource_name),
                timeout_ms: self.config.timeout_seconds * 1000,
            }),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: Some(resource),
            attestation: None,
            provenance: Some(crate::runtime::capability_marketplace::types::CapabilityProvenance {
                source: "mcp_discovery".to_string(),
                version: Some("1.0.0".to_string()),
                content_hash: format!("mcp_resource_{}_{}", self.config.name, resource_name),
                custody_chain: vec!["mcp_discovery".to_string()],
                registered_at: chrono::Utc::now(),
            }),
            permissions: vec![],
            metadata: {
                let mut metadata = HashMap::new();
                metadata.insert("mcp_server".to_string(), self.config.name.clone());
                metadata.insert("mcp_protocol_version".to_string(), self.config.protocol_version.clone());
                metadata.insert("capability_type".to_string(), "mcp_resource".to_string());
                metadata
            },
        })
    }

    /// Health check for the MCP server
    pub async fn health_check(&self) -> RuntimeResult<bool> {
        let health_url = format!("{}/health", self.config.endpoint);
        
        let request = self.client
            .get(&health_url)
            .build()
            .map_err(|e| RuntimeError::Generic(format!("Failed to build MCP health check request: {}", e)))?;

        match timeout(
            Duration::from_secs(5), // Shorter timeout for health checks
            self.client.execute(request),
        )
        .await
        {
            Ok(Ok(response)) => Ok(response.status().is_success()),
            Ok(Err(e)) => Err(RuntimeError::Generic(format!("MCP health check failed: {}", e))),
            Err(_) => Err(RuntimeError::Generic("MCP health check timeout".to_string())),
        }
    }
}

#[async_trait]
impl CapabilityDiscovery for MCPDiscoveryProvider {
    async fn discover(&self) -> RuntimeResult<Vec<CapabilityManifest>> {
        let mut all_capabilities = Vec::new();
        
        // Discover tools
        match self.discover_tools().await {
            Ok(tools) => {
                eprintln!("Discovered {} MCP tools from server: {}", tools.len(), self.config.name);
                all_capabilities.extend(tools);
            }
            Err(e) => {
                eprintln!("MCP tools discovery failed for server {}: {}", self.config.name, e);
            }
        }
        
        // Discover resources
        match self.discover_resources().await {
            Ok(resources) => {
                eprintln!("Discovered {} MCP resources from server: {}", resources.len(), self.config.name);
                all_capabilities.extend(resources);
            }
            Err(e) => {
                eprintln!("MCP resources discovery failed for server {}: {}", self.config.name, e);
            }
        }
        
        Ok(all_capabilities)
    }

    fn name(&self) -> &str {
        "MCPDiscovery"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Builder for MCP discovery configuration
pub struct MCPDiscoveryBuilder {
    config: MCPServerConfig,
}

impl MCPDiscoveryBuilder {
    pub fn new() -> Self {
        Self {
            config: MCPServerConfig::default(),
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

    pub fn build(self) -> RuntimeResult<MCPDiscoveryProvider> {
        MCPDiscoveryProvider::new(self.config)
    }
}

impl Default for MCPDiscoveryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_discovery_config_default() {
        let config = MCPServerConfig::default();
        assert_eq!(config.timeout_seconds, 30);
        assert_eq!(config.protocol_version, "2024-11-05");
        assert!(config.auth_token.is_none());
    }

    #[test]
    fn test_mcp_discovery_builder() {
        let provider = MCPDiscoveryBuilder::new()
            .name("test_server".to_string())
            .endpoint("http://localhost:3000".to_string())
            .timeout_seconds(60)
            .protocol_version("2024-11-05".to_string())
            .build();

        assert!(provider.is_ok());
    }

    #[test]
    fn test_convert_tool_to_capability() {
        let config = MCPServerConfig {
            name: "test_server".to_string(),
            endpoint: "http://localhost:3000".to_string(),
            auth_token: None,
            timeout_seconds: 30,
            protocol_version: "2024-11-05".to_string(),
        };
        
        let provider = MCPDiscoveryProvider::new(config).unwrap();
        
        let tool = MCPTool {
            name: "test_tool".to_string(),
            description: Some("A test MCP tool".to_string()),
            inputSchema: None,
            outputSchema: None,
        };
        
        let capability = provider.convert_tool_to_capability(tool);
        
        assert_eq!(capability.id, "mcp.test_server.test_tool");
        assert_eq!(capability.name, "test_tool");
        assert_eq!(capability.metadata.get("capability_type").unwrap(), "mcp_tool");
    }
}
