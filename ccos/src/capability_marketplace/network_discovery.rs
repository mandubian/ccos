use async_trait::async_trait;
use crate::capability_marketplace::types::{CapabilityDiscovery, CapabilityManifest, CapabilityProvenance, HttpCapability, LocalCapability, ProviderType};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;

/// Configuration for network-based capability discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkDiscoveryConfig {
    /// Base URL for the capability registry API
    pub base_url: String,
    /// Authentication token (if required)
    pub auth_token: Option<String>,
    /// Request timeout in seconds
    pub timeout_seconds: u64,
    /// Maximum number of retries
    pub max_retries: u32,
    /// Retry delay in milliseconds
    pub retry_delay_ms: u64,
    /// Additional headers to include in requests
    pub headers: HashMap<String, String>,
}

impl Default for NetworkDiscoveryConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.capability-registry.example.com".to_string(),
            auth_token: None,
            timeout_seconds: 30,
            max_retries: 3,
            retry_delay_ms: 1000,
            headers: HashMap::new(),
        }
    }
}

/// Response from a capability registry API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityRegistryResponse {
    pub capabilities: Vec<serde_json::Value>, // Use raw JSON instead of CapabilityManifest
    pub total_count: usize,
    pub next_page_token: Option<String>,
    pub error: Option<String>,
}

/// Network-based capability discovery provider
pub struct NetworkDiscoveryProvider {
    config: NetworkDiscoveryConfig,
    client: reqwest::Client,
}

