use super::types::*;
use crate::mcp::discovery_session::{MCPServerInfo, MCPSessionManager};
use async_trait::async_trait;
use chrono::Utc;
use rtfs::runtime::error::RuntimeError;
use rtfs::runtime::values::Value;
use serde_json::Value as JsonValue;
use std::any::Any;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::fs;

/// Enhanced discovery providers for different capability sources
pub enum DiscoveryProvider {
    /// Static built-in capabilities
    Static(StaticDiscoveryProvider),
    /// File-based manifest discovery
    FileManifest(FileManifestDiscoveryProvider),
    /// Network-based discovery (placeholder for future implementation)
    Network(NetworkDiscoveryProvider),
}

/// Static discovery provider for built-in capabilities
pub struct StaticDiscoveryProvider {
    capabilities: Vec<CapabilityManifest>,
}

impl StaticDiscoveryProvider {
    pub fn new() -> Self {
        Self {
            capabilities: vec![
                // Add some static capabilities here
                CapabilityManifest {
                    id: "static.hello".to_string(),
                    name: "Static Hello".to_string(),
                    description: "A static hello capability".to_string(),
                    provider: ProviderType::Local(LocalCapability {
                        handler: Arc::new(|_| {
                            Ok(Value::String("Hello from static discovery!".to_string()))
                        }),
                    }),
                    version: "1.0.0".to_string(),
                    input_schema: None,
                    output_schema: None,
                    attestation: None,
                    provenance: Some(CapabilityProvenance {
                        source: "static_discovery".to_string(),
                        version: Some("1.0.0".to_string()),
                        content_hash: "static_hello_hash".to_string(),
                        custody_chain: vec!["static_discovery".to_string()],
                        registered_at: chrono::Utc::now(),
                    }),
                    permissions: vec![],
                    effects: vec![],
                    metadata: HashMap::new(),
                    agent_metadata: None,
                    domains: Vec::new(),
                    categories: Vec::new(),
                },
            ],
        }
    }
}

#[async_trait::async_trait]
impl CapabilityDiscovery for StaticDiscoveryProvider {
    async fn discover(&self) -> Result<Vec<CapabilityManifest>, RuntimeError> {
        Ok(self.capabilities.clone())
    }

