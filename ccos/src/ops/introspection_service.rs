//! Introspection Service
//!
//! A reusable service for introspecting APIs (OpenAPI, MCP, HTML docs) and generating
//! RTFS capability files. This module is used by MCP server, CLI, and TUI.

use crate::approval::{
    queue::{DiscoverySource, RiskAssessment, RiskLevel, ServerInfo},
    storage_file::FileApprovalStorage,
    UnifiedApprovalQueue,
};
use crate::capability_marketplace::types::CapabilityManifest;
use crate::secrets::SecretStore;
use crate::synthesis::core::schema_serializer::type_expr_to_rtfs_compact;
use crate::synthesis::introspection::api_introspector::{
    APIIntrospectionResult, APIIntrospector, DiscoveredEndpoint,
};
use crate::utils::fs::get_workspace_root;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Result of an introspection operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntrospectionResult {
    /// Whether introspection succeeded
    pub success: bool,
    /// The introspection source type
    pub source: IntrospectionSource,
    /// Server name
    pub server_name: String,
    /// API introspection result (if OpenAPI)
    pub api_result: Option<APIIntrospectionResult>,
    /// Discovered manifests (for MCP)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub manifests: Vec<CapabilityManifest>,
    /// Optional approval ID if queued
    pub approval_id: Option<String>,
    /// Error message if failed
    pub error: Option<String>,
}

/// Source of introspection
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntrospectionSource {
    OpenApi,
    Mcp,
    McpStdio,
    HtmlDocs,
    Unknown,
}

/// Result of generating RTFS files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RtfsGenerationResult {
    /// Directory where files were written
    pub output_dir: PathBuf,
    /// List of generated capability file paths (relative to output_dir)
    pub capability_files: Vec<String>,
    /// Path to server.json
    pub server_json_path: PathBuf,
}

/// Introspection Service for discovering and generating capabilities
pub struct IntrospectionService {
    introspector: APIIntrospector,
    mcp_discovery: Option<Arc<crate::mcp::core::MCPDiscoveryService>>,
    llm_discovery: Option<Arc<crate::discovery::llm_discovery::LlmDiscoveryService>>,
    approval_queue: Option<UnifiedApprovalQueue<FileApprovalStorage>>,
}

impl Default for IntrospectionService {
    fn default() -> Self {
        Self {
            introspector: APIIntrospector::new(),
            mcp_discovery: None,
            llm_discovery: None,
            approval_queue: None,
        }
    }
}

impl IntrospectionService {
    /// Create a new introspection service
    pub fn new(
        mcp_discovery: Arc<crate::mcp::core::MCPDiscoveryService>,
        llm_discovery: Arc<crate::discovery::llm_discovery::LlmDiscoveryService>,
        approval_queue: UnifiedApprovalQueue<FileApprovalStorage>,
    ) -> Self {
        Self {
            introspector: APIIntrospector::new(),
            mcp_discovery: Some(mcp_discovery),
            llm_discovery: Some(llm_discovery),
            approval_queue: Some(approval_queue),
        }
    }

    /// Create a new empty introspection service
    pub fn empty() -> Self {
        Self::default()
    }

    /// Introspect an OpenAPI spec URL
    pub async fn introspect_openapi(
        &self,
        spec_url: &str,
        server_name: &str,
    ) -> RuntimeResult<IntrospectionResult> {
        match self
            .introspector
            .introspect_from_openapi(spec_url, server_name)
            .await
        {
            Ok(api_result) => Ok(IntrospectionResult {
                success: true,
                source: IntrospectionSource::OpenApi,
                server_name: server_name.to_string(),
                api_result: Some(api_result),
                manifests: Vec::new(),
                approval_id: None,
                error: None,
            }),
            Err(e) => Ok(IntrospectionResult {
                success: false,
                source: IntrospectionSource::OpenApi,
                server_name: server_name.to_string(),
                api_result: None,
                manifests: Vec::new(),
                approval_id: None,
                error: Some(e.to_string()),
            }),
        }
    }

