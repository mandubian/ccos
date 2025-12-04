//! Server operations - pure logic functions for server management

use super::{ServerInfo, ServerListOutput};
use crate::capability_marketplace::mcp_discovery::{MCPDiscoveryProvider, MCPServerConfig};
use crate::discovery::{
    ApprovalQueue, DiscoverySource, PendingDiscovery, RegistrySearcher, RiskAssessment, RiskLevel,
    ServerInfo as DiscoveryServerInfo,
};
use crate::mcp::core::MCPDiscoveryService;
use crate::mcp::types::{DiscoveryOptions, MCPTool};
use crate::synthesis::introspection::api_introspector::APIIntrospector;
use crate::synthesis::introspection::mcp_introspector::{MCPIntrospectionResult, MCPIntrospector};
use crate::utils::fs::find_workspace_root;
use chrono::Utc;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use std::io::{self, Write};
use std::path::PathBuf;
use uuid::Uuid;

/// List configured servers
pub async fn list_servers() -> RuntimeResult<ServerListOutput> {
    let workspace_root = find_workspace_root();
    let queue = ApprovalQueue::new(&workspace_root);
    let approved = queue.list_approved()?;

    let servers: Vec<ServerInfo> = approved
        .into_iter()
        .map(|server| {
            let id = server.id.clone();
            let name = server.server_info.name.clone();
            let endpoint = server.server_info.endpoint.clone();
            let source = Some(server.source.name());

            let auth_status = if let Some(ref auth_var) = server.server_info.auth_env_var {
                let token_set = std::env::var(auth_var).is_ok();
                if token_set {
                    Some(format!("✓ {} (set)", auth_var))
                } else {
                    Some(format!("⚠ {} (not set)", auth_var))
                }
            } else {
                None
            };

            ServerInfo {
                id,
                name,
                endpoint,
                description: server.server_info.description.clone(),
                source,
                matching_capabilities: None,
                status: if server.should_dismiss() {
                    "failing".to_string()
                } else {
                    "healthy".to_string()
                },
                health_score: Some(server.error_rate()),
                auth_status,
            }
        })
        .collect();

    Ok(ServerListOutput {
        servers: servers.clone(),
        count: servers.len(),
    })
}

/// Add a new server
pub async fn add_server(url: String, name: Option<String>) -> RuntimeResult<String> {
    let workspace_root = find_workspace_root();
    let queue = ApprovalQueue::new(&workspace_root);
    let name_str = name.unwrap_or_else(|| "manual-server".to_string());

    let discovery = PendingDiscovery {
        id: format!("manual-{}", Uuid::new_v4()),
        source: DiscoverySource::Manual {
            user: "cli".to_string(),
        },
        server_info: DiscoveryServerInfo {
            name: name_str.clone(),
            endpoint: url.clone(),
            description: Some("Manually added via CLI".to_string()),
            auth_env_var: Some(ApprovalQueue::suggest_auth_env_var(&name_str)),
            capabilities_path: None,
            alternative_endpoints: Vec::new(),
        },
        domain_match: vec![],
        risk_assessment: RiskAssessment {
            level: RiskLevel::Low,
            reasons: vec!["manual_add".to_string()],
        },
        requested_at: Utc::now(),
        expires_at: Utc::now() + chrono::Duration::hours(24 * 30),
        requesting_goal: None,
    };

    queue.add(discovery.clone())?;
    Ok(discovery.id)
}

/// Remove a server
pub async fn remove_server(name: String) -> RuntimeResult<()> {
    // TODO: Implement server removal logic
    Ok(())
}

