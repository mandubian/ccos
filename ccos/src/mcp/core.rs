//! Unified MCP Discovery Service
//!
//! This module provides a single, unified API for MCP discovery that consolidates
//! the discovery logic from multiple modules.
//!
//! This service provides:
//! - Tool discovery from MCP servers
//! - Resource discovery from MCP servers
//! - Schema conversion (JSON Schema → RTFS TypeExpr)
//! - Manifest creation from discovered tools
//! - Automatic registration in marketplace and catalog
//! - Caching support for discovered tools
//! - Rate limiting and retry policies

use crate::mcp::discovery_session::{MCPSessionManager, MCPServerInfo};
use crate::mcp::registry::MCPRegistryClient;
use crate::mcp::types::*;
use crate::mcp::cache::MCPCache;
use crate::mcp::rate_limiter::{RateLimiter, RetryContext};
use crate::capability_marketplace::types::{CapabilityManifest, MCPCapability, ProviderType};
use crate::capability_marketplace::CapabilityMarketplace;
use crate::catalog::{CatalogService, CatalogSource};
use crate::planner::modular_planner::types::DomainHint;
use crate::capability_marketplace::config_mcp_discovery::LocalConfigMcpDiscovery;
use crate::synthesis::mcp_introspector::MCPIntrospector;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use std::collections::HashMap;
use std::sync::Arc;

/// Unified MCP Discovery Service
///
/// Provides a single API for discovering MCP capabilities across all CCOS modules.
/// Consolidates discovery logic from resolution, marketplace, and introspection modules.
pub struct MCPDiscoveryService {
    session_manager: Arc<MCPSessionManager>,
    registry_client: MCPRegistryClient,
    config_discovery: LocalConfigMcpDiscovery,
    introspector: MCPIntrospector,
    cache: Arc<MCPCache>,
    rate_limiter: Arc<RateLimiter>,
    /// Optional marketplace for automatic registration
    marketplace: Option<Arc<CapabilityMarketplace>>,
    /// Optional catalog for automatic indexing
    catalog: Option<Arc<CatalogService>>,
}

impl MCPDiscoveryService {
    /// Create a new MCP discovery service
    pub fn new() -> Self {
        Self {
            session_manager: Arc::new(MCPSessionManager::new(None)),
            registry_client: MCPRegistryClient::new(),
            config_discovery: LocalConfigMcpDiscovery::new(),
            introspector: MCPIntrospector::new(),
            cache: Arc::new(MCPCache::new()),
            rate_limiter: Arc::new(RateLimiter::new()),
            marketplace: None,
            catalog: None,
        }
    }

    /// Create a new MCP discovery service with custom auth headers
    pub fn with_auth_headers(auth_headers: Option<HashMap<String, String>>) -> Self {
        Self {
            session_manager: Arc::new(MCPSessionManager::new(auth_headers)),
            registry_client: MCPRegistryClient::new(),
            config_discovery: LocalConfigMcpDiscovery::new(),
            introspector: MCPIntrospector::new(),
            cache: Arc::new(MCPCache::new()),
            rate_limiter: Arc::new(RateLimiter::new()),
            marketplace: None,
            catalog: None,
        }
    }

    /// Add marketplace for automatic capability registration
    pub fn with_marketplace(mut self, marketplace: Arc<CapabilityMarketplace>) -> Self {
        self.marketplace = Some(marketplace);
        self
    }

    /// Add catalog for automatic capability indexing
    pub fn with_catalog(mut self, catalog: Arc<CatalogService>) -> Self {
        self.catalog = Some(catalog);
        self
    }

    /// Discover tools from an MCP server
    ///
    /// This is the core discovery method that all modules should use.
    /// It handles session management, caching, rate limiting, retries, and schema conversion.
    pub async fn discover_tools(
        &self,
        server_config: &MCPServerConfig,
        options: &DiscoveryOptions,
    ) -> RuntimeResult<Vec<DiscoveredMCPTool>> {
        // Check cache first if enabled
        if options.use_cache {
            if let Some(cached_tools) = self.cache.get(server_config) {
                return Ok(cached_tools);
            }
        }

        // Apply rate limiting if enabled
        if options.rate_limit.enabled {
            self.rate_limiter.set_server_config(&server_config.endpoint, options.rate_limit.clone());
            self.rate_limiter.acquire_async(&server_config.endpoint).await;
        }

        // Set up retry context
        let mut retry_ctx = RetryContext::new(options.retry_policy.clone());

        // Attempt discovery with retries
        loop {
            match self.discover_tools_inner(server_config, options).await {
                Ok(tools) => {
                    retry_ctx.success();
                    return Ok(tools);
                }
                Err(e) => {
                    // Check if we should retry
                    let should_retry = self.is_retryable_error(&e, &options.retry_policy);
                    
                    if should_retry {
                        if let Some(delay) = retry_ctx.next_attempt(Some(e.to_string())) {
                            log::warn!(
                                "Discovery failed for {} (attempt {}), retrying in {:?}: {}",
                                server_config.name,
                                retry_ctx.attempt,
                                delay,
                                e
                            );
                            tokio::time::sleep(delay).await;
                            
                            // Re-acquire rate limit token for retry
                            if options.rate_limit.enabled {
                                self.rate_limiter.acquire_async(&server_config.endpoint).await;
                            }
                            continue;
                        }
                    }
                    
                    // No more retries or non-retryable error
                    return Err(e);
                }
            }
        }
    }