    /// Introspect an MCP server
    pub async fn introspect_mcp(
        &self,
        endpoint: &str,
        name: Option<String>,
        auth_token: Option<String>,
        discovery_service: &crate::mcp::core::MCPDiscoveryService,
        output_dir: &Path,
    ) -> RuntimeResult<IntrospectionResult> {
        let server_name = name.clone().unwrap_or_else(|| {
            if let Ok(url) = url::Url::parse(endpoint) {
                url.host_str()
                    .map(|h| h.replace(".", "_"))
                    .unwrap_or_else(|| endpoint.split('/').last().unwrap_or("unknown").to_string())
            } else {
                endpoint.split('/').last().unwrap_or("unknown").to_string()
            }
        });

        let server_config = crate::capability_marketplace::mcp_discovery::MCPServerConfig {
            name: server_name.clone(),
            endpoint: endpoint.to_string(),
            auth_token,
            timeout_seconds: 30,
            protocol_version: "2024-11-05".to_string(),
        };

        let options = crate::mcp::types::DiscoveryOptions {
            export_to_rtfs: true,
            export_directory: Some(output_dir.to_string_lossy().to_string()),
            register_in_marketplace: false,
            create_approval_request: true,
            ..Default::default()
        };

        match discovery_service
            .discover_and_export_tools(&server_config, &options)
            .await
        {
            Ok((manifests, approval_id)) => Ok(IntrospectionResult {
                success: true,
                source: IntrospectionSource::Mcp,
                server_name,
                api_result: None,
                manifests,
                approval_id,
                error: None,
            }),
            Err(e) => Ok(IntrospectionResult {
                success: false,
                source: IntrospectionSource::Mcp,
                server_name,
                api_result: None,
                manifests: Vec::new(),
                approval_id: None,
                error: Some(e.to_string()),
            }),
        }
    }

    /// Generate RTFS capability files from an introspection result
    pub fn generate_rtfs_files(
        &self,
        result: &IntrospectionResult,
        output_dir: &Path,
        spec_url: &str,
    ) -> RuntimeResult<RtfsGenerationResult> {
        let api_result = result
            .api_result
            .as_ref()
            .ok_or_else(|| RuntimeError::Generic("No API result to generate files from".into()))?;

        // Create output directory
        std::fs::create_dir_all(output_dir).map_err(|e| {
            RuntimeError::Generic(format!("Failed to create output directory: {}", e))
        })?;

        let server_id = sanitize_filename(&result.server_name);
        let module_name = api_result
            .api_title
            .to_lowercase()
            .replace(" ", "_")
            .replace("-", "_")
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_')
            .collect::<String>();

        let mut capability_files = Vec::new();

        // Group endpoints by tag (first path segment)
        let mut endpoints_by_tag: std::collections::HashMap<String, Vec<&DiscoveredEndpoint>> =
            std::collections::HashMap::new();

        for ep in &api_result.endpoints {
            let tag = ep
                .path
                .trim_start_matches('/')
                .split('/')
                .next()
                .unwrap_or("general")
                .to_string();
            endpoints_by_tag.entry(tag).or_default().push(ep);
        }

        // Generate RTFS files per tag
        for (tag, endpoints) in &endpoints_by_tag {
            let tag_dir = output_dir.join("openapi").join(tag);
            std::fs::create_dir_all(&tag_dir).ok();

            for ep in endpoints {
                let cap_name = ep.endpoint_id.to_lowercase();
                let rtfs_content =
                    self.generate_rtfs_capability(ep, api_result, &module_name, spec_url);

                let cap_file = tag_dir.join(format!("{}.rtfs", cap_name));
                if std::fs::write(&cap_file, &rtfs_content).is_ok() {
                    capability_files.push(format!("openapi/{}/{}.rtfs", tag, cap_name));
                }
            }
        }

        // Create server.rtfs
        let files_rtfs = capability_files
            .iter()
            .map(|f| format!("\"{}\"", f))
            .collect::<Vec<_>>()
            .join(" ");

        let server_rtfs = format!(
            r#";; Server Manifest: {}
(server
  :source {{
    :type "OpenAPI"
    :spec_url "{}"
  }}
  :server_info {{
    :name "{}"
    :endpoint "{}"
    :description "{}"
    :auth_env_var {}
  }}
  :api_info {{
    :title "{}"
    :version "{}"
    :base_url "{}"
    :endpoints_count {}
  }}
  :capability_files [{}]
)
"#,
            result.server_name,
            spec_url,
            result.server_name,
            api_result.base_url,
            format!("{} v{}", api_result.api_title, api_result.api_version),
            if api_result.auth_requirements.auth_type.is_empty() {
                "nil".to_string()
            } else {
                format!("\"{}\"", format!("{}_API_KEY", module_name.to_uppercase()))
            },
            api_result.api_title,
            api_result.api_version,
            api_result.base_url,
            api_result.endpoints.len(),
            files_rtfs
        );

        let server_rtfs_path = output_dir.join("server.rtfs");
        std::fs::write(&server_rtfs_path, &server_rtfs)
            .map_err(|e| RuntimeError::Generic(format!("Failed to write server.rtfs: {}", e)))?;

        Ok(RtfsGenerationResult {
            output_dir: output_dir.to_path_buf(),
            capability_files,
            server_json_path: server_rtfs_path, // Returning path to rtfs in legacy field name for now
        })
    }