/// Show server health status
pub async fn server_health(name: Option<String>) -> RuntimeResult<Vec<ServerInfo>> {
    let workspace_root = find_workspace_root();
    let queue = ApprovalQueue::new(&workspace_root);
    let approved = queue.list_approved()?;

    let target_servers: Vec<_> = if let Some(n) = name {
        approved
            .into_iter()
            .filter(|s| s.server_info.name == n || s.id == n)
            .collect()
    } else {
        approved
    };

    let health_info = target_servers
        .into_iter()
        .map(|server| {
            let id = server.id.clone();
            let name = server.server_info.name.clone();
            let endpoint = server.server_info.endpoint.clone();
            let source = Some(server.source.name());

            let auth_status = if let Some(ref auth_var) = server.server_info.auth_env_var {
                let token_set = std::env::var(auth_var).is_ok();
                if token_set {
                    Some(format!("✓ {} (set)", auth_var))
                } else {
                    Some(format!("⚠ {} (not set)", auth_var))
                }
            } else {
                None
            };

            ServerInfo {
                id,
                name,
                endpoint,
                description: server.server_info.description.clone(),
                source,
                matching_capabilities: None,
                status: if server.should_dismiss() {
                    "failing".to_string()
                } else {
                    "healthy".to_string()
                },
                health_score: Some(server.error_rate()),
                auth_status,
            }
        })
        .collect();

    Ok(health_info)
}

/// Search for servers in registry and overrides
pub async fn search_servers(
    query: String,
    capability: Option<String>,
    _llm: bool,
    _llm_model: Option<String>,
) -> RuntimeResult<Vec<ServerInfo>> {
    let searcher = RegistrySearcher::new();
    let initial_results = searcher.search(&query).await?;

    // Store result and optional matching capabilities
    let mut filtered_results = Vec::new();

    if let Some(ref cap_name) = capability {
        // Filter logic using MCPDiscoveryService
        let discovery_service = MCPDiscoveryService::new();

        for result in initial_results {
            // Only check servers with HTTP endpoints
            if result.server_info.endpoint.is_empty()
                || !result.server_info.endpoint.starts_with("http")
            {
                continue;
            }

            // Create server config
            let server_config = MCPServerConfig {
                name: result.server_info.name.clone(),
                endpoint: result.server_info.endpoint.clone(),
                auth_token: None,    // Will use env vars if needed
                timeout_seconds: 10, // Shorter timeout for search
                protocol_version: "2024-11-05".to_string(),
            };

            // Discover tools from server
            let options = DiscoveryOptions {
                introspect_output_schemas: false,
                use_cache: true,
                register_in_marketplace: false,
                export_to_rtfs: false,
                export_directory: None,
                auth_headers: None,
                ..Default::default()
            };

            match discovery_service
                .discover_tools(&server_config, &options)
                .await
            {
                Ok(tools) => {
                    // Find matching capabilities
                    let matches: Vec<String> = tools
                        .iter()
                        .filter(|tool| {
                            let name_match = tool
                                .tool_name
                                .to_lowercase()
                                .contains(&cap_name.to_lowercase());
                            let desc_match = tool
                                .description
                                .as_ref()
                                .map(|d| d.to_lowercase().contains(&cap_name.to_lowercase()))
                                .unwrap_or(false);
                            name_match || desc_match
                        })
                        .map(|t| t.tool_name.clone())
                        .collect();

                    if !matches.is_empty() {
                        filtered_results.push((result, Some(matches)));
                    }
                }
                Err(_) => {
                    // Skip if discovery fails
                }
            }
        }
    } else {
        filtered_results = initial_results.into_iter().map(|r| (r, None)).collect();
    }

    Ok(filtered_results
        .into_iter()
        .map(|(result, caps)| {
            let server_info = result.server_info;
            ServerInfo {
                id: server_info.name.clone(),
                name: server_info.name,
                endpoint: server_info.endpoint,
                description: server_info.description.clone(),
                source: Some(result.source.name()),
                matching_capabilities: caps,
                status: "pending".to_string(),
                health_score: None,
                auth_status: None,
            }
        })
        .collect())
}

