use super::types::*;
use crate::runtime::error::RuntimeError;
use async_trait::async_trait;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;

pub struct NetworkDiscoveryAgent {
    pub(crate) registry_endpoint: String,
    pub(crate) auth_token: Option<String>,
    pub(crate) refresh_interval: std::time::Duration,
    pub(crate) last_discovery: std::time::Instant,
}

impl NetworkDiscoveryAgent {
    pub fn new(registry_endpoint: String, auth_token: Option<String>, refresh_interval_secs: u64) -> Self {
        Self {
            registry_endpoint,
            auth_token,
            refresh_interval: std::time::Duration::from_secs(refresh_interval_secs),
            last_discovery: std::time::Instant::now() - std::time::Duration::from_secs(refresh_interval_secs),
        }
    }

    async fn parse_capability_manifest(&self, cap_json: &serde_json::Value) -> Result<CapabilityManifest, RuntimeError> {
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
        let provider = ProviderType::Local(LocalCapability {
            handler: Arc::new(|_| Err(crate::runtime::error::RuntimeError::Generic(
                "Discovered capability not implemented".to_string(),
            ))),
        });
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
            permissions: vec![],
            metadata: HashMap::new(),
        })
    }
}

#[async_trait]
impl CapabilityDiscovery for NetworkDiscoveryAgent {
    async fn discover(&self) -> Result<Vec<CapabilityManifest>, RuntimeError> {
        if self.last_discovery.elapsed() < self.refresh_interval {
            return Ok(vec![]);
        }
        let client = reqwest::Client::new();
        let payload = serde_json::json!({
            "method": "discover_capabilities",
            "params": {"limit": 100, "include_attestations": true, "include_provenance": true}
        });
        let mut request = client
            .post(&self.registry_endpoint)
            .header("Content-Type", "application/json")
            .json(&payload)
            .timeout(std::time::Duration::from_secs(30));
        if let Some(token) = &self.auth_token {
            request = request.bearer_auth(token);
        }
        let response = request
            .send()
            .await
            .map_err(|e| RuntimeError::Generic(format!("Network discovery failed: {}", e)))?;
        if !response.status().is_success() {
            return Err(RuntimeError::Generic(format!("Registry error: {}", response.status())));
        }
        let response_json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse discovery response: {}", e)))?;
        let capabilities = if let Some(result) = response_json.get("result") {
            if let Some(caps) = result.get("capabilities") {
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
            }
        } else {
            vec![]
        };
        Ok(capabilities)
    }
}

pub struct LocalFileDiscoveryAgent {
    pub(crate) discovery_path: std::path::PathBuf,
    pub(crate) file_pattern: String,
}

impl LocalFileDiscoveryAgent {
    pub fn new(discovery_path: std::path::PathBuf, file_pattern: String) -> Self {
        Self { discovery_path, file_pattern }
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
                                    if let Ok(cap_json) = serde_json::from_str::<serde_json::Value>(&content) {
                                        if let Ok(manifest) = parse_capability_manifest_from_json(&cap_json).await {
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
}

async fn parse_capability_manifest_from_json(cap_json: &serde_json::Value) -> Result<CapabilityManifest, RuntimeError> {
    let id = cap_json
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| RuntimeError::Generic("Missing capability id".to_string()))?
        .to_string();
    let name = cap_json.get("name").and_then(|v| v.as_str()).unwrap_or(&id).to_string();
    let description = cap_json.get("description").and_then(|v| v.as_str()).unwrap_or("Discovered capability").to_string();
    let version = cap_json.get("version").and_then(|v| v.as_str()).unwrap_or("1.0.0").to_string();
    let provider = if let Some(endpoint) = cap_json.get("endpoint").and_then(|v| v.as_str()) {
        ProviderType::Http(HttpCapability { base_url: endpoint.to_string(), auth_token: None, timeout_ms: 30000 })
    } else {
        ProviderType::Local(LocalCapability { handler: Arc::new(|_| Err(crate::runtime::error::RuntimeError::Generic("Discovered capability not implemented".to_string()))) })
    };
    let attestation = cap_json.get("attestation").and_then(|att_json| parse_capability_attestation(att_json).ok());
    let provenance = Some(CapabilityProvenance { source: "local_file_discovery".to_string(), version: Some(version.clone()), content_hash: compute_content_hash(&format!("{}{}{}", id, name, description)), custody_chain: vec!["local_file_discovery".to_string()], registered_at: Utc::now() });
    Ok(CapabilityManifest { id, name, description, provider, version, input_schema: None, output_schema: None, attestation, provenance, permissions: vec![], metadata: HashMap::new() })
}

fn parse_capability_attestation(att_json: &serde_json::Value) -> Result<CapabilityAttestation, RuntimeError> {
    let signature = att_json.get("signature").and_then(|v| v.as_str()).ok_or_else(|| RuntimeError::Generic("Missing attestation signature".to_string()))?.to_string();
    let authority = att_json.get("authority").and_then(|v| v.as_str()).ok_or_else(|| RuntimeError::Generic("Missing attestation authority".to_string()))?.to_string();
    let created_at = att_json.get("created_at").and_then(|v| v.as_str()).and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map(|dt| dt.with_timezone(&Utc)).unwrap_or_else(|| Utc::now());
    let expires_at = att_json.get("expires_at").and_then(|v| v.as_str()).and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map(|dt| dt.with_timezone(&Utc));
    let metadata = att_json.get("metadata").and_then(|v| v.as_object()).map(|obj| obj.iter().filter_map(|(k,v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect()).unwrap_or_default();
    Ok(CapabilityAttestation { signature, authority, created_at, expires_at, metadata })
}

pub fn compute_content_hash(content: &str) -> String {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}