    /// Check if an error is retryable based on the retry policy
    fn is_retryable_error(&self, error: &RuntimeError, policy: &crate::mcp::rate_limiter::RetryPolicy) -> bool {
        let error_str = error.to_string().to_lowercase();
        
        // Check for rate limiting (429)
        if error_str.contains("429") || error_str.contains("too many requests") || error_str.contains("rate limit") {
            return policy.should_retry_status(429);
        }
        
        // Check for server errors
        if error_str.contains("500") || error_str.contains("internal server error") {
            return policy.should_retry_status(500);
        }
        if error_str.contains("502") || error_str.contains("bad gateway") {
            return policy.should_retry_status(502);
        }
        if error_str.contains("503") || error_str.contains("service unavailable") {
            return policy.should_retry_status(503);
        }
        if error_str.contains("504") || error_str.contains("gateway timeout") {
            return policy.should_retry_status(504);
        }
        
        // Check for network errors (transient)
        if error_str.contains("timeout") || error_str.contains("connection") || error_str.contains("network") {
            return true;
        }
        
        false
    }

    /// Inner discovery method (without retry logic)
    async fn discover_tools_inner(
        &self,
        server_config: &MCPServerConfig,
        options: &DiscoveryOptions,
    ) -> RuntimeResult<Vec<DiscoveredMCPTool>> {
        let auth_headers = if let Some(ref custom_auth) = options.auth_headers {
            Some(custom_auth.clone())
        } else if let Some(ref token) = server_config.auth_token {
            let mut headers = HashMap::new();
            headers.insert("Authorization".to_string(), format!("Bearer {}", token));
            Some(headers)
        } else {
            // Fallback: try to get token from environment variables
            // Check server-specific token first (e.g., GITHUB_MCP_TOKEN)
            let namespace = if let Some(slash_pos) = server_config.name.find('/') {
                &server_config.name[..slash_pos]
            } else {
                &server_config.name
            };
            let normalized_namespace = namespace.replace('-', "_").to_uppercase();
            let server_specific_var = format!("{}_MCP_TOKEN", normalized_namespace);
            
            let token = std::env::var(&server_specific_var)
                .ok()
                .or_else(|| {
                    // For GitHub, also check legacy names
                    if namespace.to_lowercase() == "github" {
                        std::env::var("GITHUB_PAT")
                            .ok()
                            .or_else(|| std::env::var("GITHUB_TOKEN").ok())
                    } else {
                        None
                    }
                })
                .or_else(|| std::env::var("MCP_AUTH_TOKEN").ok());
            
            if let Some(token) = token {
                if !token.is_empty() {
                    let mut headers = HashMap::new();
                    headers.insert("Authorization".to_string(), format!("Bearer {}", token));
                    Some(headers)
                } else {
                    None
                }
            } else {
                None
            }
        };

        // Create session manager with auth if needed
        let session_manager = if auth_headers.is_some() {
            Arc::new(MCPSessionManager::new(auth_headers.clone()))
        } else {
            Arc::clone(&self.session_manager)
        };

        // Initialize session
        let client_info = MCPServerInfo {
            name: "ccos-discovery-service".to_string(),
            version: "1.0.0".to_string(),
        };

        let session = session_manager
            .initialize_session(&server_config.endpoint, &client_info)
            .await?;

        // Call tools/list
        let tools_response = session_manager
            .make_request(&session, "tools/list", serde_json::json!({}))
            .await;

        // Terminate session
        let _ = session_manager.terminate_session(&session).await;

        // Parse response
        let mcp_response = tools_response?;
        let tools_array = mcp_response
            .get("result")
            .and_then(|r| r.get("tools"))
            .and_then(|t| t.as_array())
            .ok_or_else(|| RuntimeError::Generic("Invalid MCP tools/list response".to_string()))?;

        // Parse tools using introspector
        let mut discovered_tools = Vec::new();
        for tool_json in tools_array {
            // Use introspector's parse method (we'll need to expose it or replicate logic)
            let tool_name = tool_json
                .get("name")
                .and_then(|n| n.as_str())
                .ok_or_else(|| RuntimeError::Generic("MCP tool missing name".to_string()))?
                .to_string();

            let description = tool_json
                .get("description")
                .and_then(|d| d.as_str())
                .map(|s| s.to_string());

            // Convert input schema
            let (input_schema, input_schema_json) = if let Some(schema) = tool_json.get("inputSchema") {
                let type_expr = MCPIntrospector::type_expr_from_json_schema(schema)?;
                (Some(type_expr), Some(schema.clone()))
            } else {
                (None, None)
            };

            discovered_tools.push(DiscoveredMCPTool {
                tool_name,
                description,
                input_schema,
                output_schema: None, // Will be filled in below if introspection is enabled
                input_schema_json,
            });
        }

        // Output schema introspection (if requested)
        // This is done after the initial discovery to avoid multiple sessions
        if options.introspect_output_schemas && auth_headers.is_some() {
            log::info!("🔍 Introspecting output schemas for {} tools...", discovered_tools.len());
            
            for tool in &mut discovered_tools {
                match self.introspector.introspect_output_schema(
                    tool,
                    &server_config.endpoint,
                    &server_config.name,
                    auth_headers.clone(),
                    None, // No input overrides
                ).await {
                    Ok((schema, _sample)) => {
                        tool.output_schema = schema;
                    }
                    Err(e) => {
                        log::warn!("Failed to introspect output schema for {}: {}", tool.tool_name, e);
                    }
                }
            }
        }

        // Cache the results
        if options.use_cache {
            self.cache.store(server_config, discovered_tools.clone());
        }

        Ok(discovered_tools)
    }