/// Introspect a server to discover its tools/capabilities
///
/// Can be called with either:
/// - Server name (looks up endpoint from approved/pending servers)
/// - Direct endpoint URL
pub async fn introspect_server(server: String) -> RuntimeResult<MCPIntrospectionResult> {
    let workspace_root = find_workspace_root();
    let queue = ApprovalQueue::new(&workspace_root);

    // Try to find server by name in approved or pending
    let (endpoint, auth_env_var_owned) = if server.starts_with("http") {
        // Direct URL provided - no auth_env_var
        (server.clone(), None)
    } else {
        // Look up by name
        let approved = queue.list_approved()?;
        let pending = queue.list_pending()?;

        // Search approved first
        let found_approved = approved
            .iter()
            .find(|s| s.server_info.name == server || s.id == server)
            .map(|s| {
                (
                    s.server_info.endpoint.clone(),
                    s.server_info.auth_env_var.clone(),
                )
            });

        let found = found_approved.or_else(|| {
            // Search pending
            pending
                .iter()
                .find(|s| s.server_info.name == server || s.id == server)
                .map(|s| {
                    (
                        s.server_info.endpoint.clone(),
                        s.server_info.auth_env_var.clone(),
                    )
                })
        });

        if let Some((ep, auth)) = found {
            (ep, auth)
        } else {
            return Err(RuntimeError::Generic(format!(
                "Server '{}' not found. Use the endpoint URL directly or add the server first.",
                server
            )));
        }
    };

    if endpoint.is_empty() || !endpoint.starts_with("http") {
        return Err(RuntimeError::Generic(format!(
            "Server '{}' does not have a valid HTTP endpoint: '{}'",
            server, endpoint
        )));
    }

    // Convert owned Option<String> to Option<&str> for the function call
    let auth_env_var = auth_env_var_owned.as_deref();
    introspect_server_by_url(&endpoint, &server, auth_env_var).await
}

/// Introspect a server by endpoint URL directly (for interactive discovery)
///
/// If auth_env_var is provided, reads the token from that environment variable
/// and includes it in Authorization header.
pub async fn introspect_server_by_url(
    endpoint: &str,
    name: &str,
    auth_env_var: Option<&str>,
) -> RuntimeResult<MCPIntrospectionResult> {
    let introspector = MCPIntrospector::new();

    // Build auth headers if env var is specified
    let auth_headers = if let Some(env_var) = auth_env_var {
        if let Ok(token) = std::env::var(env_var) {
            let mut headers = std::collections::HashMap::new();

            // Format Authorization header
            // Handle cases where token already includes "Bearer " prefix
            let auth_value = if token.trim_start().starts_with("Bearer ") {
                token.trim().to_string()
            } else if token.trim_start().starts_with("bearer ") {
                // Case-insensitive check
                format!(
                    "Bearer {}",
                    token
                        .trim_start()
                        .strip_prefix("bearer ")
                        .unwrap_or(&token)
                        .trim()
                )
            } else {
                // Token doesn't have Bearer prefix - add it
                format!("Bearer {}", token.trim())
            };

            headers.insert("Authorization".to_string(), auth_value.clone());

            // Debug: show what we're sending (masked)
            if std::env::var("CCOS_DEBUG").is_ok() {
                let masked = if auth_value.len() > 20 {
                    format!(
                        "{}...{}",
                        &auth_value[..10],
                        &auth_value[auth_value.len() - 4..]
                    )
                } else {
                    "***".to_string()
                };
                eprintln!("🔐 Using auth from {}: {}", env_var, masked);
                eprintln!("   Authorization header format: Bearer <token>");
            }

            Some(headers)
        } else {
            // Token not set - return helpful error
            let mut error_msg = format!(
                "Authentication required: Environment variable '{}' is not set.\n\
                 Set it with: export {}='your-token-here'\n\
                 Note: Token should be just the token value (we add 'Bearer' prefix automatically)",
                env_var, env_var
            );

            // Add GitHub-specific hint
            if name.to_lowercase().contains("github") {
                error_msg.push_str(&format!(
                    "\n\n💡 For GitHub servers, you can also use:\n\
                     • GITHUB_TOKEN (standard GitHub token)\n\
                     • GITHUB_PAT (Personal Access Token)"
                ));
            }

            return Err(rtfs::runtime::error::RuntimeError::Generic(error_msg));
        }
    } else {
        None
    };

    // Catch auth errors and provide better context
    match introspector
        .introspect_mcp_server_with_auth(endpoint, name, auth_headers.clone())
        .await
    {
        Ok(result) => Ok(result),
        Err(e) => {
            let error_msg = e.to_string();

            // Enhance error message with env var context and GitHub Copilot guidance
            if error_msg.contains("401")
                || error_msg.contains("Unauthorized")
                || error_msg.contains("authentication failed")
            {
                let mut enhanced_msg = error_msg.clone();

                if let Some(env_var) = auth_env_var {
                    enhanced_msg
                        .push_str(&format!("\n\n🔐 Using environment variable: {}", env_var));
                }

                // GitHub Copilot specific guidance
                if endpoint.contains("githubcopilot.com") || name.to_lowercase().contains("github")
                {
                    enhanced_msg.push_str(
                        "\n\n💡 GitHub Copilot MCP requires a GitHub Copilot API token:\n\
                         • This is different from a regular GitHub PAT\n\
                         • Get it from: https://github.com/settings/tokens?type=beta\n\
                         • Or from GitHub Copilot settings\n\
                         • The token should have 'copilot' scope/permissions",
                    );
                }

                return Err(rtfs::runtime::error::RuntimeError::Generic(enhanced_msg));
            }

            Err(e)
        }
    }
}