    fn name(&self) -> &str {
        "StaticDiscovery"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// File-based manifest discovery provider
pub struct FileManifestDiscoveryProvider {
    manifest_path: String,
}

impl FileManifestDiscoveryProvider {
    pub fn new(manifest_path: String) -> Self {
        Self { manifest_path }
    }
}

#[async_trait::async_trait]
impl CapabilityDiscovery for FileManifestDiscoveryProvider {
    async fn discover(&self) -> Result<Vec<CapabilityManifest>, RuntimeError> {
        if !Path::new(&self.manifest_path).exists() {
            return Ok(vec![]); // Return empty if file doesn't exist
        }

        let content = fs::read_to_string(&self.manifest_path)
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to read manifest file: {}", e)))?;

        // For now, return empty since we don't have serde support
        // In a real implementation, you'd parse JSON manifests here
        eprintln!(
            "File manifest discovery not yet implemented for: {}",
            self.manifest_path
        );
        Ok(vec![])
    }

    fn name(&self) -> &str {
        "FileManifestDiscovery"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Network discovery provider (placeholder for future implementation)
pub struct NetworkDiscoveryProvider {
    endpoint_url: String,
    timeout_seconds: u64,
}

impl NetworkDiscoveryProvider {
    pub fn new(endpoint_url: String, timeout_seconds: u64) -> Self {
        Self {
            endpoint_url,
            timeout_seconds,
        }
    }
}

#[async_trait::async_trait]
impl CapabilityDiscovery for NetworkDiscoveryProvider {
    async fn discover(&self) -> Result<Vec<CapabilityManifest>, RuntimeError> {
        // Placeholder implementation - in real implementation, this would:
        // 1. Make HTTP request to endpoint_url
        // 2. Parse response for capability manifests
        // 3. Validate and return discovered capabilities

        // For now, return empty vector to avoid network dependencies
        eprintln!(
            "Network discovery not yet implemented for endpoint: {}",
            self.endpoint_url
        );
        Ok(vec![])
    }

    fn name(&self) -> &str {
        "NetworkDiscovery"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub struct NetworkDiscoveryAgent {
    pub(crate) registry_endpoint: String,
    pub(crate) auth_token: Option<String>,
    pub(crate) refresh_interval: std::time::Duration,
    pub(crate) last_discovery: std::time::Instant,
    session_manager: Arc<MCPSessionManager>,
}

impl NetworkDiscoveryAgent {
    pub fn new(
        registry_endpoint: String,
        auth_token: Option<String>,
        refresh_interval_secs: u64,
    ) -> Self {
        let auth_headers = auth_token.as_ref().map(|token| {
            let mut headers = HashMap::new();
            headers.insert("Authorization".to_string(), format!("Bearer {}", token));
            headers
        });

        Self {
            registry_endpoint,
            auth_token,
            refresh_interval: std::time::Duration::from_secs(refresh_interval_secs),
            last_discovery: std::time::Instant::now()
                - std::time::Duration::from_secs(refresh_interval_secs),
            session_manager: Arc::new(MCPSessionManager::new(auth_headers)),
        }
    }

    async fn parse_capability_manifest(
        &self,
        cap_json: &serde_json::Value,
    ) -> Result<CapabilityManifest, RuntimeError> {
        let id = cap_json
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RuntimeError::Generic("Missing capability ID".to_string()))?
            .to_string();
        let name = cap_json
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(&id)
            .to_string();
        let description = cap_json
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("No description available")
            .to_string();
        let version = cap_json
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("1.0.0")
            .to_string();
        let provider = ProviderType::Local(LocalCapability {
            handler: Arc::new(|_| {
                Err(rtfs::runtime::error::RuntimeError::Generic(
                    "Discovered capability not implemented".to_string(),
                ))
            }),
        });
        let permissions = extract_string_list(cap_json.get("permissions"));
        let mut effects = extract_string_list(cap_json.get("effects"));
        if effects.is_empty() {
            effects = extract_string_list(
                cap_json
                    .get("metadata")
                    .and_then(|meta| meta.get("effects")),
            );
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
                content_hash: compute_content_hash(&format!("{}{}{}", id, name, description)),
                custody_chain: vec!["network_discovery".to_string()],
                registered_at: Utc::now(),
            }),
            permissions,
            effects,
            metadata,
            agent_metadata: None,
            domains: Vec::new(),
            categories: Vec::new(),
        })
    }
}

#[async_trait]
impl CapabilityDiscovery for NetworkDiscoveryAgent {
    async fn discover(&self) -> Result<Vec<CapabilityManifest>, RuntimeError> {
        if self.last_discovery.elapsed() < self.refresh_interval {
            return Ok(vec![]);
        }

        let client_info = MCPServerInfo {
            name: "ccos-network-discovery".to_string(),
            version: "1.0.0".to_string(),
        };

        let session = self
            .session_manager
            .initialize_session(&self.registry_endpoint, &client_info)
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to init session: {}", e)))?;

        let response_result = self
            .session_manager
            .make_request(
                &session,
                "discover_capabilities",
                serde_json::json!({
                    "limit": 100,
                    "include_attestations": true,
                    "include_provenance": true
                }),
            )
            .await
            .map_err(|e| RuntimeError::Generic(format!("Network discovery failed: {}", e)));

        // Ensure we terminate the session even if request fails
        let _ = self.session_manager.terminate_session(&session).await;

        let response = response_result?;

        let capabilities = if let Some(caps) = response.get("capabilities") {
            if let serde_json::Value::Array(caps_array) = caps {
                let mut manifests = Vec::new();
                for cap_json in caps_array {
                    if let Ok(m) = self.parse_capability_manifest(cap_json).await {
                        manifests.push(m);
                    }
                }
                manifests
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        Ok(capabilities)
    }

    fn name(&self) -> &str {
        "NetworkDiscoveryAgent"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub struct LocalFileDiscoveryAgent {
    pub(crate) discovery_path: std::path::PathBuf,
    pub(crate) file_pattern: String,
}

impl LocalFileDiscoveryAgent {
    pub fn new(discovery_path: std::path::PathBuf, file_pattern: String) -> Self {
        Self {
            discovery_path,
            file_pattern,
        }
    }
}

#[async_trait]
impl CapabilityDiscovery for LocalFileDiscoveryAgent {
    async fn discover(&self) -> Result<Vec<CapabilityManifest>, RuntimeError> {
        let mut manifests = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.discovery_path) {
            for entry in entries {
                if let Ok(entry) = entry {
                    let path = entry.path();
                    if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
                        if let Some(filename) = path.file_name().and_then(|s| s.to_str()) {
                            if filename.contains(&self.file_pattern) {
                                if let Ok(content) = std::fs::read_to_string(&path) {
                                    if let Ok(cap_json) =
                                        serde_json::from_str::<serde_json::Value>(&content)
                                    {
                                        if let Ok(manifest) =
                                            parse_capability_manifest_from_json(&cap_json).await
                                        {
                                            manifests.push(manifest);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(manifests)
    }

    fn name(&self) -> &str {
        "LocalFileDiscoveryAgent"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

async fn parse_capability_manifest_from_json(
    cap_json: &serde_json::Value,
) -> Result<CapabilityManifest, RuntimeError> {
    let id = cap_json
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| RuntimeError::Generic("Missing capability id".to_string()))?
        .to_string();
    let name = cap_json
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(&id)
        .to_string();
    let description = cap_json
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("Discovered capability")
        .to_string();
    let version = cap_json
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("1.0.0")
        .to_string();
    let provider = if let Some(endpoint) = cap_json.get("endpoint").and_then(|v| v.as_str()) {
        ProviderType::Http(HttpCapability {
            base_url: endpoint.to_string(),
            auth_token: None,
            timeout_ms: 30000,
        })
    } else {
        ProviderType::Local(LocalCapability {
            handler: Arc::new(|_| {
                Err(rtfs::runtime::error::RuntimeError::Generic(
                    "Discovered capability not implemented".to_string(),
                ))
            }),
        })
    };
    let attestation = cap_json
        .get("attestation")
        .and_then(|att_json| parse_capability_attestation(att_json).ok());
    let provenance = Some(CapabilityProvenance {
        source: "local_file_discovery".to_string(),
        version: Some(version.clone()),
        content_hash: compute_content_hash(&format!("{}{}{}", id, name, description)),
        custody_chain: vec!["local_file_discovery".to_string()],
        registered_at: Utc::now(),
    });
    let permissions = extract_string_list(cap_json.get("permissions"));
    let mut effects = extract_string_list(cap_json.get("effects"));
    if effects.is_empty() {
        effects = extract_string_list(
            cap_json
                .get("metadata")
                .and_then(|meta| meta.get("effects")),
        );
    }
    let metadata = extract_metadata_map(cap_json.get("metadata"));

    Ok(CapabilityManifest {
        id,
        name,
        description,
        provider,
        version,
        input_schema: None,
        output_schema: None,
        attestation,
        provenance,
        permissions,
        effects,
        metadata,
        agent_metadata: None,
        domains: Vec::new(),
        categories: Vec::new(),
    })
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

fn parse_capability_attestation(
    att_json: &serde_json::Value,
) -> Result<CapabilityAttestation, RuntimeError> {
    let signature = att_json
        .get("signature")
        .and_then(|v| v.as_str())
        .ok_or_else(|| RuntimeError::Generic("Missing attestation signature".to_string()))?
        .to_string();
    let authority = att_json
        .get("authority")
        .and_then(|v| v.as_str())
        .ok_or_else(|| RuntimeError::Generic("Missing attestation authority".to_string()))?
        .to_string();
    let created_at = att_json
        .get("created_at")
        .and_then(|v| v.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|| Utc::now());
    let expires_at = att_json
        .get("expires_at")
        .and_then(|v| v.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc));
    let metadata = att_json
        .get("metadata")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();
    Ok(CapabilityAttestation {
        signature,
        authority,
        created_at,
        expires_at,
        metadata,
    })
}

pub fn compute_content_hash(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}