    /// Generate RTFS content for a single endpoint
    fn generate_rtfs_capability(
        &self,
        ep: &DiscoveredEndpoint,
        api_result: &APIIntrospectionResult,
        module_name: &str,
        spec_url: &str,
    ) -> String {
        let cap_name = ep.endpoint_id.to_lowercase();
        let cap_id = format!("{}.{}", module_name, cap_name);

        let mut rtfs = String::new();

        // Header comment
        rtfs.push_str(&format!(";; Capability: {}\n", ep.name));
        rtfs.push_str(&format!(";; {} API\n", api_result.api_title));
        rtfs.push_str(&format!(";; Base URL: {}\n", api_result.base_url));
        rtfs.push_str(&format!(";; Endpoint: {} {}\n\n", ep.method, ep.path));

        // Capability definition
        rtfs.push_str(&format!("(capability \"{}\"\n", cap_id));
        rtfs.push_str(&format!("  :name \"{}\"\n", escape_string(&ep.name)));
        rtfs.push_str(&format!("  :version \"{}\"\n", api_result.api_version));
        rtfs.push_str(&format!(
            "  :description \"{}\"\n",
            escape_string(&ep.description)
        ));
        rtfs.push_str(&format!(
            "  :provider \"{}\"\n",
            escape_string(&api_result.api_title)
        ));
        rtfs.push_str("  :permissions [:network.http]\n");

        // Effects based on method
        let effects = match ep.method.as_str() {
            "GET" => "[:network_request]",
            "POST" | "PUT" | "PATCH" => "[:network_request :state_write]",
            "DELETE" => "[:network_request :state_delete]",
            _ => "[:network_request]",
        };
        rtfs.push_str(&format!("  :effects {}\n", effects));

        // Metadata block
        rtfs.push_str("  :metadata {\n");
        rtfs.push_str("    :openapi {\n");
        rtfs.push_str(&format!("      :base_url \"{}\"\n", api_result.base_url));
        rtfs.push_str(&format!("      :endpoint_method \"{}\"\n", ep.method));
        rtfs.push_str(&format!("      :endpoint_path \"{}\"\n", ep.path));

        // Auth info - only include if auth is actually required (not "none")
        let auth = &api_result.auth_requirements;
        let needs_auth =
            ep.requires_auth || (!auth.auth_type.is_empty() && auth.auth_type != "none");
        if needs_auth {
            rtfs.push_str("      :auth {\n");
            rtfs.push_str(&format!(
                "        :type \"{}\"\n",
                if auth.auth_type.is_empty() {
                    "apiKey"
                } else {
                    &auth.auth_type
                }
            ));
            rtfs.push_str(&format!(
                "        :location \"{}\"\n",
                if auth.auth_location.is_empty() {
                    "header"
                } else {
                    &auth.auth_location
                }
            ));
            rtfs.push_str(&format!(
                "        :env_var \"{}_API_KEY\"\n",
                module_name.to_uppercase()
            ));
            rtfs.push_str("      }\n");
        }
        rtfs.push_str("    }\n");

        rtfs.push_str("    :discovery {\n");
        rtfs.push_str("      :method \"openapi_introspection\"\n");
        rtfs.push_str(&format!("      :source_url \"{}\"\n", spec_url));
        rtfs.push_str("    }\n");
        rtfs.push_str("  }\n");

        // Input schema - use actual schema or fallback to :any
        let input_schema_str = match &ep.input_schema {
            Some(schema) => type_expr_to_rtfs_compact(schema),
            None => ":any".to_string(),
        };
        rtfs.push_str(&format!("  :input-schema {}\n", input_schema_str));

        // Output schema - use actual schema or fallback to :any
        let output_schema_str = match &ep.output_schema {
            Some(schema) => type_expr_to_rtfs_compact(schema),
            None => ":any".to_string(),
        };
        rtfs.push_str(&format!("  :output-schema {}\n", output_schema_str));

        rtfs.push_str(")\n");

        rtfs
    }