/// Save discovered tools to RTFS capabilities file and link to pending entry
pub async fn save_discovered_tools(
    introspection: &MCPIntrospectionResult,
    server_info: &DiscoveryServerInfo,
    pending_id: Option<&str>,
) -> RuntimeResult<String> {
    // Convert DiscoveredMCPTool to MCPTool
    let mcp_tools: Vec<MCPTool> = introspection
        .tools
        .iter()
        .map(|tool| {
            MCPTool {
                name: tool.tool_name.clone(),
                description: tool.description.clone(),
                input_schema: tool.input_schema_json.clone(),
                output_schema: None, // DiscoveredMCPTool doesn't have output_schema_json
                metadata: None,
                annotations: None,
            }
        })
        .collect();

    if mcp_tools.is_empty() {
        return Err(RuntimeError::Generic("No tools to save".to_string()));
    }

    // Create server config
    let server_config = MCPServerConfig {
        name: introspection.server_name.clone(),
        endpoint: introspection.server_url.clone(),
        auth_token: None, // Don't store token in file
        timeout_seconds: 30,
        protocol_version: introspection.protocol_version.clone(),
    };

    // Create discovery provider
    let provider = MCPDiscoveryProvider::new(server_config.clone())?;

    // Convert tools to RTFS format
    let rtfs_capabilities = provider.convert_tools_to_rtfs_format(&mcp_tools)?;

    // Find the pending entry to get the server ID
    // Use workspace root to ensure server.json is saved in the correct location
    let workspace_root = find_workspace_root();
    let queue = ApprovalQueue::new(&workspace_root);
    let pending = queue.list_pending()?;

    // Find the pending entry for this server
    // Use server_info from the parameter (which matches the discovery result) rather than introspection
    // because introspection.server_name might differ from the discovery server_info.name
    let entry = if let Some(id) = pending_id {
        // Use provided ID if available
        pending.iter().find(|e| e.id == id)
    } else {
        // Fallback to name/endpoint matching using server_info (from discovery) not introspection
        pending.iter().find(|e| {
            e.server_info.name == server_info.name
                || e.server_info.endpoint == server_info.endpoint
                || e.server_info.endpoint == introspection.server_url
        })
    }
    .ok_or_else(|| {
        RuntimeError::Generic(format!(
            "Pending entry not found for server: {} (searched by ID: {:?}, name: {}, endpoint: {})",
            server_info.name, pending_id, server_info.name, server_info.endpoint
        ))
    })?;

    // Use the same server_id format as approval flow (sanitize_filename to match directory structure)
    let server_id = crate::utils::fs::sanitize_filename(&entry.server_info.name);

    // Find workspace root to ensure we save to the correct capabilities/ directory
    let workspace_root = find_workspace_root();
    
    // Debug: log the workspace root being used
    eprintln!("📁 Using workspace root: {}", workspace_root.display());
    
    // Save to capabilities/servers/pending/{server_id}/capabilities.rtfs
    // This matches the approval flow which moves files from pending to approved
    let pending_dir = workspace_root.join("capabilities/servers/pending");
    let server_dir = pending_dir.join(&server_id);

    // Create directory if it doesn't exist
    std::fs::create_dir_all(&server_dir).map_err(|e| {
        RuntimeError::Generic(format!(
            "Failed to create pending capabilities directory: {}",
            e
        ))
    })?;

    let capabilities_file = server_dir.join("capabilities.rtfs");
    let capabilities_path = capabilities_file.to_string_lossy().to_string();

    // Check if capabilities file already exists (from a previous introspection)
    if capabilities_file.exists() {
        // Check if this is the same server (by comparing server name/endpoint)
        // If it's the same, we can safely overwrite (update)
        // If it's different, we should warn or merge
        eprintln!(
            "⚠️  Capabilities file already exists for pending server: {}",
            server_id
        );
        eprintln!(
            "   Existing file: {}",
            capabilities_path
        );
        eprintln!("   Updating with new introspection results...");
    }

    // Save RTFS capabilities (overwrites existing file if present)
    provider.save_rtfs_capabilities(&rtfs_capabilities, &capabilities_path)?;

    // Update the pending entry to include capabilities_path
    // IMPORTANT: Update in place to avoid removing the directory (which would delete capabilities.rtfs)
    let mut updated_entry = entry.clone();
    updated_entry.server_info.capabilities_path = Some(capabilities_path.clone());

    // Update the entry in place (preserves directory and capabilities.rtfs file)
    queue.update_pending(&updated_entry)?;

    Ok(capabilities_path)
}

