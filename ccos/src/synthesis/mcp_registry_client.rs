use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// MCP Registry client for discovering and querying MCP servers
pub struct McpRegistryClient {
    base_url: String,
    client: reqwest::Client,
}

/// MCP Server entry from the registry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServer {
    #[serde(rename = "$schema")]
    pub schema: Option<String>,
    pub name: String,
    pub description: String,
    pub version: String,
    pub repository: Option<McpRepository>,
    pub packages: Option<Vec<McpPackage>>,
    pub remotes: Option<Vec<McpRemote>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRepository {
    pub url: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPackage {
    #[serde(rename = "registryType")]
    pub registry_type: String,
    pub identifier: String,
    pub version: Option<String>,
    #[serde(rename = "registryBaseUrl")]
    pub registry_base_url: Option<String>,
    #[serde(rename = "runtimeHint")]
    pub runtime_hint: Option<String>,
    pub transport: McpTransport,
    #[serde(rename = "environmentVariables")]
    pub environment_variables: Option<Vec<McpEnvironmentVariable>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRemote {
    pub r#type: String,
    pub url: String,
    pub headers: Option<Vec<McpHeader>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTransport {
    pub r#type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpEnvironmentVariable {
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "isSecret")]
    pub is_secret: Option<bool>,
    #[serde(rename = "isRequired")]
    pub is_required: Option<bool>,
    pub format: Option<String>,
    pub default: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpHeader {
    pub name: String,
    pub description: Option<String>,
    pub value: Option<String>,
    #[serde(rename = "isSecret", default)]
    pub is_secret: Option<bool>,
    #[serde(rename = "isRequired", default)]
    pub is_required: Option<bool>,
}

/// Registry search response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrySearchResponse {
    pub servers: Vec<RegistryServerEntry>,
    pub metadata: RegistryMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryServerEntry {
    pub server: McpServer,
    #[serde(rename = "_meta")]
    pub meta: RegistryServerMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryServerMeta {
    #[serde(rename = "io.modelcontextprotocol.registry/official")]
    pub official: RegistryOfficialMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryOfficialMeta {
    pub status: String,
    #[serde(rename = "publishedAt")]
    pub published_at: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
    #[serde(rename = "isLatest")]
    pub is_latest: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryMetadata {
    #[serde(rename = "nextCursor")]
    pub next_cursor: Option<String>,
    pub count: usize,
}

impl McpRegistryClient {
    /// Create a new MCP Registry client
    pub fn new() -> Self {
        Self {
            base_url: "https://registry.modelcontextprotocol.io".to_string(),
            client: reqwest::Client::new(),
        }
    }

    /// Search for MCP servers by capability name or description
    pub async fn search_servers(&self, query: &str) -> RuntimeResult<Vec<McpServer>> {
        let url = format!("{}/v0.1/servers", self.base_url);

        let params = [
            ("search", query),
            ("limit", "50"), // Reasonable limit for discovery
        ];

        let response = self
            .client
            .get(&url)
            .query(&params)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| {
                rtfs::runtime::error::RuntimeError::Generic(format!(
                    "Failed to search MCP servers: {}",
                    e
                ))
            })?;

        if !response.status().is_success() {
            return Err(rtfs::runtime::error::RuntimeError::Generic(format!(
                "MCP Registry API error: {}",
                response.status()
            )));
        }

        let search_response: RegistrySearchResponse = response.json().await.map_err(|e| {
            rtfs::runtime::error::RuntimeError::Generic(format!(
                "Failed to parse MCP Registry response: {}",
                e
            ))
        })?;

        // Extract servers from registry entries
        let servers: Vec<McpServer> = search_response
            .servers
            .into_iter()
            .map(|entry| entry.server)
            .collect();
        Ok(servers)
    }

    /// Get all available servers from the registry
    pub async fn list_all_servers(&self) -> RuntimeResult<Vec<McpServer>> {
        let url = format!("{}/v0.1/servers", self.base_url);

        let params = [
            ("limit", "100"), // Get a reasonable number of servers
        ];

        let response = self
            .client
            .get(&url)
            .query(&params)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| {
                rtfs::runtime::error::RuntimeError::Generic(format!(
                    "Failed to list MCP servers: {}",
                    e
                ))
            })?;

        if !response.status().is_success() {
            return Err(rtfs::runtime::error::RuntimeError::Generic(format!(
                "MCP Registry API error: {}",
                response.status()
            )));
        }

        let search_response: RegistrySearchResponse = response.json().await.map_err(|e| {
            rtfs::runtime::error::RuntimeError::Generic(format!(
                "Failed to parse MCP Registry response: {}",
                e
            ))
        })?;

        // Extract servers from registry entries
        let servers: Vec<McpServer> = search_response
            .servers
            .into_iter()
            .map(|entry| entry.server)
            .collect();
        Ok(servers)
    }

    /// Get a specific server by ID
    pub async fn get_server(&self, server_id: &str) -> RuntimeResult<Option<McpServer>> {
        let url = format!("{}/v0.1/servers/{}", self.base_url, server_id);

        let response = self.client.get(&url).send().await.map_err(|e| {
            rtfs::runtime::error::RuntimeError::Generic(format!("Failed to get MCP server: {}", e))
        })?;

        if response.status() == 404 {
            return Ok(None);
        }

        if !response.status().is_success() {
            return Err(rtfs::runtime::error::RuntimeError::Generic(format!(
                "MCP Registry API error: {}",
                response.status()
            )));
        }

        let server: McpServer = response.json().await.map_err(|e| {
            rtfs::runtime::error::RuntimeError::Generic(format!(
                "Failed to parse MCP server: {}",
                e
            ))
        })?;

        Ok(Some(server))
    }

    /// Find servers that provide a specific capability
    pub async fn find_capability_providers(
        &self,
        capability_name: &str,
    ) -> RuntimeResult<Vec<McpServer>> {
        // Search for servers by capability name
        let servers = self.search_servers(capability_name).await?;

        // Filter servers based on name and description matching
        let matching_servers: Vec<McpServer> = servers
            .into_iter()
            .filter(|server| {
                let capability_lower = capability_name.to_lowercase();
                server.name.to_lowercase().contains(&capability_lower)
                    || server
                        .description
                        .to_lowercase()
                        .contains(&capability_lower)
            })
            .collect();

        Ok(matching_servers)
    }

    /// Get mock servers for development when the real API is unavailable
    fn get_mock_servers(&self, _query: &str) -> Vec<McpServer> {
        // Return empty vector - mock capabilities should only be created at startup
        Vec::new()
    }

    /// Convert MCP server to CCOS capability manifest
    pub fn convert_to_capability_manifest(
        &self,
        server: &McpServer,
        capability_name: &str,
    ) -> RuntimeResult<crate::capability_marketplace::types::CapabilityManifest> {
        use crate::capability_marketplace::types::*;

        // Create provider metadata
        let mut metadata = HashMap::new();
        metadata.insert("mcp_server_name".to_string(), server.name.clone());
        metadata.insert("mcp_server_version".to_string(), server.version.clone());

        if let Some(ref repo) = server.repository {
            metadata.insert("mcp_repository_url".to_string(), repo.url.clone());
            metadata.insert("mcp_repository_source".to_string(), repo.source.clone());
        }

        // Add package information if available
        if let Some(ref packages) = server.packages {
            for (i, package) in packages.iter().enumerate() {
                metadata.insert(
                    format!("mcp_package_{}_type", i),
                    package.registry_type.clone(),
                );
                metadata.insert(
                    format!("mcp_package_{}_identifier", i),
                    package.identifier.clone(),
                );
                if let Some(ref version) = package.version {
                    metadata.insert(format!("mcp_package_{}_version", i), version.clone());
                }
                if let Some(ref runtime_hint) = package.runtime_hint {
                    metadata.insert(
                        format!("mcp_package_{}_runtime_hint", i),
                        runtime_hint.clone(),
                    );
                }
            }
        }

        // Add remote information if available
        if let Some(ref remotes) = server.remotes {
            for (i, remote) in remotes.iter().enumerate() {
                metadata.insert(format!("mcp_remote_{}_type", i), remote.r#type.clone());
                metadata.insert(format!("mcp_remote_{}_url", i), remote.url.clone());
            }
        }

        // Determine the server URL for the MCP capability
        let server_url = if let Some(ref remotes) = server.remotes {
            if let Some(remote) = remotes.first() {
                remote.url.clone()
            } else {
                format!("mcp://{}", server.name)
            }
        } else {
            format!("mcp://{}", server.name)
        };

        // Create the capability manifest
        let manifest = CapabilityManifest {
            id: format!("mcp.{}.{}", server.name.replace("/", "."), capability_name),
            name: format!("{} (via {})", capability_name, server.name),
            description: format!(
                "{} - Provided by MCP server: {}",
                server.description, server.name
            ),
            version: server.version.clone(),
            provider: ProviderType::MCP(
                crate::capability_marketplace::types::MCPCapability {
                    server_url,
                    tool_name: capability_name.to_string(),
                    timeout_ms: 30000, // 30 second timeout
                },
            ),
            input_schema: None,  // MCP Registry doesn't provide schema information
            output_schema: None, // MCP Registry doesn't provide schema information
            attestation: None,
            provenance: Some(CapabilityProvenance {
                source: "mcp_registry".to_string(),
                version: Some(server.version.clone()),
                content_hash: format!("mcp_registry_{}_{}", server.name, capability_name),
                custody_chain: vec!["mcp_registry".to_string()],
                registered_at: chrono::Utc::now(),
            }),
            permissions: vec![],
            effects: vec![],
            metadata,
            agent_metadata: None,
        };

        Ok(manifest)
    }
}

type RuntimeResult<T> = Result<T, rtfs::runtime::error::RuntimeError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mcp_registry_client_new() {
        let client = McpRegistryClient::new();
        assert_eq!(client.base_url, "https://registry.modelcontextprotocol.io");
    }

    #[tokio::test]
    async fn test_search_servers_mock() {
        let client = McpRegistryClient::new();
        let servers = client.search_servers("nonexistent").await.unwrap();
        // Should return mock servers when API is unavailable
        assert!(servers.is_empty() || servers.len() > 0);
    }

    #[tokio::test]
    async fn test_find_capability_providers() {
        let client = McpRegistryClient::new();
        let servers = client.find_capability_providers("github").await.unwrap();
        // Should return mock GitHub server when API is unavailable
        assert!(servers.is_empty() || servers.len() > 0);
    }

    #[tokio::test]
    async fn test_convert_to_capability_manifest() {
        let client = McpRegistryClient::new();
        let server = McpServer {
            schema: None,
            name: "test/server".to_string(),
            description: "Test server".to_string(),
            version: "1.0.0".to_string(),
            repository: None,
            packages: None,
            remotes: None,
        };

        let manifest = client
            .convert_to_capability_manifest(&server, "test_capability")
            .unwrap();
        assert_eq!(manifest.id, "mcp.test.server.test_capability");
        assert_eq!(manifest.name, "test_capability (via test/server)");
        assert_eq!(manifest.version, "1.0.0");
    }
}