impl NetworkDiscoveryProvider {
    /// Create a new network discovery provider
    pub fn new(config: NetworkDiscoveryConfig) -> RuntimeResult<Self> {
        let mut client_builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_seconds));

        // Add default headers
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_static("CCOS-Capability-Marketplace/1.0"),
        );
        headers.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static("application/json"),
        );

        // Add custom headers from config
        for (key, value) in &config.headers {
            if let Ok(header_name) = key.parse::<reqwest::header::HeaderName>() {
                if let Ok(header_value) = value.parse::<reqwest::header::HeaderValue>() {
                    headers.insert(header_name, header_value);
                }
            }
        }

        client_builder = client_builder.default_headers(headers);

        let client = client_builder
            .build()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self { config, client })
    }

    /// Discover capabilities from a remote registry
    pub async fn discover_capabilities(&self) -> RuntimeResult<Vec<CapabilityManifest>> {
        let mut all_capabilities = Vec::new();
        let mut next_page_token = None;
        let mut retry_count = 0;

        loop {
            match self.fetch_capabilities_page(next_page_token.as_deref()).await {
                Ok(response) => {
                    if let Some(error) = response.error {
                        return Err(RuntimeError::Generic(format!(
                            "Registry API error: {}",
                            error
                        )));
                    }

                    // Convert raw JSON to CapabilityManifest
                    for cap_json in response.capabilities {
                        if let Ok(manifest) = self.parse_capability_manifest(&cap_json).await {
                            all_capabilities.push(manifest);
                        }
                    }
                    next_page_token = response.next_page_token;

                    // If no more pages, break
                    if next_page_token.is_none() {
                        break;
                    }

                    // Reset retry count on success
                    retry_count = 0;
                }
                Err(e) => {
                    retry_count += 1;
                    if retry_count > self.config.max_retries {
                        return Err(RuntimeError::Generic(format!(
                            "Failed to fetch capabilities after {} retries: {}",
                            self.config.max_retries, e
                        )));
                    }

                    // Wait before retrying
                    tokio::time::sleep(Duration::from_millis(self.config.retry_delay_ms)).await;
                    eprintln!("Retrying capability discovery (attempt {}/{}): {}", 
                             retry_count, self.config.max_retries, e);
                }
            }
        }

        Ok(all_capabilities)
    }

    /// Fetch a single page of capabilities
    async fn fetch_capabilities_page(
        &self,
        page_token: Option<&str>,
    ) -> RuntimeResult<CapabilityRegistryResponse> {
        let mut url = format!("{}/capabilities", self.config.base_url);
        
        // Add query parameters
        let mut query_params = vec![("limit".to_string(), "100".to_string())];
        if let Some(token) = page_token {
            query_params.push(("page_token".to_string(), token.to_string()));
        }

        let mut request_builder = self.client.get(&url);

        // Add authentication if provided
        if let Some(token) = &self.config.auth_token {
            request_builder = request_builder.header("Authorization", format!("Bearer {}", token));
        }

        // Add query parameters
        for (key, value) in query_params {
            request_builder = request_builder.query(&[(key, value)]);
        }

        let request = request_builder.build().map_err(|e| {
            RuntimeError::Generic(format!("Failed to build request: {}", e))
        })?;

        // Execute request with timeout
        let response = timeout(
            Duration::from_secs(self.config.timeout_seconds),
            self.client.execute(request),
        )
        .await
        .map_err(|_| RuntimeError::Generic("Request timeout".to_string()))?
        .map_err(|e| RuntimeError::Generic(format!("Request failed: {}", e)))?;

        // Check status code
        if !response.status().is_success() {
            return Err(RuntimeError::Generic(format!(
                "HTTP error: {} - {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }

        // Parse response
        let response_text = response.text().await.map_err(|e| {
            RuntimeError::Generic(format!("Failed to read response: {}", e))
        })?;

        serde_json::from_str(&response_text).map_err(|e| {
            RuntimeError::Generic(format!("Failed to parse response: {}", e))
        })
    }

    /// Validate a capability manifest from the network
    pub fn validate_capability_manifest(&self, manifest: &CapabilityManifest) -> RuntimeResult<()> {
        // Basic validation
        if manifest.id.is_empty() {
            return Err(RuntimeError::Generic("Capability ID cannot be empty".to_string()));
        }

        if manifest.name.is_empty() {
            return Err(RuntimeError::Generic("Capability name cannot be empty".to_string()));
        }

        // Validate version format
        if !self.is_valid_semver(&manifest.version) {
            return Err(RuntimeError::Generic(
                "Version must be in semantic versioning format (e.g., 1.0.0)".to_string(),
            ));
        }

        Ok(())
    }

    /// Simple semantic version validation
    fn is_valid_semver(&self, version: &str) -> bool {
        let parts: Vec<&str> = version.split('.').collect();
        if parts.len() != 3 {
            return false;
        }

        for part in parts {
            if part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()) {
                return false;
            }
        }

        true
    }

    /// Health check for the network discovery provider
    pub async fn health_check(&self) -> RuntimeResult<bool> {
        let health_url = format!("{}/health", self.config.base_url);
        
        let request = self.client
            .get(&health_url)
            .build()
            .map_err(|e| RuntimeError::Generic(format!("Failed to build health check request: {}", e)))?;

        match timeout(
            Duration::from_secs(5), // Shorter timeout for health checks
            self.client.execute(request),
        )
        .await
        {
            Ok(Ok(response)) => Ok(response.status().is_success()),
            Ok(Err(e)) => Err(RuntimeError::Generic(format!("Health check failed: {}", e))),
            Err(_) => Err(RuntimeError::Generic("Health check timeout".to_string())),
        }
    }

    /// Parse a capability manifest from JSON
    async fn parse_capability_manifest(&self, cap_json: &serde_json::Value) -> RuntimeResult<CapabilityManifest> {
        let id = cap_json
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RuntimeError::Generic("Missing capability ID".to_string()))?
            .to_string();
        
        let name = cap_json.get("name").and_then(|v| v.as_str()).unwrap_or(&id).to_string();
        let description = cap_json
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("No description available")
            .to_string();
        let version = cap_json.get("version").and_then(|v| v.as_str()).unwrap_or("1.0.0").to_string();
        
        // Create a simple local capability as placeholder
        let provider = ProviderType::Local(LocalCapability {
            handler: Arc::new(|_| Err(RuntimeError::Generic(
                "Discovered capability not implemented".to_string(),
            ))),
        });
        
        let permissions = extract_string_list(cap_json.get("permissions"));
        let mut effects = extract_string_list(cap_json.get("effects"));
        if effects.is_empty() {
            effects = extract_string_list(cap_json.get("metadata").and_then(|meta| meta.get("effects")));
        }
        let metadata = extract_metadata_map(cap_json.get("metadata"));

        Ok(CapabilityManifest {
            id: id.clone(),
            name: name.clone(),
            description: description.clone(),
            provider,
            version: version.clone(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: Some(CapabilityProvenance {
                source: "network_discovery".to_string(),
                version: Some(version.clone()),
                content_hash: format!("hash_{}", id),
                custody_chain: vec!["network_discovery".to_string()],
                registered_at: chrono::Utc::now(),
            }),
            permissions,
            effects,
            metadata,
        })
    }
}

#[async_trait]
impl CapabilityDiscovery for NetworkDiscoveryProvider {
    async fn discover(&self) -> Result<Vec<CapabilityManifest>, RuntimeError> {
        self.discover_capabilities().await
    }
    
    fn name(&self) -> &str {
        "NetworkDiscovery"
    }
    
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn extract_string_list(value: Option<&JsonValue>) -> Vec<String> {
    match value {
        Some(JsonValue::Array(items)) => items
            .iter()
            .filter_map(|item| match item {
                JsonValue::String(s) => Some(s.trim().to_string()),
                JsonValue::Number(num) => Some(num.to_string()),
                JsonValue::Bool(b) => Some(b.to_string()),
                _ => None,
            })
            .collect(),
        Some(JsonValue::String(s)) => vec![s.trim().to_string()],
        Some(JsonValue::Bool(b)) => vec![b.to_string()],
        Some(JsonValue::Number(num)) => vec![num.to_string()],
        _ => Vec::new(),
    }
}

fn extract_metadata_map(value: Option<&JsonValue>) -> HashMap<String, String> {
    let mut metadata = HashMap::new();

    if let Some(JsonValue::Object(obj)) = value {
        for (key, entry) in obj {
            if key == "effects" {
                continue;
            }

            let string_value = match entry {
                JsonValue::String(s) => Some(s.trim().to_string()),
                JsonValue::Bool(b) => Some(b.to_string()),
                JsonValue::Number(num) => Some(num.to_string()),
                _ => None,
            };

            if let Some(v) = string_value {
                metadata.insert(key.clone(), v);
            }
        }
    }

    metadata
}

/// Builder for network discovery configuration
pub struct NetworkDiscoveryBuilder {
    config: NetworkDiscoveryConfig,
}

impl NetworkDiscoveryBuilder {
    pub fn new() -> Self {
        Self {
            config: NetworkDiscoveryConfig::default(),
        }
    }

    pub fn base_url(mut self, url: String) -> Self {
        self.config.base_url = url;
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

    pub fn max_retries(mut self, retries: u32) -> Self {
        self.config.max_retries = retries;
        self
    }

    pub fn retry_delay_ms(mut self, delay: u64) -> Self {
        self.config.retry_delay_ms = delay;
        self
    }

    pub fn header(mut self, key: String, value: String) -> Self {
        self.config.headers.insert(key, value);
        self
    }

    pub fn build(self) -> RuntimeResult<NetworkDiscoveryProvider> {
        NetworkDiscoveryProvider::new(self.config)
    }
}

impl Default for NetworkDiscoveryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtfs::runtime::capability_marketplace::types::{
        CapabilityManifest, ProviderType, CapabilityAttestation, CapabilityProvenance,
    };

    #[test]
    fn test_network_discovery_config_default() {
        let config = NetworkDiscoveryConfig::default();
        assert_eq!(config.timeout_seconds, 30);
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.retry_delay_ms, 1000);
        assert!(config.auth_token.is_none());
    }

    #[test]
    fn test_network_discovery_builder() {
        let provider = NetworkDiscoveryBuilder::new()
            .base_url("https://test.example.com".to_string())
            .timeout_seconds(60)
            .max_retries(5)
            .header("X-Custom-Header".to_string(), "test-value".to_string())
            .build();

        assert!(provider.is_ok());
    }

    #[test]
    fn test_validate_capability_manifest() {
        let provider = NetworkDiscoveryProvider::new(NetworkDiscoveryConfig::default()).unwrap();
        
        let valid_manifest = CapabilityManifest {
            id: "test.capability".to_string(),
            name: "Test Capability".to_string(),
            description: "A test capability".to_string(),
            provider: ProviderType::Http(HttpCapability {
                base_url: "https://example.com/api".to_string(),
                auth_token: None,
                timeout_ms: 30000,
            }),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: Some(CapabilityProvenance {
                source: "test".to_string(),
                version: Some("1.0.0".to_string()),
                content_hash: "test_hash".to_string(),
                custody_chain: vec!["test".to_string()],
                registered_at: chrono::Utc::now(),
            }),
            permissions: vec![],
            effects: vec![],
            metadata: HashMap::new(),
        };

        assert!(provider.validate_capability_manifest(&valid_manifest).is_ok());
    }

    #[test]
    fn test_validate_capability_manifest_invalid() {
        let provider = NetworkDiscoveryProvider::new(NetworkDiscoveryConfig::default()).unwrap();
        
        let invalid_manifest = CapabilityManifest {
            id: "".to_string(), // Empty ID
            name: "Test Capability".to_string(),
            description: "A test capability".to_string(),
            provider: ProviderType::Local(LocalCapability {
                handler: Arc::new(|_| Ok(Value::String("test".to_string()))),
            }),
            version: "invalid-version".to_string(), // Invalid version
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: Some(CapabilityProvenance {
                source: "test".to_string(),
                version: Some("invalid-version".to_string()),
                content_hash: "test_hash".to_string(),
                custody_chain: vec!["test".to_string()],
                registered_at: chrono::Utc::now(),
            }),
            permissions: vec![],
            effects: vec![],
            metadata: HashMap::new(),
        };

        assert!(provider.validate_capability_manifest(&invalid_manifest).is_err());
    }

    #[test]
    fn test_semver_validation() {
        let provider = NetworkDiscoveryProvider::new(NetworkDiscoveryConfig::default()).unwrap();
        
        assert!(provider.is_valid_semver("1.0.0"));
        assert!(provider.is_valid_semver("2.1.3"));
        assert!(!provider.is_valid_semver("1.0"));
        assert!(!provider.is_valid_semver("1.0.0.0"));
        assert!(!provider.is_valid_semver("1.0.a"));
        assert!(!provider.is_valid_semver(""));
    }
}
