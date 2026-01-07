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

use crate::approval::{
    queue::ApprovedDiscovery, storage_file::FileApprovalStorage, UnifiedApprovalQueue,
};
use crate::capability_marketplace::config_mcp_discovery::LocalConfigMcpDiscovery;
use crate::capability_marketplace::mcp_discovery::{MCPDiscoveryProvider, MCPServerConfig};
use crate::capability_marketplace::types::{CapabilityManifest, MCPCapability, ProviderType};
use crate::capability_marketplace::CapabilityMarketplace;
use crate::catalog::{CatalogService, CatalogSource};
use crate::mcp::cache::MCPCache;
use crate::mcp::discovery_session::{MCPServerInfo, MCPSessionManager};
use crate::mcp::rate_limiter::{RateLimiter, RetryContext};
use crate::mcp::registry::MCPRegistryClient;
use crate::mcp::types::*;
use crate::planner::modular_planner::types::DomainHint;
use crate::synthesis::mcp_introspector::MCPIntrospector;
use crate::utils::fs::get_workspace_root;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use std::collections::HashMap;
use std::sync::Arc;

/// Unified MCP Discovery Service
///
/// Provides a single API for discovering MCP capabilities across all CCOS modules.
/// Consolidates discovery logic from resolution, marketplace, and introspection modules.
pub struct MCPDiscoveryService {
    /// Shared HTTP client for connection pooling and reuse
    http_client: Arc<reqwest::Client>,
    session_manager: Arc<MCPSessionManager>,
    registry_client: MCPRegistryClient,
    config_discovery: LocalConfigMcpDiscovery,
    approval_queue: UnifiedApprovalQueue<FileApprovalStorage>,
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
        // Create a shared HTTP client with connection pooling
        let http_client = Arc::new(
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .connect_timeout(std::time::Duration::from_secs(10))
                .pool_max_idle_per_host(10) // Reuse connections
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()), // Fallback if builder fails
        );

        let workspace_root = get_workspace_root();
        // If the workspace root is set to the config directory, the approval queue
        // (which looks for "capabilities/servers/approved") needs to look in the parent.
        let approval_base = if workspace_root.ends_with("config") {
            workspace_root
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or(workspace_root.clone())
        } else {
            workspace_root.clone()
        };

        let storage_path =
            approval_base.join(&rtfs::config::AgentConfig::from_env().storage.approvals_dir);
        let storage = Arc::new(
            FileApprovalStorage::new(storage_path).expect("Failed to create approval storage"),
        );
        let approval_queue = UnifiedApprovalQueue::new(storage);

        Self {
            http_client: Arc::clone(&http_client),
            session_manager: Arc::new(MCPSessionManager::with_client(http_client, None)),
            registry_client: MCPRegistryClient::new(),
            config_discovery: LocalConfigMcpDiscovery::new(),
            approval_queue,
            introspector: MCPIntrospector::new(),
            cache: Arc::new(MCPCache::new()),
            rate_limiter: Arc::new(RateLimiter::new()),
            marketplace: None,
            catalog: None,
        }
    }

    /// Create a new MCP discovery service with custom auth headers
    pub fn with_auth_headers(auth_headers: Option<HashMap<String, String>>) -> Self {
        // Create a shared HTTP client with connection pooling
        let http_client = Arc::new(
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .connect_timeout(std::time::Duration::from_secs(10))
                .pool_max_idle_per_host(10) // Reuse connections
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()), // Fallback if builder fails
        );

        let workspace_root = get_workspace_root();
        // If the workspace root is set to the config directory, the approval queue
        // (which looks for "capabilities/servers/approved") needs to look in the parent.
        let approval_base = if workspace_root.ends_with("config") {
            workspace_root
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or(workspace_root.clone())
        } else {
            workspace_root.clone()
        };

        let storage_path =
            approval_base.join(&rtfs::config::AgentConfig::from_env().storage.approvals_dir);
        let storage = Arc::new(
            FileApprovalStorage::new(storage_path).expect("Failed to create approval storage"),
        );
        let approval_queue = UnifiedApprovalQueue::new(storage);

        Self {
            http_client: Arc::clone(&http_client),
            session_manager: Arc::new(MCPSessionManager::with_client(http_client, auth_headers)),
            registry_client: MCPRegistryClient::new(),
            config_discovery: LocalConfigMcpDiscovery::new(),
            approval_queue,
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
    /// For approved non-MCP servers, it loads RTFS capability files instead of connecting via MCP.
    pub async fn discover_tools(
        &self,
        server_config: &MCPServerConfig,
        options: &DiscoveryOptions,
    ) -> RuntimeResult<Vec<DiscoveredMCPTool>> {
        // Check for approved RTFS capability files first (for non-MCP servers)
        if !options.ignore_approved_files {
            ccos_eprintln!("🔍 Checking approved servers for: {}", server_config.name);
            match self.approval_queue.list_approved_servers().await {
                Ok(approved_requests) => {
                    let approved: Vec<_> = approved_requests
                        .iter()
                        .filter_map(|r| r.to_approved_discovery())
                        .collect();
                    ccos_eprintln!("📋 Loaded {} approved server(s) from queue", approved.len());
                    log::debug!("Loaded {} approved server(s) from queue", approved.len());

                    // Try to find matching approved server
                    let approved_server = approved.iter().find(|s| {
                        // Match by name (handle slashes vs underscores) or endpoint
                        let server_name_normalized = server_config
                            .name
                            .replace("/", "_")
                            .replace(" ", "_")
                            .to_lowercase();
                        let approved_name_normalized = s
                            .server_info
                            .name
                            .replace("/", "_")
                            .replace(" ", "_")
                            .to_lowercase();
                        let matches = server_name_normalized == approved_name_normalized
                            || s.server_info.name == server_config.name
                            || s.server_info.endpoint == server_config.endpoint;
                        if matches {
                            ccos_eprintln!(
                                "✅ Found approved server match: {} (normalized: {})",
                                s.server_info.name,
                                approved_name_normalized
                            );
                            log::debug!(
                                "Found approved server match: {} (normalized: {})",
                                s.server_info.name,
                                approved_name_normalized
                            );
                        }
                        matches
                    });

                    if let Some(approved_server) = approved_server {
                        // Try to load capability files - use from approved.json if available, otherwise discover from directory
                        let mut files_to_load =
                            if let Some(ref files) = approved_server.capability_files {
                                if !files.is_empty() {
                                    files.clone()
                                } else {
                                    Vec::new()
                                }
                            } else {
                                Vec::new()
                            };

                        // If capability_files is not set or empty, try to discover files from directory
                        if files_to_load.is_empty() {
                            let server_id = approved_server
                                .server_info
                                .name
                                .to_lowercase()
                                .replace(" ", "_")
                                .replace("/", "_");
                            let approved_roots = [
                                std::path::Path::new("capabilities/servers/approved").to_path_buf(),
                                std::path::Path::new("../capabilities/servers/approved")
                                    .to_path_buf(),
                            ];
                            let approved_dir = approved_roots
                                .iter()
                                .find(|p| p.join(&server_id).exists())
                                .map(|p| p.join(&server_id))
                                .unwrap_or_else(|| {
                                    approved_roots
                                        .first()
                                        .cloned()
                                        .unwrap_or_else(|| {
                                            std::path::PathBuf::from(
                                                "capabilities/servers/approved",
                                            )
                                        })
                                        .join(&server_id)
                                });

                            if approved_dir.exists() {
                                // Recursively collect RTFS files from directory
                                Self::collect_rtfs_files_recursive(
                                    &approved_dir,
                                    "capabilities/servers/approved",
                                    &mut files_to_load,
                                );

                                if !files_to_load.is_empty() {
                                    // Update approved.json with found files (best effort)
                                    let _ = self
                                        .update_approved_capability_files(
                                            &approved_server.id,
                                            &files_to_load,
                                        )
                                        .await;
                                } else {
                                    // Keep going to MCP discovery if no files found
                                    ccos_eprintln!("⚠️  No RTFS capability files found for approved server: {} (searched in {})",
                                        approved_server.server_info.name,
                                        approved_dir.display()
                                    );
                                }
                            }
                        }

                        if !files_to_load.is_empty() {
                            // Load RTFS capability files instead of connecting via MCP
                            ccos_eprintln!(
                                "🔄 Loading RTFS capabilities for approved server: {}",
                                approved_server.server_info.name
                            );
                            log::info!(
                                "Loading RTFS capabilities for approved server: {}",
                                approved_server.server_info.name
                            );
                            return self
                                .load_rtfs_capabilities_from_approved(
                                    approved_server,
                                    &files_to_load,
                                )
                                .await;
                        }
                    } else {
                        // Fallback: Check if RTFS files exist in approved directory even without queue entry
                        let server_id_normalized = server_config
                            .name
                            .to_lowercase()
                            .replace(" ", "_")
                            .replace("/", "_");
                        let approved_roots = [
                            std::path::Path::new("capabilities/servers/approved").to_path_buf(),
                            std::path::Path::new("../capabilities/servers/approved").to_path_buf(),
                        ];
                        let approved_dir = approved_roots
                            .iter()
                            .find(|p| p.join(&server_id_normalized).exists())
                            .map(|p| p.join(&server_id_normalized));

                        if let Some(approved_dir) = approved_dir {
                            let mut files_to_load = Vec::new();
                            Self::collect_rtfs_files_recursive(
                                &approved_dir,
                                "capabilities/servers/approved",
                                &mut files_to_load,
                            );

                            if !files_to_load.is_empty() {
                                ccos_eprintln!(
                                    "📂 Found {} RTFS file(s) in approved directory for: {} (skipping MCP)",
                                    files_to_load.len(),
                                    server_config.name
                                );
                                log::info!(
                                    "Found {} RTFS file(s) in approved directory for: {} - skipping MCP discovery",
                                    files_to_load.len(),
                                    server_config.name
                                );
                                // Create a minimal ApprovedDiscovery for loading
                                let synthetic_approved = crate::approval::ApprovedDiscovery {
                                    id: server_id_normalized.clone(),
                                    source: crate::approval::DiscoverySource::LocalOverride {
                                        path: approved_dir.to_string_lossy().to_string(),
                                    },
                                    server_info: crate::approval::ServerInfo {
                                        name: server_config.name.clone(),
                                        endpoint: server_config.endpoint.clone(),
                                        description: None,
                                        auth_env_var: None,
                                        capabilities_path: None,
                                        alternative_endpoints: vec![],
                                    },
                                    domain_match: vec![],
                                    risk_assessment: crate::approval::RiskAssessment {
                                        level: crate::approval::RiskLevel::Low,
                                        reasons: vec![],
                                    },
                                    requesting_goal: None,
                                    approved_at: chrono::Utc::now(),
                                    approved_by: crate::approval::ApprovalAuthority::Auto,
                                    approval_reason: Some("Pre-loaded from RTFS files".to_string()),
                                    capability_files: Some(files_to_load.clone()),
                                    version: 1,
                                    last_successful_call: None,
                                    consecutive_failures: 0,
                                    total_calls: 0,
                                    total_errors: 0,
                                };
                                return self
                                    .load_rtfs_capabilities_from_approved(
                                        &synthetic_approved,
                                        &files_to_load,
                                    )
                                    .await;
                            }
                        }

                        ccos_eprintln!(
                            "ℹ️  No approved RTFS files found for: {} (will try MCP)",
                            server_config.name
                        );
                        log::debug!("No approved server match found for: {}", server_config.name);
                    }
                }
                Err(e) => {
                    log::warn!("Failed to check approved queue: {}", e);
                    // Continue to MCP discovery
                }
            }
        } else {
            ccos_eprintln!("ℹ️  Ignoring approved files (forced discovery)");
        }

        // Check cache first if enabled
        if options.use_cache {
            if let Some(cached_tools) = self.cache.get(server_config) {
                return Ok(cached_tools);
            }
        }

        // Apply rate limiting if enabled
        if options.rate_limit.enabled {
            self.rate_limiter
                .set_server_config(&server_config.endpoint, options.rate_limit.clone());
            self.rate_limiter
                .acquire_async(&server_config.endpoint)
                .await;
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
                                self.rate_limiter
                                    .acquire_async(&server_config.endpoint)
                                    .await;
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
    fn is_retryable_error(
        &self,
        error: &RuntimeError,
        policy: &crate::mcp::rate_limiter::RetryPolicy,
    ) -> bool {
        let error_str = error.to_string().to_lowercase();

        // Check for rate limiting (429)
        if error_str.contains("429")
            || error_str.contains("too many requests")
            || error_str.contains("rate limit")
        {
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
        if error_str.contains("timeout")
            || error_str.contains("connection")
            || error_str.contains("network")
        {
            return true;
        }

        false
    }

    /// Recursively collect RTFS files from a directory
    fn collect_rtfs_files_recursive(
        dir: &std::path::Path,
        base_path: &str,
        files: &mut Vec<String>,
    ) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().map_or(false, |ext| ext == "rtfs") {
                    if let Ok(rel_path) = path.strip_prefix(base_path) {
                        files.push(rel_path.to_string_lossy().to_string());
                    }
                } else if path.is_dir() {
                    // Recursively search subdirectories
                    Self::collect_rtfs_files_recursive(&path, base_path, files);
                }
            }
        }
    }

    /// Update capability_files in approved.json (helper method)
    async fn update_approved_capability_files(
        &self,
        server_id: &str,
        files: &[String],
    ) -> RuntimeResult<()> {
        // Best-effort update - don't fail discovery if this fails
        if let Err(e) = self
            .approval_queue
            .update_approved_server_capabilities(server_id, files.to_vec())
            .await
        {
            log::warn!(
                "Failed to update capability_files for server {}: {}",
                server_id,
                e
            );
        }
        Ok(())
    }

    /// Load RTFS capability files from an approved server
    async fn load_rtfs_capabilities_from_approved(
        &self,
        approved_server: &ApprovedDiscovery,
        capability_files: &[String],
    ) -> RuntimeResult<Vec<DiscoveredMCPTool>> {
        use crate::capability_marketplace::mcp_discovery::MCPDiscoveryProvider;

        let mut discovered_tools = Vec::new();
        let mut errors = Vec::new();

        ccos_eprintln!("📂 Loading from {}", approved_server.server_info.name);

        let approved_roots = [
            std::path::Path::new("capabilities/servers/approved").to_path_buf(),
            std::path::Path::new("../capabilities/servers/approved").to_path_buf(),
        ];

        for file_path in capability_files {
            let full_path = if std::path::Path::new(file_path).is_absolute() {
                std::path::PathBuf::from(file_path)
            } else {
                approved_roots
                    .iter()
                    .find_map(|root| {
                        let candidate = root.join(file_path);
                        if candidate.exists() {
                            Some(candidate)
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| {
                        approved_roots
                            .first()
                            .cloned()
                            .unwrap_or_else(|| {
                                std::path::PathBuf::from("capabilities/servers/approved")
                            })
                            .join(file_path)
                    })
            };

            if !full_path.exists() {
                let err_msg = format!("Capability file not found: {}", full_path.display());
                ccos_eprintln!("⚠️  {}", err_msg);
                log::warn!("{}", err_msg);
                errors.push(err_msg);
                continue;
            }

            log::debug!("Loading RTFS file: {}", full_path.display());

            let parser = MCPDiscoveryProvider::new(MCPServerConfig::default()).map_err(|e| {
                RuntimeError::Generic(format!("Failed to initialize RTFS parser: {}", e))
            })?;

            match parser.load_rtfs_capabilities(full_path.to_str().unwrap()) {
                Ok(module) => {
                    let mut success_count = 0;
                    let mut failed_caps = Vec::new();

                    for (idx, cap_def) in module.capabilities.iter().enumerate() {
                        match parser.rtfs_to_capability_manifest(cap_def) {
                            Ok(manifest) => {
                                log::debug!("Converted capability: {}", manifest.name);
                                // Convert CapabilityManifest to DiscoveredMCPTool
                                // Convert TypeExpr to JSON Schema so required params are available
                                let input_schema_json = manifest
                                    .input_schema
                                    .as_ref()
                                    .and_then(|ts| ts.to_json().ok());
                                let tool = DiscoveredMCPTool {
                                    tool_name: manifest.name.clone(),
                                    description: Some(manifest.description.clone()),
                                    input_schema: manifest.input_schema.clone(),
                                    output_schema: manifest.output_schema.clone(),
                                    input_schema_json,
                                };
                                discovered_tools.push(tool);
                                success_count += 1;
                            }
                            Err(e) => {
                                let err_msg = format!(
                                    "Failed to convert RTFS capability #{} in {}: {}",
                                    idx + 1,
                                    full_path.display(),
                                    e
                                );
                                log::warn!("{}", err_msg);
                                failed_caps.push(err_msg.clone());
                                errors.push(err_msg);
                            }
                        }
                    }

                    // Compact summary line
                    if failed_caps.is_empty() {
                        ccos_eprintln!(
                            "   ✅ Loaded {} capabilities from {}",
                            success_count,
                            full_path.file_name().unwrap_or_default().to_string_lossy()
                        );
                    } else {
                        ccos_eprintln!(
                            "   ⚠️  Loaded {}/{} capabilities from {} ({} failed)",
                            success_count,
                            module.capabilities.len(),
                            full_path.file_name().unwrap_or_default().to_string_lossy(),
                            failed_caps.len()
                        );
                        for err in &failed_caps {
                            ccos_eprintln!("      ❌ {}", err);
                        }
                    }
                }
                Err(e) => {
                    let err_msg =
                        format!("Failed to load RTFS file {}: {}", full_path.display(), e);
                    ccos_eprintln!("❌ {}", err_msg);
                    log::warn!("{}", err_msg);
                    errors.push(err_msg);
                }
            }
        }

        if discovered_tools.is_empty() {
            let error_summary = if errors.is_empty() {
                "No capabilities found in files".to_string()
            } else {
                format!("Errors encountered:\n  {}", errors.join("\n  "))
            };
            return Err(RuntimeError::Generic(format!(
                "No valid capabilities found in approved files for server: {}\n{}",
                approved_server.server_info.name, error_summary
            )));
        }

        ccos_eprintln!(
            "✅ Successfully loaded {} capability(ies) from approved server: {}",
            discovered_tools.len(),
            approved_server.server_info.name
        );
        log::info!(
            "Successfully loaded {} capability(ies) from approved server: {}",
            discovered_tools.len(),
            approved_server.server_info.name
        );
        Ok(discovered_tools)
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
        // Use shared HTTP client for connection pooling
        let session_manager = if auth_headers.is_some() {
            Arc::new(MCPSessionManager::with_client(
                Arc::clone(&self.http_client),
                auth_headers.clone(),
            ))
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
            let (input_schema, input_schema_json) =
                if let Some(schema) = tool_json.get("inputSchema") {
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

        // Output schema introspection (if explicitly requested)
        // This is expensive and skipped by default (lazy loading)
        // Only run if both introspect_output_schemas is true AND lazy_output_schemas is false
        let should_introspect = options.introspect_output_schemas && !options.lazy_output_schemas;
        if should_introspect && auth_headers.is_some() {
            log::info!(
                "🔍 Introspecting output schemas for {} tools...",
                discovered_tools.len()
            );

            for tool in &mut discovered_tools {
                match self
                    .introspector
                    .introspect_output_schema(
                        tool,
                        &server_config.endpoint,
                        &server_config.name,
                        auth_headers.clone(),
                        None, // No input overrides
                    )
                    .await
                {
                    Ok((schema, _sample)) => {
                        tool.output_schema = schema;
                    }
                    Err(e) => {
                        log::warn!(
                            "Failed to introspect output schema for {}: {}",
                            tool.tool_name,
                            e
                        );
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
            .ok_or_else(|| {
                RuntimeError::Generic("Invalid MCP resources/list response".to_string())
            })?;

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
        manifest
            .metadata
            .insert("mcp_server_name".to_string(), server_config.name.clone());
        manifest.metadata.insert(
            "mcp_server_endpoint".to_string(),
            server_config.endpoint.clone(),
        );
        manifest.metadata.insert(
            "discovery_source".to_string(),
            "mcp_unified_service".to_string(),
        );

        // Automatically infer domains and categories from server name and tool name
        manifest = manifest.with_inferred_domains_and_categories(&server_config.name);

        manifest
    }

    /// Register a discovered capability in marketplace and catalog
    pub async fn register_capability(&self, manifest: &CapabilityManifest) -> RuntimeResult<()> {
        // Register or update in marketplace if available
        if let Some(ref marketplace) = self.marketplace {
            // Use update_capability to handle version comparison
            // This will register if new, or update with version tracking if existing
            match marketplace.update_capability(manifest.clone(), false).await {
                Ok(result) => {
                    // Log version comparison if updated
                    if result.updated {
                        log::debug!(
                            "Updated capability {}: {:?} (previous: {:?})",
                            manifest.id,
                            result.version_comparison,
                            result.previous_version
                        );
                    }
                }
                Err(e) => {
                    // If update fails due to breaking changes, log warning but continue
                    // The capability won't be updated, but discovery continues
                    log::warn!(
                        "Failed to update capability {}: {}. Skipping update.",
                        manifest.id,
                        e
                    );
                    // Still try to register as new if it doesn't exist
                    if let Err(reg_err) = marketplace
                        .register_capability_manifest(manifest.clone())
                        .await
                    {
                        log::warn!("Also failed to register as new: {}", reg_err);
                    }
                }
            }
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
        // If caching is enabled and no force refresh, check if we already have an export file
        if options.use_cache && !options.force_refresh {
            let export_dir = options
                .export_directory
                .as_ref()
                .map(|s| {
                    let path = std::path::PathBuf::from(s);
                    if path.is_absolute() {
                        path
                    } else {
                        get_workspace_root().join(&path)
                    }
                })
                .unwrap_or_else(|| {
                    std::env::var("CCOS_CAPABILITY_STORAGE")
                        .ok()
                        .map(|s| {
                            let path = std::path::PathBuf::from(s);
                            if path.is_absolute() {
                                path
                            } else {
                                get_workspace_root().join(&path)
                            }
                        })
                        .unwrap_or_else(|| get_workspace_root().join("capabilities/discovered"))
                });
            let server_dir = export_dir.join("mcp").join(&server_config.name);
            let module_file = server_dir.join("capabilities.rtfs");

            if module_file.exists() {
                ccos_eprintln!(
                    "ℹ️  Found existing capability export for {}: {}",
                    server_config.name,
                    module_file.display()
                );

                // Try to load manifests from this file instead of re-discovering
                if let Some(ref marketplace) = self.marketplace {
                    let path_str = module_file.to_str().unwrap_or_default();
                    let parser = MCPDiscoveryProvider::new_with_rtfs_host_factory(
                        server_config.clone(),
                        marketplace.get_rtfs_host_factory(),
                    )
                    .map_err(|e| {
                        RuntimeError::Generic(format!("Failed to initialize RTFS parser: {}", e))
                    })?;

                    if let Ok(module) = parser.load_rtfs_capabilities(path_str) {
                        let mut manifests = Vec::new();
                        for cap_def in module.capabilities {
                            if let Ok(manifest) = parser.rtfs_to_capability_manifest(&cap_def) {
                                manifests.push(manifest);
                            }
                        }

                        if !manifests.is_empty() {
                            ccos_eprintln!(
                                "   ✅ Loaded {} capabilities from export file",
                                manifests.len()
                            );

                            // Ensure they are registered in marketplace if requested
                            if options.register_in_marketplace {
                                for manifest in &manifests {
                                    self.register_capability(manifest).await?;
                                }
                            }

                            return Ok(manifests);
                        } else {
                            ccos_eprintln!("   ⚠️  Loaded 0 capabilities from export file");
                        }
                    } else if let Err(e) = parser.load_rtfs_capabilities(path_str) {
                        ccos_eprintln!(
                            "   ⚠️  Failed to load RTFS capabilities from export file: {}",
                            e
                        );
                    }
                } else {
                    ccos_eprintln!("   ⚠️  Marketplace not available for loading export file");
                }
            }
        }

        // Check if this server is approved and has RTFS files - if so, skip export
        let is_approved_with_files = {
            if let Ok(approved_requests) = self.approval_queue.list_approved_servers().await {
                let approved: Vec<_> = approved_requests
                    .iter()
                    .filter_map(|r| r.to_approved_discovery())
                    .collect();

                let server_name_normalized = server_config
                    .name
                    .replace("/", "_")
                    .replace(" ", "_")
                    .to_lowercase();

                if let Some(approved_server) = approved.iter().find(|s| {
                    let approved_name_normalized = s
                        .server_info
                        .name
                        .replace("/", "_")
                        .replace(" ", "_")
                        .to_lowercase();
                    server_name_normalized == approved_name_normalized
                        || s.server_info.name == server_config.name
                        || s.server_info.endpoint == server_config.endpoint
                }) {
                    // Check if it has capability files
                    if let Some(ref files) = approved_server.capability_files {
                        !files.is_empty()
                    } else {
                        // Check if directory exists
                        let server_id = approved_server
                            .server_info
                            .name
                            .replace("/", "_")
                            .replace(":", "_");
                        let workspace_root = get_workspace_root();
                        let approved_dir = workspace_root
                            .join("capabilities/servers/approved")
                            .join(&server_id);
                        approved_dir.exists()
                    }
                } else {
                    // Fallback: check if directory exists even without queue entry
                    let server_id_normalized = server_config
                        .name
                        .to_lowercase()
                        .replace(" ", "_")
                        .replace("/", "_");
                    let approved_roots = [
                        std::path::Path::new("capabilities/servers/approved").to_path_buf(),
                        std::path::Path::new("../capabilities/servers/approved").to_path_buf(),
                    ];
                    approved_roots.iter().any(|root| {
                        let dir = root.join(&server_id_normalized);
                        if dir.exists() {
                            // Check if there are RTFS files in the directory
                            std::fs::read_dir(&dir)
                                .ok()
                                .map(|entries| {
                                    entries.flatten().any(|e| {
                                        e.path().extension().map_or(false, |ext| ext == "rtfs")
                                    })
                                })
                                .unwrap_or(false)
                        } else {
                            false
                        }
                    })
                }
            } else {
                false
            }
        };

        // Discover tools
        let tools = self.discover_tools(server_config, options).await?;

        // Convert to manifests
        let mut manifests = Vec::new();
        for tool in &tools {
            let manifest = self.tool_to_manifest(tool, server_config);
            manifests.push(manifest);
        }

        // Skip export if loading from approved files (they're already saved)
        if is_approved_with_files {
            ccos_eprintln!("ℹ️  Capabilities loaded from approved RTFS files - skipping export");
            // Still register in marketplace if requested (but don't export)
            if options.register_in_marketplace {
                for manifest in &manifests {
                    self.register_capability(manifest).await?;
                }
            }
            return Ok(manifests);
        }

        // Check for existing capabilities and warn user
        let mut existing_capabilities = Vec::new();
        if let Some(ref marketplace) = self.marketplace {
            for manifest in &manifests {
                if marketplace.has_capability(&manifest.id).await {
                    existing_capabilities.push(manifest.id.clone());
                }
            }
        }

        // Also check if RTFS export file already exists (only if not loading from approved files)
        let export_file_exists = if is_approved_with_files {
            false // Don't check - we're loading from approved files, skip export
        } else {
            let export_dir = options
                .export_directory
                .as_ref()
                .map(|s| {
                    let path = std::path::PathBuf::from(s);
                    if path.is_absolute() {
                        path
                    } else {
                        get_workspace_root().join(&path)
                    }
                })
                .unwrap_or_else(|| {
                    std::env::var("CCOS_CAPABILITY_STORAGE")
                        .ok()
                        .map(|s| {
                            let path = std::path::PathBuf::from(s);
                            if path.is_absolute() {
                                path
                            } else {
                                get_workspace_root().join(&path)
                            }
                        })
                        .unwrap_or_else(|| get_workspace_root().join("capabilities/discovered"))
                });
            let server_dir = export_dir.join("mcp").join(&server_config.name);
            let module_file = server_dir.join("capabilities.rtfs");
            module_file.exists()
        };

        // Warn if capabilities already exist
        if !existing_capabilities.is_empty() || export_file_exists {
            if options.non_interactive {
                ccos_println!(
                    "🔍 Discovery found existing capabilities, proceeding in non-interactive mode."
                );
            } else {
                ccos_eprintln!();
                ccos_eprintln!("⚠️  Warning: Some capabilities were already discovered:");
                if !existing_capabilities.is_empty() {
                    ccos_eprintln!(
                        "   • {} capability(ies) already registered in marketplace:",
                        existing_capabilities.len()
                    );
                    for cap_id in &existing_capabilities {
                        ccos_eprintln!("     - {}", cap_id);
                    }
                }
                if export_file_exists {
                    ccos_eprintln!("   • RTFS export file already exists for this server");
                }
                ccos_eprintln!();

                // Ask for confirmation before proceeding
                print!("Continue and overwrite? (y/n): ");
                use std::io::{self, Write};
                io::stdout()
                    .flush()
                    .map_err(|e| RuntimeError::Generic(format!("Failed to flush stdout: {}", e)))?;

                let mut confirm = String::new();
                io::stdin()
                    .read_line(&mut confirm)
                    .map_err(|e| RuntimeError::Generic(format!("Failed to read input: {}", e)))?;
                let confirm = confirm.trim().to_lowercase();

                if confirm != "y" && confirm != "yes" {
                    ccos_eprintln!("   Skipping registration and export.");
                    return Ok(manifests); // Return manifests but don't register/export
                }
            }
        }

        // Register if requested
        if options.register_in_marketplace {
            for manifest in &manifests {
                self.register_capability(manifest).await?;
            }
        }

        // Export to RTFS if requested (only if user confirmed)
        if options.export_to_rtfs {
            self.export_server_capabilities_to_rtfs(server_config, &manifests, options)
                .await?;
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
        use std::fs;
        use std::path::PathBuf;

        // Determine export directory
        // Use absolute path or relative to current working directory
        let export_dir = options
            .export_directory
            .as_ref()
            .map(|s| {
                let path = PathBuf::from(s);
                if path.is_absolute() {
                    path
                } else {
                    // Make relative to workspace root
                    get_workspace_root().join(&path)
                }
            })
            .unwrap_or_else(|| {
                // Default: use environment variable or fallback to capabilities/discovered in current dir
                std::env::var("CCOS_CAPABILITY_STORAGE")
                    .map(|s| {
                        let path = PathBuf::from(s);
                        if path.is_absolute() {
                            path
                        } else {
                            get_workspace_root().join(&path)
                        }
                    })
                    .unwrap_or_else(|_| {
                        // Default to configured discovered capabilities path
                        crate::utils::fs::get_configured_discovered_path()
                    })
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
        rtfs_content.push_str(&format!(
            ";; Server: {} ({})\n\n",
            server_config.name, server_config.endpoint
        ));
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
        ccos_println!(
            "  💾 Exported {} capabilities to {}",
            manifests.len(),
            module_file.display()
        );

        Ok(())
    }

    /// Get server config for a domain hint
    pub async fn get_server_for_domain(&self, domain: &DomainHint) -> Option<MCPServerConfig> {
        // Use to_domain_string() for config-driven domain handling
        let hint = domain.to_domain_string();

        let configs = self.list_known_servers().await;
        for config in configs {
            if config.name.contains(&hint) || hint.contains(&config.name) {
                return Some(config);
            }
        }

        None
    }

    /// List all known servers (from config and approval queue)
    pub async fn list_known_servers(&self) -> Vec<MCPServerConfig> {
        let mut servers = self.config_discovery.get_all_server_configs();
        ccos_eprintln!("📋 Config discovery found {} server(s)", servers.len());

        // Add approved servers from queue
        match self.approval_queue.list_approved_servers().await {
            Ok(approved_requests) => {
                let approved: Vec<_> = approved_requests
                    .iter()
                    .filter_map(|r| r.to_approved_discovery())
                    .collect();
                ccos_eprintln!(
                    "📋 Approval queue found {} approved server(s)",
                    approved.len()
                );
                for server in approved {
                    // Check for duplicates (by name or endpoint)
                    let is_duplicate = servers.iter().any(|s| {
                        s.name == server.server_info.name
                            || s.endpoint == server.server_info.endpoint
                    });

                    if !is_duplicate {
                        ccos_eprintln!(
                            "✅ Adding approved server: {} ({})",
                            server.server_info.name,
                            server.server_info.endpoint
                        );
                        servers.push(MCPServerConfig {
                            name: server.server_info.name.clone(),
                            endpoint: server.server_info.endpoint.clone(),
                            auth_token: None, // Will fallback to env vars if needed
                            timeout_seconds: 30, // Default
                            protocol_version: "2024-11-05".to_string(), // Default
                        });
                    } else {
                        ccos_eprintln!(
                            "⏭️ Skipping duplicate server: {} ({})",
                            server.server_info.name,
                            server.server_info.endpoint
                        );
                    }
                }
            }
            Err(e) => {
                ccos_eprintln!("❌ Failed to list approved servers: {}", e);
            }
        }

        ccos_eprintln!("📋 Total known servers: {}", servers.len());
        servers
    }

    // ================================
    // Registry Integration Methods
    // ================================

    /// Search the MCP registry for servers that might provide a capability
    ///
    /// This method searches the official MCP registry for servers matching
    /// the given capability name or query. Results are cached to avoid
    /// repeated lookups.
    pub async fn search_registry_for_capability(
        &self,
        capability_query: &str,
        use_cache: bool,
    ) -> RuntimeResult<Vec<crate::mcp::registry::McpServer>> {
        // Check cache first
        if use_cache {
            if let Some(cached) = self.cache.get_registry_search(capability_query) {
                log::debug!("Registry search cache hit for '{}'", capability_query);
                return Ok(cached);
            }
        }

        // Search the registry
        log::info!("🔍 Searching MCP registry for '{}'...", capability_query);
        let servers = self
            .registry_client
            .search_servers(capability_query)
            .await?;

        // Cache results
        if use_cache {
            self.cache
                .store_registry_search(capability_query, servers.clone());
        }

        log::info!(
            "📦 Found {} servers matching '{}'",
            servers.len(),
            capability_query
        );
        Ok(servers)
    }

    /// Find servers that can provide a specific capability
    ///
    /// This method:
    /// 1. First checks local/configured servers
    /// 2. Falls back to registry search if no local servers found
    /// 3. Converts registry results to MCPServerConfig for discovery
    pub async fn find_servers_for_capability(
        &self,
        capability_name: &str,
        options: &DiscoveryOptions,
    ) -> RuntimeResult<Vec<MCPServerConfig>> {
        let mut found_servers = Vec::new();

        // First, check if any known local servers might have this capability
        let local_servers = self.list_known_servers().await;
        for server in local_servers {
            // Check if server name hints at having this capability
            let server_name_lower = server.name.to_lowercase();
            let capability_lower = capability_name.to_lowercase();

            // Simple heuristic: server name contains capability keywords
            if server_name_lower.contains(&capability_lower)
                || capability_lower.contains(&server_name_lower)
            {
                found_servers.push(server);
            }
        }

        // If we found local servers, return them first
        if !found_servers.is_empty() {
            log::debug!(
                "Found {} local servers for '{}'",
                found_servers.len(),
                capability_name
            );
            return Ok(found_servers);
        }

        // Fall back to registry search
        let registry_servers = self
            .search_registry_for_capability(capability_name, options.use_cache)
            .await?;

        // Convert registry servers to MCPServerConfig
        for registry_server in registry_servers {
            if let Some(config) = self.registry_server_to_config(&registry_server) {
                found_servers.push(config);
            }
        }

        Ok(found_servers)
    }

    /// Convert a registry McpServer to MCPServerConfig
    fn registry_server_to_config(
        &self,
        server: &crate::mcp::registry::McpServer,
    ) -> Option<MCPServerConfig> {
        // Try to get a usable endpoint from remotes
        let endpoint = server.remotes.as_ref().and_then(|remotes| {
            crate::mcp::registry::MCPRegistryClient::select_best_remote_url(remotes)
        })?;

        Some(MCPServerConfig {
            name: server.name.clone(),
            endpoint,
            auth_token: None, // Will need to be configured separately
            timeout_seconds: 30,
            protocol_version: "2024-11-05".to_string(),
        })
    }

    /// Discover tools from registry-found servers
    ///
    /// This is a high-level method that:
    /// 1. Searches the registry for servers matching the query
    /// 2. Attempts to discover tools from each found server in parallel (with concurrency control)
    /// 3. Returns all discovered tools across all servers
    ///
    /// Parallel discovery is controlled by `options.max_parallel_discoveries` to
    /// prevent overwhelming servers and getting rate-limited or banned.
    pub async fn discover_from_registry(
        &self,
        capability_query: &str,
        options: &DiscoveryOptions,
    ) -> RuntimeResult<Vec<(MCPServerConfig, Vec<DiscoveredMCPTool>)>> {
        let servers = self
            .find_servers_for_capability(capability_query, options)
            .await?;

        if servers.is_empty() {
            return Ok(Vec::new());
        }

        // Use parallel discovery with concurrency control
        let max_parallel = options.max_parallel_discoveries;
        log::info!(
            "🔍 Discovering from {} server(s) with max parallelism: {}",
            servers.len(),
            max_parallel
        );

        // Create a semaphore to limit concurrent discoveries
        let semaphore = Arc::new(tokio::sync::Semaphore::new(max_parallel));
        let mut handles = Vec::new();

        for server_config in servers {
            let permit = Arc::clone(&semaphore);
            let service = self.clone_for_parallel();
            let options_clone = options.clone();
            let server_config_clone = server_config.clone();

            let handle = tokio::spawn(async move {
                // Acquire permit (blocks if max_parallel discoveries are in progress)
                let _permit = permit.acquire().await.unwrap();

                match service
                    .discover_tools(&server_config_clone, &options_clone)
                    .await
                {
                    Ok(tools) => {
                        if !tools.is_empty() {
                            log::info!(
                                "✅ Discovered {} tools from '{}'",
                                tools.len(),
                                server_config_clone.name
                            );
                            Ok((server_config_clone, tools))
                        } else {
                            Err(RuntimeError::Generic(format!(
                                "No tools found from '{}'",
                                server_config_clone.name
                            )))
                        }
                    }
                    Err(e) => {
                        log::warn!(
                            "⚠️ Failed to discover from '{}': {}",
                            server_config_clone.name,
                            e
                        );
                        Err(e)
                    }
                }
            });

            handles.push(handle);
        }

        // Wait for all discoveries to complete and collect results
        let mut results = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(Ok((server_config, tools))) => {
                    results.push((server_config, tools));
                }
                Ok(Err(e)) => {
                    // Already logged, continue with other servers
                    log::debug!("Discovery task failed: {}", e);
                }
                Err(e) => {
                    log::warn!("Discovery task panicked: {}", e);
                }
            }
        }

        log::info!(
            "✅ Parallel discovery complete: {} server(s) succeeded, {} total tools",
            results.len(),
            results.iter().map(|(_, tools)| tools.len()).sum::<usize>()
        );

        Ok(results)
    }

    /// Clone the service for parallel execution
    /// This creates a lightweight clone that shares the same underlying resources
    fn clone_for_parallel(&self) -> Self {
        Self {
            http_client: Arc::clone(&self.http_client),
            session_manager: Arc::clone(&self.session_manager),
            registry_client: MCPRegistryClient::new(), // Registry client is stateless
            config_discovery: LocalConfigMcpDiscovery::new(), // Config discovery is stateless
            approval_queue: self.approval_queue.clone(),
            introspector: MCPIntrospector::new(), // Introspector is stateless
            cache: Arc::clone(&self.cache),       // Share cache
            rate_limiter: Arc::clone(&self.rate_limiter), // Share rate limiter
            marketplace: self.marketplace.as_ref().map(Arc::clone),
            catalog: self.catalog.as_ref().map(Arc::clone),
        }
    }

    /// Get the registry client (for direct access when needed)
    pub fn registry_client(&self) -> &MCPRegistryClient {
        &self.registry_client
    }

    // ================================
    // Cache Warming Methods
    // ================================

    /// Warm the cache for a list of servers (on-demand)
    ///
    /// This method proactively discovers tools from the specified servers
    /// and populates the cache. Useful for pre-loading frequently used servers.
    ///
    /// Discovery is done in parallel with concurrency control to avoid
    /// overwhelming servers.
    ///
    /// # Arguments
    /// * `servers` - List of server configurations to warm
    /// * `options` - Discovery options (cache will be enabled automatically)
    ///
    /// # Returns
    /// Statistics about the warming operation (successful/failed servers)
    pub async fn warm_cache_for_servers(
        &self,
        servers: &[MCPServerConfig],
        options: &DiscoveryOptions,
    ) -> RuntimeResult<CacheWarmingStats> {
        if servers.is_empty() {
            return Ok(CacheWarmingStats {
                total_servers: 0,
                successful: 0,
                failed: 0,
                cached_tools: 0,
            });
        }

        log::info!("🔥 Warming cache for {} server(s)...", servers.len());

        // Create options with cache enabled
        let mut warm_options = options.clone();
        warm_options.use_cache = true; // Ensure cache is enabled
        warm_options.introspect_output_schemas = false; // Skip expensive introspection during warming
        warm_options.lazy_output_schemas = true; // Use lazy loading for warming

        // Use parallel discovery with concurrency control
        let max_parallel = warm_options.max_parallel_discoveries;
        let semaphore = Arc::new(tokio::sync::Semaphore::new(max_parallel));
        let mut handles = Vec::new();

        for server_config in servers {
            let permit = Arc::clone(&semaphore);
            let service = self.clone_for_parallel();
            let options_clone = warm_options.clone();
            let server_config_clone = server_config.clone();

            let handle = tokio::spawn(async move {
                let _permit = permit.acquire().await.unwrap();

                match service
                    .discover_tools(&server_config_clone, &options_clone)
                    .await
                {
                    Ok(tools) => Ok((server_config_clone.name.clone(), tools.len())),
                    Err(e) => {
                        log::debug!(
                            "Cache warming failed for '{}': {}",
                            server_config_clone.name,
                            e
                        );
                        Err((server_config_clone.name.clone(), e))
                    }
                }
            });

            handles.push(handle);
        }

        // Collect results
        let mut stats = CacheWarmingStats {
            total_servers: servers.len(),
            successful: 0,
            failed: 0,
            cached_tools: 0,
        };

        for handle in handles {
            match handle.await {
                Ok(Ok((server_name, tool_count))) => {
                    stats.successful += 1;
                    stats.cached_tools += tool_count;
                    log::debug!(
                        "✅ Cache warmed for '{}': {} tools",
                        server_name,
                        tool_count
                    );
                }
                Ok(Err((server_name, _))) => {
                    stats.failed += 1;
                    log::debug!("⚠️ Cache warming failed for '{}'", server_name);
                }
                Err(e) => {
                    stats.failed += 1;
                    log::warn!("Cache warming task panicked: {}", e);
                }
            }
        }

        log::info!(
            "🔥 Cache warming complete: {}/{} servers successful, {} tools cached",
            stats.successful,
            stats.total_servers,
            stats.cached_tools
        );

        Ok(stats)
    }

    /// Warm cache for all known configured servers (startup warming)
    ///
    /// This method discovers all servers from the local configuration
    /// and warms the cache. Useful for startup initialization when you
    /// want to pre-load all configured servers.
    ///
    /// # Arguments
    /// * `options` - Discovery options (cache will be enabled automatically)
    ///
    /// # Returns
    /// Statistics about the warming operation
    pub async fn warm_cache_for_all_configured_servers(
        &self,
        options: &DiscoveryOptions,
    ) -> RuntimeResult<CacheWarmingStats> {
        let servers = self.list_known_servers().await;
        log::info!(
            "🔥 Warming cache for {} configured server(s)...",
            servers.len()
        );
        self.warm_cache_for_servers(&servers, options).await
    }
}

/// Statistics from cache warming operations
#[derive(Debug, Clone)]
pub struct CacheWarmingStats {
    /// Total number of servers attempted
    pub total_servers: usize,
    /// Number of servers successfully warmed
    pub successful: usize,
    /// Number of servers that failed
    pub failed: usize,
    /// Total number of tools cached
    pub cached_tools: usize,
}

impl Default for MCPDiscoveryService {
    fn default() -> Self {
        Self::new()
    }
}