    /// Discover resources from an MCP server
    pub async fn discover_resources(
        &self,
        server_config: &MCPServerConfig,
    ) -> RuntimeResult<Vec<serde_json::Value>> {
        // Build auth headers
        let auth_headers = if let Some(ref token) = server_config.auth_token {
            let mut headers = HashMap::new();
            headers.insert("Authorization".to_string(), format!("Bearer {}", token));
            Some(headers)
        } else {
            None
        };

        // Create session manager with auth if needed
        let session_manager = if auth_headers.is_some() {
            Arc::new(MCPSessionManager::new(auth_headers))
        } else {
            Arc::clone(&self.session_manager)
        };

        // Initialize session
        let client_info = MCPServerInfo {
            name: "ccos-discovery-service".to_string(),
            version: "1.0.0".to_string(),
        };

        let session = session_manager
            .initialize_session(&server_config.endpoint, &client_info)
            .await?;

        // Call resources/list
        let resources_response = session_manager
            .make_request(&session, "resources/list", serde_json::json!({}))
            .await;

        // Terminate session
        let _ = session_manager.terminate_session(&session).await;

        // Parse response
        let mcp_response = resources_response?;
        let resources_array = mcp_response
            .get("result")
            .and_then(|r| r.get("resources"))
            .and_then(|t| t.as_array())
            .ok_or_else(|| RuntimeError::Generic("Invalid MCP resources/list response".to_string()))?;

        Ok(resources_array.clone())
    }

    /// Convert a discovered tool to a capability manifest
    pub fn tool_to_manifest(
        &self,
        tool: &DiscoveredMCPTool,
        server_config: &MCPServerConfig,
    ) -> CapabilityManifest {
        let capability_id = format!("mcp.{}.{}", server_config.name, tool.tool_name);

        let provider = ProviderType::MCP(MCPCapability {
            server_url: server_config.endpoint.clone(),
            tool_name: tool.tool_name.clone(),
            timeout_ms: server_config.timeout_seconds * 1000,
            auth_token: server_config.auth_token.clone(),
        });

        let mut manifest = CapabilityManifest::new(
            capability_id,
            tool.tool_name.clone(),
            tool.description.clone().unwrap_or_default(),
            provider,
            "1.0.0".to_string(),
        );

        manifest.input_schema = tool.input_schema.clone();
        manifest.output_schema = tool.output_schema.clone();

        // Add metadata
        manifest.metadata.insert(
            "mcp_server_name".to_string(),
            server_config.name.clone(),
        );
        manifest.metadata.insert(
            "mcp_server_endpoint".to_string(),
            server_config.endpoint.clone(),
        );
        manifest.metadata.insert(
            "discovery_source".to_string(),
            "mcp_unified_service".to_string(),
        );

        manifest
    }