    /// Create an approval request for the introspection result
    pub async fn create_approval_request(
        &self,
        result: &IntrospectionResult,
        spec_url: &str,
        approval_queue: &UnifiedApprovalQueue<FileApprovalStorage>,
        capability_files: Option<Vec<String>>,
        expiry_hours: i64,
    ) -> RuntimeResult<String> {
        let api_result = result
            .api_result
            .as_ref()
            .ok_or_else(|| RuntimeError::Generic("No API result to create approval for".into()))?;

        let server_info = ServerInfo {
            name: result.server_name.clone(),
            endpoint: api_result.base_url.clone(),
            description: Some(format!(
                "{} - {} endpoints discovered from OpenAPI spec",
                api_result.api_title,
                api_result.endpoints.len()
            )),
            auth_env_var: if api_result.auth_requirements.auth_type.is_empty()
                || api_result.auth_requirements.auth_type == "none"
            {
                None
            } else {
                let module_name = api_result
                    .api_title
                    .to_lowercase()
                    .replace(" ", "_")
                    .replace("-", "_");
                Some(format!("{}_API_KEY", module_name.to_uppercase()))
            },
            capabilities_path: None,
            alternative_endpoints: vec![],
            capability_files,
        };

        let approval_id = approval_queue
            .add_server_discovery(
                DiscoverySource::WebSearch {
                    url: spec_url.to_string(),
                },
                server_info.clone(),
                vec!["openapi".to_string()],
                RiskAssessment {
                    level: RiskLevel::Low,
                    reasons: vec!["OpenAPI spec provides structured API definition".to_string()],
                },
                Some(format!("Introspected from OpenAPI spec: {}", spec_url)),
                expiry_hours,
            )
            .await?;

        // If auth is required, check if secret exists and queue if missing
        if let Some(auth_var) = &server_info.auth_env_var {
            let store = SecretStore::new(Some(get_workspace_root())).unwrap_or_else(|_| {
                SecretStore::new(None).unwrap_or_else(|_| panic!("Failed to create SecretStore"))
            });

            if !store.has(auth_var) {
                let _ = approval_queue
                    .add_secret_approval(
                        format!("{}.introspect", result.server_name),
                        auth_var.clone(),
                        format!(
                            "API Key for {} discovered during introspection",
                            result.server_name
                        ),
                        expiry_hours,
                    )
                    .await;
            }
        }

        Ok(approval_id)
    }

    /// Check if URL looks like an OpenAPI spec
    pub fn is_openapi_url(url: &str) -> bool {
        let trimmed = url.trim();
        // Skip stdio commands
        if trimmed.starts_with("npx ")
            || trimmed.starts_with("node ")
            || trimmed.starts_with("python")
            || trimmed.starts_with("/")
            || trimmed.starts_with("./")
        {
            return false;
        }

        let lower = trimmed.to_lowercase();

        // Check for spec file extensions (including in query params like ?api-docs.json)
        lower.ends_with(".json")
            || lower.ends_with(".yaml")
            || lower.ends_with(".yml")
            || lower.contains("swagger")
            || lower.contains("openapi")
            || lower.contains("api-docs.json")
            || lower.contains("api-docs.yaml")
            || lower.contains("api-docs.yml")
    }

    /// Check if string looks like a stdio command
    pub fn is_stdio_command(s: &str) -> bool {
        let trimmed = s.trim();
        trimmed.starts_with("npx ")
            || trimmed.starts_with("node ")
            || trimmed.starts_with("python")
            || trimmed.starts_with("/")
            || trimmed.starts_with("./")
            || (!trimmed.contains("://") && !trimmed.is_empty())
    }
}

/// Sanitize a string to be used as a filename
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Escape special characters in strings for RTFS
fn escape_string(s: &str) -> String {
    s.replace("\\", "\\\\")
        .replace("\"", "\\\"")
        .replace("\n", " ")
        .replace("\r", "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_openapi_url() {
        assert!(IntrospectionService::is_openapi_url(
            "https://example.com/api.yaml"
        ));
        assert!(IntrospectionService::is_openapi_url(
            "https://example.com/swagger.json"
        ));
        assert!(IntrospectionService::is_openapi_url(
            "https://example.com/openapi/v1"
        ));
        assert!(!IntrospectionService::is_openapi_url(
            "https://example.com/docs"
        ));
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("Hello World"), "Hello_World");
        assert_eq!(sanitize_filename("api/v1"), "api_v1");
        assert_eq!(sanitize_filename("test-name_123"), "test-name_123");
    }
}