/// Save API capabilities discovered from documentation parsing
pub async fn save_api_capabilities(
    api_result: &crate::synthesis::introspection::APIIntrospectionResult,
    pending_id: &str,
) -> RuntimeResult<String> {
    use std::io::Write;

    let workspace_root = find_workspace_root();
    let queue = crate::discovery::ApprovalQueue::new(&workspace_root);
    let entry = queue.get_pending(pending_id)?.ok_or_else(|| {
        RuntimeError::Generic(format!("Pending entry not found for ID: {}", pending_id))
    })?;

    // Generate RTFS content for HTTP API capabilities
    let mut rtfs_content = String::new();
    rtfs_content.push_str(";; Auto-generated HTTP API capabilities from documentation parsing\n");
    rtfs_content.push_str(&format!(
        ";; API: {} v{}\n",
        api_result.api_title, api_result.api_version
    ));
    rtfs_content.push_str(&format!(";; Base URL: {}\n\n", api_result.base_url));

    // Create a module for the API
    let module_name = api_result
        .api_title
        .to_lowercase()
        .replace(" ", "_")
        .replace("-", "_")
        .replace(".", "_");

    rtfs_content.push_str(&format!("(module {}\n", module_name));
    rtfs_content.push_str("  :version \"1.0.0\"\n");
    rtfs_content.push_str(&format!("  :description \"{}\"\n\n", api_result.api_title));

    // Generate capability for each endpoint
    for endpoint in &api_result.endpoints {
        let cap_name = endpoint
            .endpoint_id
            .replace(".", "_")
            .replace("-", "_")
            .replace("/", "_");

        rtfs_content.push_str(&format!("  (def-capability {}\n", cap_name));
        rtfs_content.push_str("    :type :capability\n");
        rtfs_content.push_str(&format!("    :id \"{}.{}\"\n", module_name, cap_name));
        rtfs_content.push_str(&format!("    :name \"{}\"\n", endpoint.name));
        rtfs_content.push_str(&format!(
            "    :description \"{}\"\n",
            endpoint.description.replace("\"", "\\\"")
        ));
        rtfs_content.push_str("    :version \"1.0.0\"\n");
        rtfs_content.push_str("    :provider :Http\n");
        rtfs_content.push_str(&format!("    :provider-meta {{\n"));
        rtfs_content.push_str(&format!("      :base-url \"{}\"\n", api_result.base_url));
        rtfs_content.push_str(&format!("      :method \"{}\"\n", endpoint.method));
        rtfs_content.push_str(&format!("      :path \"{}\"\n", endpoint.path));
        rtfs_content.push_str("      :timeout-ms 30000\n");
        if endpoint.requires_auth {
            let auth = &api_result.auth_requirements;
            rtfs_content.push_str(&format!("      :auth-type \"{}\"\n", auth.auth_type));
            rtfs_content.push_str(&format!(
                "      :auth-location \"{}\"\n",
                auth.auth_location
            ));
        }
        rtfs_content.push_str("    }\n");

        // Input schema
        if let Some(ref schema) = endpoint.input_schema {
            rtfs_content.push_str(&format!("    :input-schema {}\n", format_type_expr(schema)));
        } else {
            rtfs_content.push_str("    :input-schema :any\n");
        }

        // Output schema
        if let Some(ref schema) = endpoint.output_schema {
            rtfs_content.push_str(&format!(
                "    :output-schema {}\n",
                format_type_expr(schema)
            ));
        } else {
            rtfs_content.push_str("    :output-schema :any\n");
        }

        rtfs_content.push_str("    :permissions [:http]\n");
        rtfs_content.push_str("    :effects []\n");
        rtfs_content.push_str("  )\n\n");
    }

    rtfs_content.push_str(")\n");

    // Save to file
    let server_id = entry
        .server_info
        .name
        .to_lowercase()
        .replace(" ", "_")
        .replace("/", "_");

    let pending_dir = std::path::Path::new("capabilities/servers/pending");
    let server_dir = pending_dir.join(&server_id);

    std::fs::create_dir_all(&server_dir).map_err(|e| {
        RuntimeError::Generic(format!(
            "Failed to create pending capabilities directory: {}",
            e
        ))
    })?;

    let capabilities_file = server_dir.join("capabilities.rtfs");
    let capabilities_path = capabilities_file.to_string_lossy().to_string();

    let mut file = std::fs::File::create(&capabilities_file)
        .map_err(|e| RuntimeError::Generic(format!("Failed to create capabilities file: {}", e)))?;

    file.write_all(rtfs_content.as_bytes())
        .map_err(|e| RuntimeError::Generic(format!("Failed to write capabilities: {}", e)))?;

    // Update the pending entry to include capabilities_path
    let mut updated_entry = entry.clone();
    updated_entry.server_info.capabilities_path = Some(capabilities_path.clone());

    queue.remove_pending(&entry.id)?;
    queue.add(updated_entry)?;

    Ok(capabilities_path)
}

/// Helper to format TypeExpr as RTFS string
fn format_type_expr(type_expr: &rtfs::ast::TypeExpr) -> String {
    use rtfs::ast::TypeExpr;

    match type_expr {
        TypeExpr::Primitive(p) => format!(":{:?}", p).to_lowercase(),
        TypeExpr::Any => ":any".to_string(),
        TypeExpr::Vector(inner) => format!("[{}]", format_type_expr(inner)),
        TypeExpr::Map { entries, wildcard } => {
            let mut parts = Vec::new();
            for entry in entries {
                let opt = if entry.optional { " :optional" } else { "" };
                parts.push(format!(
                    ":{} {}{}",
                    entry.key.0,
                    format_type_expr(&entry.value_type),
                    opt
                ));
            }
            if let Some(w) = wildcard {
                parts.push(format!(":* {}", format_type_expr(w)));
            }
            format!("{{{}}}", parts.join(" "))
        }
        TypeExpr::Alias(s) => format!(":{}", s.0),
        _ => ":any".to_string(),
    }
}