    /// Register a discovered capability in marketplace and catalog
    pub async fn register_capability(
        &self,
        manifest: &CapabilityManifest,
    ) -> RuntimeResult<()> {
        // Register in marketplace if available
        if let Some(ref marketplace) = self.marketplace {
            marketplace
                .register_capability_manifest(manifest.clone())
                .await?;
        }

        // Index in catalog if available
        if let Some(ref catalog) = self.catalog {
            catalog.register_capability(manifest, CatalogSource::Discovered);
        }

        Ok(())
    }

    /// Discover tools and optionally export them to RTFS module file
    ///
    /// This is a convenience method that combines discovery, registration, and export.
    pub async fn discover_and_export_tools(
        &self,
        server_config: &MCPServerConfig,
        options: &DiscoveryOptions,
    ) -> RuntimeResult<Vec<CapabilityManifest>> {
        // Discover tools
        let tools = self.discover_tools(server_config, options).await?;

        // Convert to manifests
        let mut manifests = Vec::new();
        for tool in &tools {
            let manifest = self.tool_to_manifest(tool, server_config);
            manifests.push(manifest);
        }

        // Register if requested
        if options.register_in_marketplace {
            for manifest in &manifests {
                self.register_capability(manifest).await?;
            }
        }

        // Export to RTFS if requested
        if options.export_to_rtfs {
            self.export_server_capabilities_to_rtfs(server_config, &manifests, options).await?;
        }

        Ok(manifests)
    }

    /// Export capabilities from a server to a single RTFS module file
    async fn export_server_capabilities_to_rtfs(
        &self,
        server_config: &MCPServerConfig,
        manifests: &[CapabilityManifest],
        options: &DiscoveryOptions,
    ) -> RuntimeResult<()> {
        use std::path::PathBuf;
        use std::fs;

        // Determine export directory
        let export_dir = options.export_directory.as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::var("CCOS_CAPABILITY_STORAGE")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| PathBuf::from("capabilities/discovered"))
            });

        // Create directory structure: capabilities/discovered/mcp/<server_name>/
        let server_dir = export_dir.join("mcp").join(&server_config.name);
        fs::create_dir_all(&server_dir).map_err(|e| {
            RuntimeError::Generic(format!("Failed to create export directory: {}", e))
        })?;

        // Export to single module file: capabilities/discovered/mcp/<server_name>/capabilities.rtfs
        let module_file = server_dir.join("capabilities.rtfs");
        
        // Create RTFS module with only the discovered capabilities from this server
        let mut rtfs_content = String::new();
        rtfs_content.push_str(";; CCOS MCP Capabilities Module\n");
        rtfs_content.push_str(&format!(";; Generated at: {}\n", chrono::Utc::now()));
        rtfs_content.push_str(&format!(";; Server: {} ({})\n\n", server_config.name, server_config.endpoint));
        rtfs_content.push_str("(do\n");

        for manifest in manifests {
            // Generate implementation code for MCP capabilities
            let implementation_code = match &manifest.provider {
                ProviderType::MCP(mcp) => format!(
                    "(fn [input] (call :ccos.capabilities.mcp.call :server-url \"{}\" :tool-name \"{}\" :input input))",
                    mcp.server_url, mcp.tool_name
                ),
                _ => "(fn [input] (error \"Implementation not available\"))".to_string(),
            };

            let cap_rtfs = crate::synthesis::missing_capability_resolver::MissingCapabilityResolver::manifest_to_rtfs(
                manifest,
                &implementation_code,
            );
            rtfs_content.push_str(&format!("  {}\n\n", cap_rtfs));
        }

        rtfs_content.push_str(")\n");
        fs::write(&module_file, rtfs_content).map_err(|e| {
            RuntimeError::Generic(format!("Failed to write RTFS module file: {}", e))
        })?;
        println!("  💾 Exported {} capabilities to {}", manifests.len(), module_file.display());

        Ok(())
    }

    /// Get server config for a domain hint
    pub fn get_server_for_domain(&self, domain: &DomainHint) -> Option<MCPServerConfig> {
        let hint = match domain {
            DomainHint::GitHub => "github",
            DomainHint::Slack => "slack",
            DomainHint::FileSystem => "filesystem",
            DomainHint::Database => "database",
            DomainHint::Web => "web",
            DomainHint::Email => "email",
            DomainHint::Calendar => "calendar",
            DomainHint::Generic => "general",
            DomainHint::Custom(s) => s.as_str(),
        };

        let configs = self.config_discovery.get_all_server_configs();
        for config in configs {
            if config.name.contains(hint) || hint.contains(&config.name) {
                return Some(config);
            }
        }

        None
    }

    /// List all known servers
    pub fn list_known_servers(&self) -> Vec<MCPServerConfig> {
        self.config_discovery.get_all_server_configs()
    }
}

impl Default for MCPDiscoveryService {
    fn default() -> Self {
        Self::new()
    }
}

