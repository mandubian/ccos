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
    APIIntrospectionResult, APIIntrospector, AuthRequirements, DiscoveredEndpoint,
};
use crate::utils::fs::get_workspace_root;
use rtfs::ast::TypeExpr;
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
    /// Browser discovery result (if Browser)
    pub browser_result: Option<crate::ops::browser_discovery::BrowserDiscoveryResult>,
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
    Browser,
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
    browser_discovery: Option<Arc<crate::ops::browser_discovery::BrowserDiscoveryService>>,
    approval_queue: Option<UnifiedApprovalQueue<FileApprovalStorage>>,
}

impl Default for IntrospectionService {
    fn default() -> Self {
        Self {
            introspector: APIIntrospector::new(),
            mcp_discovery: None,
            llm_discovery: None,
            browser_discovery: None,
            approval_queue: None,
        }
    }
}

impl IntrospectionService {
    /// Create a new introspection service
    pub fn new(
        mcp_discovery: Arc<crate::mcp::core::MCPDiscoveryService>,
        llm_discovery: Arc<crate::discovery::llm_discovery::LlmDiscoveryService>,
        browser_discovery: Arc<crate::ops::browser_discovery::BrowserDiscoveryService>,
        approval_queue: UnifiedApprovalQueue<FileApprovalStorage>,
    ) -> Self {
        Self {
            introspector: APIIntrospector::new(),
            mcp_discovery: Some(mcp_discovery),
            llm_discovery: Some(llm_discovery),
            browser_discovery: Some(browser_discovery),
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
                browser_result: None,
                manifests: Vec::new(),
                approval_id: None,
                error: None,
            }),
            Err(e) => Ok(IntrospectionResult {
                success: false,
                source: IntrospectionSource::OpenApi,
                server_name: server_name.to_string(),
                api_result: None,
                browser_result: None,
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
                browser_result: None,
                manifests,
                approval_id,
                error: None,
            }),
            Err(e) => Ok(IntrospectionResult {
                success: false,
                source: IntrospectionSource::Mcp,
                server_name,
                api_result: None,
                browser_result: None,
                manifests: Vec::new(),
                approval_id: None,
                error: Some(e.to_string()),
            }),
        }
    }

    /// Introspect a URL using browser discovery
    pub async fn introspect_browser(
        &self,
        url: &str,
        server_name: &str,
    ) -> RuntimeResult<IntrospectionResult> {
        let browser = self.browser_discovery.as_ref().ok_or_else(|| {
            RuntimeError::Generic("Browser discovery service not configured".into())
        })?;

        eprintln!("🔍 Introspecting using browser: {}", url);

        let extraction_result = if let Some(llm) = &self.llm_discovery {
            browser.extract_with_llm_analysis(url, llm).await
        } else {
            browser.extract_from_url(url).await
        };

        match extraction_result {
            Ok(browser_result) => {
                // If we found an OpenAPI spec, we can prefer that
                if let Some(spec_url) = &browser_result.spec_url {
                    eprintln!("✅ Discovered OpenAPI spec via browser: {}", spec_url);
                    return self.introspect_openapi(spec_url, server_name).await;
                }

                Ok(IntrospectionResult {
                    success: browser_result.success,
                    source: IntrospectionSource::Browser,
                    server_name: server_name.to_string(),
                    api_result: None,
                    browser_result: Some(browser_result),
                    manifests: Vec::new(),
                    approval_id: None,
                    error: None,
                })
            }
            Err(e) => Ok(IntrospectionResult {
                success: false,
                source: IntrospectionSource::Browser,
                server_name: server_name.to_string(),
                api_result: None,
                browser_result: None,
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
        // Create output directory
        std::fs::create_dir_all(output_dir).map_err(|e| {
            RuntimeError::Generic(format!("Failed to create output directory: {}", e))
        })?;

        let module_name = result
            .server_name
            .to_lowercase()
            .replace(" ", "_")
            .replace("-", "_")
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_')
            .collect::<String>();

        let mut capability_files = Vec::new();

        match result.source {
            IntrospectionSource::OpenApi => {
                if let Some(api_result) = &result.api_result {
                    // Group endpoints by tag (first path segment)
                    let mut endpoints_by_tag: std::collections::HashMap<
                        String,
                        Vec<&DiscoveredEndpoint>,
                    > = std::collections::HashMap::new();

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
                            let rtfs_content = self.generate_rtfs_capability_from_openapi(
                                ep,
                                api_result,
                                &module_name,
                                spec_url,
                            );

                            let cap_file = tag_dir.join(format!("{}.rtfs", cap_name));
                            if std::fs::write(&cap_file, &rtfs_content).is_ok() {
                                capability_files.push(format!("openapi/{}/{}.rtfs", tag, cap_name));
                            }
                        }
                    }
                }
            }
            IntrospectionSource::Browser => {
                if let Some(browser_result) = &result.browser_result {
                    let openapi_dir = output_dir.join("openapi");
                    let _ = std::fs::create_dir_all(&openapi_dir);

                    for ep in &browser_result.discovered_endpoints {
                        let cap_name = ep
                            .path
                            .trim_start_matches('/')
                            .replace('/', "_")
                            .to_lowercase();
                        if cap_name.is_empty() {
                            continue;
                        }

                        let rtfs_content = self.generate_rtfs_capability_from_browser(
                            ep,
                            browser_result,
                            &module_name,
                            spec_url,
                        );
                        let cap_file = openapi_dir.join(format!("{}.rtfs", cap_name));
                        if std::fs::write(&cap_file, &rtfs_content).is_ok() {
                            capability_files.push(format!("openapi/{}.rtfs", cap_name));
                        }
                    }
                }
            }
            _ => {
                // For other types, we might not need to generate files here (already handled or not supported)
            }
        }

        // Create server.rtfs
        let files_rtfs = capability_files
            .iter()
            .map(|f| format!("\"{}\"", f))
            .collect::<Vec<_>>()
            .join(" ");

        let server_rtfs = self.generate_server_manifest(result, spec_url, &capability_files);

        let server_rtfs_path = output_dir.join("server.rtfs");
        std::fs::write(&server_rtfs_path, &server_rtfs)
            .map_err(|e| RuntimeError::Generic(format!("Failed to write server.rtfs: {}", e)))?;

        Ok(RtfsGenerationResult {
            output_dir: output_dir.to_path_buf(),
            capability_files,
            server_json_path: server_rtfs_path,
        })
    }

    /// Generate the server.rtfs manifest content
    fn generate_server_manifest(
        &self,
        result: &IntrospectionResult,
        spec_url: &str,
        capability_files: &[String],
    ) -> String {
        let files_rtfs = capability_files
            .iter()
            .map(|f| format!("\"{}\"", f))
            .collect::<Vec<_>>()
            .join(" ");

        let (source_type, endpoints_count, base_url, description, auth_env_var) =
            match result.source {
                IntrospectionSource::OpenApi => {
                    let api = result.api_result.as_ref().unwrap();
                    let module_name = result
                        .server_name
                        .to_lowercase()
                        .replace(" ", "_")
                        .replace("-", "_");
                    (
                        "OpenAPI",
                        api.endpoints.len(),
                        api.base_url.clone(),
                        format!("{} v{}", api.api_title, api.api_version),
                        if api.auth_requirements.auth_type.is_empty()
                            || api.auth_requirements.auth_type == "none"
                        {
                            "nil".into()
                        } else {
                            format!("\"{}\"", format!("{}_API_KEY", module_name.to_uppercase()))
                        },
                    )
                }
                IntrospectionSource::Browser => {
                    let browser = result.browser_result.as_ref().unwrap();
                    let auth_env = if let Some(auth) = &browser.auth {
                        if let Some(env_var) = &auth.env_var {
                            format!("\"{}\"", env_var)
                        } else {
                            "nil".into()
                        }
                    } else {
                        "nil".into()
                    };

                    let base_url = browser
                        .api_base_url
                        .clone()
                        .unwrap_or_else(|| browser.source_url.clone());

                    (
                        "Browser",
                        browser.discovered_endpoints.len(),
                        base_url,
                        format!("Discovered via browser from {}", browser.source_url),
                        auth_env,
                    )
                }
                IntrospectionSource::Mcp | IntrospectionSource::McpStdio => (
                    "MCP",
                    result.manifests.len(),
                    result.server_name.clone(),
                    format!("MCP server: {}", result.server_name),
                    "nil".into(),
                ),
                _ => ("Unknown", 0, "".into(), "".into(), "nil".into()),
            };

        format!(
            r#";; Server Manifest: {}
(server
  :source {{
    :type "{}"
    :spec_url "{}"
  }}
  :server_info {{
    :name "{}"
    :endpoint "{}"
    :description "{}"
    :auth_env_var {}
  }}
  :api_info {{
    :endpoints_count {}
    :base_url "{}"
  }}
  :capability_files [{}]
)
"#,
            result.server_name,
            source_type,
            spec_url,
            result.server_name,
            base_url,
            description,
            auth_env_var,
            endpoints_count,
            base_url,
            files_rtfs
        )
    }

    /// Generate RTFS content for an OpenAPI endpoint
    fn generate_rtfs_capability_from_openapi(
        &self,
        ep: &DiscoveredEndpoint,
        api_result: &APIIntrospectionResult,
        module_name: &str,
        spec_url: &str,
    ) -> String {
        self.generate_rtfs_capability(
            &ep.endpoint_id,
            &ep.name,
            &ep.description,
            &api_result.api_title,
            &api_result.api_version,
            &api_result.base_url,
            &ep.method,
            &ep.path,
            ep.requires_auth,
            &api_result.auth_requirements,
            module_name,
            "openapi_introspection",
            spec_url,
            ep.input_schema.as_ref(),
            ep.output_schema.as_ref(),
        )
    }

    /// Generate RTFS content for a browser-discovered endpoint
    fn generate_rtfs_capability_from_browser(
        &self,
        ep: &crate::ops::browser_discovery::DiscoveredEndpoint,
        browser_result: &crate::ops::browser_discovery::BrowserDiscoveryResult,
        module_name: &str,
        spec_url: &str,
    ) -> String {
        let auth_reqs = if let Some(auth) = &browser_result.auth {
            AuthRequirements {
                auth_type: auth.auth_type.to_string(),
                auth_location: auth
                    .key_location
                    .clone()
                    .unwrap_or_else(|| "header".to_string()),
                auth_param_name: auth
                    .header_name
                    .clone()
                    .unwrap_or_else(|| "Authorization".to_string()),
                required: auth.required,
                env_var_name: auth.env_var.clone(),
            }
        } else {
            AuthRequirements {
                auth_type: "none".into(),
                auth_location: "none".into(),
                auth_param_name: "none".into(),
                required: false,
                env_var_name: None,
            }
        };

        self.generate_rtfs_capability(
            &ep.path.trim_start_matches('/').replace('/', "_"),
            &format!("{} {}", ep.method, ep.path),
            ep.description.as_deref().unwrap_or(""),
            &browser_result
                .page_title
                .clone()
                .unwrap_or_else(|| "Browser Discovered API".into()),
            "1.0.0",
            browser_result
                .api_base_url
                .as_deref()
                .unwrap_or(&browser_result.source_url),
            &ep.method,
            &ep.path,
            false,
            &auth_reqs,
            module_name,
            "browser_discovery",
            spec_url,
            None,
            None,
        )
    }

    /// Generic RTFS capability generation
    fn generate_rtfs_capability(
        &self,
        endpoint_id: &str,
        name: &str,
        description: &str,
        api_title: &str,
        api_version: &str,
        base_url: &str,
        method: &str,
        path: &str,
        requires_auth: bool,
        auth: &AuthRequirements,
        module_name: &str,
        discovery_method: &str,
        source_url: &str,
        input_schema: Option<&TypeExpr>,
        output_schema: Option<&TypeExpr>,
    ) -> String {
        let cap_name = endpoint_id.to_lowercase();
        let cap_id = format!("{}.{}", module_name, cap_name);

        let mut rtfs = String::new();

        // Header comment
        rtfs.push_str(&format!(";; Capability: {}\n", name));
        rtfs.push_str(&format!(";; {} API\n", api_title));
        rtfs.push_str(&format!(";; Base URL: {}\n", base_url));
        rtfs.push_str(&format!(";; Endpoint: {} {}\n\n", method, path));

        // Capability definition
        rtfs.push_str(&format!("(capability \"{}\"\n", cap_id));
        rtfs.push_str(&format!("  :name \"{}\"\n", escape_string(name)));
        rtfs.push_str(&format!("  :version \"{}\"\n", api_version));
        rtfs.push_str(&format!(
            "  :description \"{}\"\n",
            escape_string(description)
        ));
        rtfs.push_str(&format!("  :provider \"{}\"\n", escape_string(api_title)));
        rtfs.push_str("  :permissions [:network.http]\n");

        // Effects based on method
        let effects = match method.to_uppercase().as_str() {
            "GET" => "[:network_request]",
            "POST" | "PUT" | "PATCH" => "[:network_request :state_write]",
            "DELETE" => "[:network_request :state_delete]",
            _ => "[:network_request]",
        };
        rtfs.push_str(&format!("  :effects {}\n", effects));

        // Metadata block
        rtfs.push_str("  :metadata {\n");
        rtfs.push_str("    :endpoint {\n");
        rtfs.push_str(&format!("      :base_url \"{}\"\n", base_url));
        rtfs.push_str(&format!("      :method \"{}\"\n", method));
        rtfs.push_str(&format!("      :path \"{}\"\n", path));

        // Auth info
        let needs_auth = requires_auth || (!auth.auth_type.is_empty() && auth.auth_type != "none");
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
                "        :param_name \"{}\"\n",
                if auth.auth_param_name.is_empty() || auth.auth_param_name == "none" {
                    "Authorization"
                } else {
                    &auth.auth_param_name
                }
            ));

            let env_var = if let Some(e) = &auth.env_var_name {
                e.clone()
            } else {
                format!("{}_API_KEY", module_name.to_uppercase())
            };

            rtfs.push_str(&format!("        :env_var \"{}\"\n", env_var));
            rtfs.push_str("      }\n");
        }
        rtfs.push_str("    }\n");

        rtfs.push_str("    :discovery {\n");
        rtfs.push_str(&format!("      :method \"{}\"\n", discovery_method));
        rtfs.push_str(&format!("      :source_url \"{}\"\n", source_url));
        rtfs.push_str("    }\n");
        rtfs.push_str("  }\n");

        // Input schema
        let input_schema_str = match input_schema {
            Some(schema) => type_expr_to_rtfs_compact(schema),
            None => ":any".to_string(),
        };
        rtfs.push_str(&format!("  :input-schema {}\n", input_schema_str));

        // Output schema
        let output_schema_str = match output_schema {
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
        let (endpoint, description, auth_env_var) = match result.source {
            IntrospectionSource::OpenApi => {
                let api_result = result.api_result.as_ref().ok_or_else(|| {
                    RuntimeError::Generic("No API result to create approval for".into())
                })?;
                let module_name = result
                    .server_name
                    .to_lowercase()
                    .replace(" ", "_")
                    .replace("-", "_");
                (
                    api_result.base_url.clone(),
                    format!(
                        "{} - {} endpoints discovered from OpenAPI spec",
                        api_result.api_title,
                        api_result.endpoints.len()
                    ),
                    if api_result.auth_requirements.auth_type.is_empty()
                        || api_result.auth_requirements.auth_type == "none"
                    {
                        None
                    } else {
                        Some(format!("{}_API_KEY", module_name.to_uppercase()))
                    },
                )
            }
            IntrospectionSource::Browser => {
                let browser_result = result.browser_result.as_ref().ok_or_else(|| {
                    RuntimeError::Generic("No browser result to create approval for".into())
                })?;
                (
                    browser_result.source_url.clone(),
                    format!(
                        "Discovered {} endpoints via browser from {}",
                        browser_result.discovered_endpoints.len(),
                        browser_result.source_url
                    ),
                    None,
                )
            }
            IntrospectionSource::Mcp | IntrospectionSource::McpStdio => (
                result.server_name.clone(),
                format!("MCP server: {}", result.server_name),
                None,
            ),
            _ => {
                return Err(RuntimeError::Generic(format!(
                    "Unsupported introspection source for approval: {:?}",
                    result.source
                )))
            }
        };

        let server_info = ServerInfo {
            name: result.server_name.clone(),
            endpoint,
            description: Some(description.clone()),
            auth_env_var,
            capabilities_path: None,
            alternative_endpoints: vec![],
            capability_files,
        };

        let discovery_source = match result.source {
            IntrospectionSource::OpenApi => DiscoverySource::OpenApi {
                url: spec_url.to_string(),
            },
            IntrospectionSource::Browser => DiscoverySource::HtmlDocs {
                url: spec_url.to_string(), // Using spec_url as the source URL here
            },
            IntrospectionSource::Mcp | IntrospectionSource::McpStdio => DiscoverySource::Mcp {
                endpoint: result.server_name.clone(),
            },
            _ => DiscoverySource::Manual {
                user: "agent".to_string(),
            },
        };

        let approval_id = approval_queue
            .add_server_discovery(
                discovery_source,
                server_info.clone(),
                vec!["dynamic".to_string()],
                RiskAssessment {
                    level: RiskLevel::Medium,
                    reasons: vec![description],
                },
                Some("Agent requested introspection".to_string()),
                expiry_hours,
            )
            .await
            .map_err(|e| {
                RuntimeError::Generic(format!("Failed to create approval request: {}", e))
            })?;

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
