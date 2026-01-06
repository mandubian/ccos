use crate::capability_marketplace::mcp_discovery::{MCPDiscoveryProvider, MCPServerConfig};
use crate::capability_marketplace::types::{CapabilityDiscovery, CapabilityManifest};
use async_trait::async_trait;
use rtfs::runtime::error::RuntimeResult;
use std::any::Any;
use std::path::PathBuf;
use std::sync::Arc;

/// Discovery provider that finds MCP servers from local configuration (overrides.json) and environment variables
pub struct LocalConfigMcpDiscovery {
    overrides_path: Option<PathBuf>,
}

impl LocalConfigMcpDiscovery {
    pub fn new() -> Self {
        Self {
            overrides_path: Self::find_overrides_path(),
        }
    }

    fn find_overrides_path() -> Option<PathBuf> {
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        // Check common locations
        let candidates = vec![
            root.join("capabilities/mcp/overrides.json"),
            root.parent()
                .unwrap_or(&root)
                .join("capabilities/mcp/overrides.json"),
        ];

        for path in candidates {
            if path.exists() {
                return Some(path);
            }
        }
        None
    }

    fn resolve_servers_from_overrides(&self) -> Vec<MCPServerConfig> {
        let mut servers = Vec::new();

        if let Some(path) = &self.overrides_path {
            if let Ok(content) = std::fs::read_to_string(path) {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(entries) = parsed.get("entries").and_then(|e| e.as_array()) {
                        for entry in entries {
                            if let Some(server_config) = self.parse_server_entry(entry) {
                                servers.push(server_config);
                            }
                        }
                    }
                }
            }
        }

        servers
    }

    fn parse_server_entry(&self, entry: &serde_json::Value) -> Option<MCPServerConfig> {
        let server = entry.get("server")?;

        // Prefer explicit server.name when present, otherwise fall back to first match pattern
        let name = server
            .get("name")
            .and_then(|n| n.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                entry
                    .get("matches")
                    .and_then(|m| m.as_array())
                    .and_then(|m| m.first())
                    .and_then(|p| p.as_str())
                    .map(|s| s.trim_end_matches(".*").trim_end_matches('*').to_string())
            })
            .unwrap_or_else(|| "unknown_mcp_server".to_string());

        let remotes = server.get("remotes").and_then(|r| r.as_array())?;

        // Find first HTTP/HTTPS endpoint
        for remote in remotes {
            if let Some(url) = remote.get("url").and_then(|u| u.as_str()) {
                if url.starts_with("http") {
                    return Some(MCPServerConfig {
                        name,
                        endpoint: url.to_string(),
                        auth_token: None, // Auth tokens usually not in checked-in config
                        timeout_seconds: 30,
                        protocol_version: "2024-11-05".to_string(),
                    });
                }
            }
        }

        None
    }

    fn resolve_servers_from_env(&self) -> Vec<MCPServerConfig> {
        let mut servers = Vec::new();

        // GitHub MCP Endpoint
        if let Ok(endpoint) = std::env::var("GITHUB_MCP_ENDPOINT") {
            let token = std::env::var("MCP_AUTH_TOKEN")
                .ok()
                .or_else(|| std::env::var("GITHUB_TOKEN").ok());

            servers.push(MCPServerConfig {
                name: "github".to_string(),
                endpoint,
                auth_token: token,
                timeout_seconds: 30,
                protocol_version: "2024-11-05".to_string(),
            });
        }

        // Add other env var conventions here if needed
        // e.g. SLACK_MCP_ENDPOINT...

        servers
    }

    /// Aggregate all discovered servers, handling duplicates (Env overrides Config)
    pub fn get_all_server_configs(&self) -> Vec<MCPServerConfig> {
        let mut servers_map = std::collections::HashMap::new();

        // 1. Load from overrides.json
        for server in self.resolve_servers_from_overrides() {
            servers_map.insert(server.name.clone(), server);
        }

        // 2. Load from Env (overwriting names if collision)
        for server in self.resolve_servers_from_env() {
            servers_map.insert(server.name.clone(), server);
        }

        servers_map.into_values().collect()
    }
}

#[async_trait]
impl CapabilityDiscovery for LocalConfigMcpDiscovery {
    async fn discover(
        &self,
        marketplace: Option<Arc<crate::capability_marketplace::CapabilityMarketplace>>,
    ) -> RuntimeResult<Vec<CapabilityManifest>> {
        let configs = self.get_all_server_configs();
        let mut all_manifests = Vec::new();

        ccos_println!(
            "🔍 LocalConfigMcpDiscovery found {} servers configuration",
            configs.len()
        );

        // Create unified service once for all servers (more efficient)
        let mut unified_service = crate::mcp::core::MCPDiscoveryService::new();

        // Inject marketplace if available
        if let Some(ref marketplace) = marketplace {
            unified_service = unified_service.with_marketplace(marketplace.clone());
        }

        let unified_service = Arc::new(unified_service);

        for config in configs {
            ccos_println!(
                "   👉 Discovering from server: {} ({})",
                config.name,
                config.endpoint
            );

            // Use unified service for discovery
            // Note: unified service will also check env vars as fallback if auth_token is None
            let options = crate::mcp::types::DiscoveryOptions {
                introspect_output_schemas: false,
                use_cache: true,
                register_in_marketplace: true, // Register in marketplace
                export_to_rtfs: true,          // Export to RTFS files
                export_directory: Some("capabilities/discovered".to_string()),
                non_interactive: true, // Don't hang on prompts during startup
                auth_headers: config.auth_token.as_ref().map(|token| {
                    let mut headers = std::collections::HashMap::new();
                    headers.insert("Authorization".to_string(), format!("Bearer {}", token));
                    headers
                }),
                ..Default::default()
            };

            // Use discover_and_export_tools which handles registration and export automatically
            match unified_service
                .discover_and_export_tools(&config, &options)
                .await
            {
                Ok(manifests) => {
                    ccos_println!("      ✅ Found {} capabilities", manifests.len());
                    all_manifests.extend(manifests);
                }
                Err(e) => {
                    ccos_println!("      ⚠️  Discovery failed for {}: {}", config.name, e);
                }
            }
        }

        Ok(all_manifests)
    }

    fn name(&self) -> &str {
        "LocalConfigMcpDiscovery"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
