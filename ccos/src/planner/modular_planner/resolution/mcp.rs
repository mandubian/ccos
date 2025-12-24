//! MCP resolution strategy
//!
//! Discovers and resolves capabilities from MCP servers.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use super::{ResolutionContext, ResolutionError, ResolutionStrategy, ResolvedCapability};
use crate::planner::modular_planner::decomposition::grounded_llm::{
    cosine_similarity, EmbeddingProvider,
};
use crate::planner::modular_planner::types::{
    ApiAction, DomainHint, IntentType, SubIntent, ToolSummary,
};

// Imports for RuntimeMcpDiscovery
use crate::capability_marketplace::CapabilityMarketplace;
use crate::mcp::registry::MCPRegistryClient;
use crate::mcp::types::DiscoveredMCPTool;
use crate::synthesis::mcp_introspector::MCPIntrospector;

/// MCP server info
#[derive(Debug, Clone)]
pub struct McpServerInfo {
    pub name: String,
    pub url: String,
    pub namespace: String,
}

/// MCP tool info discovered from server
#[derive(Debug, Clone)]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: Option<serde_json::Value>,
    pub server: McpServerInfo,
}

/// Trait for MCP discovery operations
#[async_trait(?Send)]
pub trait McpDiscovery: Send + Sync {
    /// Get MCP server for a domain hint
    async fn get_server_for_domain(&self, domain: &DomainHint) -> Option<McpServerInfo>;

    /// Discover tools from an MCP server
    async fn discover_tools(&self, server: &McpServerInfo) -> Result<Vec<McpToolInfo>, String>;

    /// Register a discovered tool as a capability
    async fn register_tool(&self, tool: &McpToolInfo) -> Result<String, String>;

    /// List all known MCP servers
    async fn list_known_servers(&self) -> Vec<McpServerInfo>;
}
use crate::catalog::CatalogService;

use crate::capability_marketplace::config_mcp_discovery::LocalConfigMcpDiscovery;

/// Runtime implementation of McpDiscovery using real MCP servers
///
/// This implementation always uses the unified `MCPDiscoveryService` for all
/// discovery operations, providing consistent behavior with caching, rate limiting, etc.
pub struct RuntimeMcpDiscovery {
    registry_client: MCPRegistryClient,
    marketplace: Arc<CapabilityMarketplace>,
    /// Optional catalog for indexing discovered tools
    catalog: Option<Arc<CatalogService>>,
    /// Local config discovery agent for resolving servers
    config_discovery: LocalConfigMcpDiscovery,
    /// Unified discovery service - always used
    discovery_service: Arc<crate::mcp::core::MCPDiscoveryService>,
}

impl RuntimeMcpDiscovery {
    pub fn new(marketplace: Arc<CapabilityMarketplace>) -> Self {
        Self {
            registry_client: MCPRegistryClient::new(),
            marketplace,
            catalog: None,
            config_discovery: LocalConfigMcpDiscovery::new(),
            discovery_service: Arc::new(crate::mcp::core::MCPDiscoveryService::new()),
        }
    }

    /// Create with a custom discovery service
    pub fn with_discovery_service(
        marketplace: Arc<CapabilityMarketplace>,
        discovery_service: Arc<crate::mcp::core::MCPDiscoveryService>,
    ) -> Self {
        Self {
            registry_client: MCPRegistryClient::new(),
            marketplace,
            catalog: None,
            config_discovery: LocalConfigMcpDiscovery::new(),
            discovery_service,
        }
    }

    /// Add catalog for indexing discovered tools
    pub fn with_catalog(mut self, catalog: Arc<CatalogService>) -> Self {
        self.catalog = Some(catalog);
        self
    }
}

#[async_trait(?Send)]
impl McpDiscovery for RuntimeMcpDiscovery {
    async fn get_server_for_domain(&self, domain: &DomainHint) -> Option<McpServerInfo> {
        if let Some(config) = self.discovery_service.get_server_for_domain(domain) {
            return Some(McpServerInfo {
                name: config.name.clone(),
                url: config.endpoint.clone(),
                namespace: config.name.clone(),
            });
        }
        None
    }

    async fn discover_tools(&self, server: &McpServerInfo) -> Result<Vec<McpToolInfo>, String> {
        // Create config for provider
        let config = crate::capability_marketplace::mcp_discovery::MCPServerConfig {
            name: server.name.clone(),
            endpoint: server.url.clone(),
            auth_token: None, // Auth token is handled by the shared session manager if configured
            timeout_seconds: 30,
            protocol_version: "2024-11-05".to_string(),
        };

        let options = crate::mcp::types::DiscoveryOptions {
            introspect_output_schemas: false,
            use_cache: true,
            register_in_marketplace: false,
            export_to_rtfs: false,
            export_directory: None,
            auth_headers: None,
            ..Default::default()
        };

        match self
            .discovery_service
            .discover_tools(&config, &options)
            .await
        {
            Ok(discovered_tools) => {
                let tools = discovered_tools
                    .into_iter()
                    .map(|t| McpToolInfo {
                        name: t.tool_name.clone(),
                        description: t.description.unwrap_or_default(),
                        input_schema: t.input_schema_json.clone(),
                        server: server.clone(),
                    })
                    .collect();
                Ok(tools)
            }
            Err(e) => Err(format!("Discovery service failed: {}", e)),
        }
    }

    async fn register_tool(&self, tool: &McpToolInfo) -> Result<String, String> {
        let config = crate::capability_marketplace::mcp_discovery::MCPServerConfig {
            name: tool.server.name.clone(),
            endpoint: tool.server.url.clone(),
            auth_token: None,
            timeout_seconds: 30,
            protocol_version: "2024-11-05".to_string(),
        };

        // Reconstruct DiscoveredMCPTool from McpToolInfo
        let discovered_tool = DiscoveredMCPTool {
            tool_name: tool.name.clone(),
            description: Some(tool.description.clone()),
            input_schema: tool
                .input_schema
                .as_ref()
                .and_then(|s| MCPIntrospector::type_expr_from_json_schema(s).ok()),
            output_schema: None,
            input_schema_json: tool.input_schema.clone(),
        };

        // Use discovery service to create manifest
        let manifest = self
            .discovery_service
            .tool_to_manifest(&discovered_tool, &config);

        // Register using discovery service (handles marketplace + catalog)
        self.discovery_service
            .register_capability(&manifest)
            .await
            .map_err(|e| format!("Failed to register capability: {}", e))?;

        Ok(manifest.id)
    }

    async fn list_known_servers(&self) -> Vec<McpServerInfo> {
        self.discovery_service
            .list_known_servers()
            .into_iter()
            .map(|config| McpServerInfo {
                name: config.name.clone(),
                url: config.endpoint.clone(),
                namespace: config.name.clone(),
            })
            .collect()
    }
}

/// Cached tool info for file persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: Option<serde_json::Value>,
    pub server_name: String,
    pub server_url: String,
    pub server_namespace: String,
    pub cached_at: u64,
}

impl From<&McpToolInfo> for CachedToolInfo {
    fn from(tool: &McpToolInfo) -> Self {
        Self {
            name: tool.name.clone(),
            description: tool.description.clone(),
            input_schema: tool.input_schema.clone(),
            server_name: tool.server.name.clone(),
            server_url: tool.server.url.clone(),
            server_namespace: tool.server.namespace.clone(),
            cached_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }
}

impl CachedToolInfo {
    pub fn to_mcp_tool_info(&self) -> McpToolInfo {
        McpToolInfo {
            name: self.name.clone(),
            description: self.description.clone(),
            input_schema: self.input_schema.clone(),
            server: McpServerInfo {
                name: self.server_name.clone(),
                url: self.server_url.clone(),
                namespace: self.server_namespace.clone(),
            },
        }
    }

    pub fn to_tool_summary(&self) -> ToolSummary {
        // Infer domain from server name using domain_config
        let domain =
            crate::planner::modular_planner::domain_config::infer_domain(&self.server_name)
                .map(DomainHint::Custom)
                .unwrap_or(DomainHint::Generic);

        // Infer action from tool name
        let action = if self.name.starts_with("list_")
            || self.name.starts_with("get_all")
            || self.name.contains("_list")
        {
            ApiAction::List
        } else if self.name.starts_with("get_") || self.name.starts_with("read_") {
            ApiAction::Get
        } else if self.name.starts_with("create_") || self.name.starts_with("add_") {
            ApiAction::Create
        } else if self.name.starts_with("update_") || self.name.starts_with("edit_") {
            ApiAction::Update
        } else if self.name.starts_with("delete_") || self.name.starts_with("remove_") {
            ApiAction::Delete
        } else if self.name.starts_with("search_") || self.name.starts_with("find_") {
            ApiAction::Search
        } else {
            ApiAction::Other(self.name.clone())
        };

        // Prefer fully-qualified capability id (manifest-style) for deduplication, keep name as short display
        // Manifest ids for MCP tools are of the form: mcp.{server_name}.{tool_name}
        let fq_id: String = format!("mcp.{}.{}", self.server_name, self.name);

        ToolSummary {
            id: fq_id,
            name: self.name.clone(),
            description: self.description.clone(),
            domain,
            action,
            input_schema: self.input_schema.clone(),
        }
    }
}

/// MCP resolution strategy.
///
/// Discovers capabilities from MCP servers based on domain hints.
pub struct McpResolution {
    discovery: Arc<dyn McpDiscovery>,
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    /// Minimum score to accept match
    min_score: f64,
    /// Cache of discovered tools (in-memory)
    tool_cache: std::sync::Mutex<HashMap<String, Vec<McpToolInfo>>>,
    /// Path to file cache directory
    cache_dir: Option<PathBuf>,
    /// Whether to skip loading from cache
    no_cache: bool,
}

impl McpResolution {
    pub fn new(discovery: Arc<dyn McpDiscovery>) -> Self {
        Self {
            discovery,
            embedding_provider: None,
            min_score: 0.3,
            tool_cache: std::sync::Mutex::new(HashMap::new()),
            cache_dir: None,
            no_cache: false,
        }
    }

    pub fn with_embedding(mut self, provider: Arc<dyn EmbeddingProvider>) -> Self {
        self.embedding_provider = Some(provider);
        self
    }

    /// Enable file-based caching to specified directory
    pub fn with_cache_dir(mut self, dir: PathBuf) -> Self {
        self.cache_dir = Some(dir);
        self
    }

    /// Disable loading from cache (will still save)
    pub fn with_no_cache(mut self, no_cache: bool) -> Self {
        self.no_cache = no_cache;
        self
    }

    /// Load tools from file cache
    fn load_from_file_cache(&self, server_name: &str) -> Option<Vec<McpToolInfo>> {
        if self.no_cache {
            return None;
        }

        let cache_dir = self.cache_dir.as_ref()?;
        let cache_file = cache_dir.join(format!("{}_tools.json", server_name));

        if !cache_file.exists() {
            return None;
        }

        let content = std::fs::read_to_string(&cache_file).ok()?;
        let cached: Vec<CachedToolInfo> = serde_json::from_str(&content).ok()?;

        // Check cache age (24 hours max)
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if let Some(first) = cached.first() {
            if now - first.cached_at > 86400 {
                ccos_println!("   ⏰ Cache for {} is stale, will refresh", server_name);
                return None;
            }
        }

        ccos_println!(
            "   📂 Loaded {} tools from cache for {}",
            cached.len(),
            server_name
        );
        Some(cached.into_iter().map(|c| c.to_mcp_tool_info()).collect())
    }

    /// Save tools to file cache
    fn save_to_file_cache(&self, server_name: &str, tools: &[McpToolInfo]) {
        let cache_dir = match &self.cache_dir {
            Some(d) => d,
            None => return,
        };

        // Create directory if needed
        if let Err(e) = std::fs::create_dir_all(cache_dir) {
            log::warn!("Failed to create cache dir: {}", e);
            return;
        }

        let cache_file = cache_dir.join(format!("{}_tools.json", server_name));
        let cached: Vec<CachedToolInfo> = tools.iter().map(CachedToolInfo::from).collect();

        match serde_json::to_string_pretty(&cached) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&cache_file, json) {
                    log::warn!("Failed to write cache: {}", e);
                } else {
                    ccos_println!(
                        "   💾 Saved {} tools to cache: {}",
                        tools.len(),
                        cache_file.display()
                    );
                }
            }
            Err(e) => log::warn!("Failed to serialize cache: {}", e),
        }
    }

    /// Get or discover tools from cache
    async fn get_tools(&self, server: &McpServerInfo) -> Result<Vec<McpToolInfo>, ResolutionError> {
        // Check in-memory cache first
        {
            let cache = self.tool_cache.lock().unwrap();
            if let Some(tools) = cache.get(&server.url) {
                return Ok(tools.clone());
            }
        }

        // Check file cache
        if let Some(tools) = self.load_from_file_cache(&server.name) {
            // Store in memory cache
            let mut cache = self.tool_cache.lock().unwrap();
            cache.insert(server.url.clone(), tools.clone());
            return Ok(tools);
        }

        // Discover tools from server
        ccos_println!("   🔍 Discovering tools from MCP server: {}", server.name);
        let tools = self
            .discovery
            .discover_tools(server)
            .await
            .map_err(|e| ResolutionError::McpError(e))?;

        ccos_println!(
            "   ✅ Discovered {} tools from {}",
            tools.len(),
            server.name
        );

        // Save to file cache
        self.save_to_file_cache(&server.name, &tools);

        // Cache in memory
        {
            let mut cache = self.tool_cache.lock().unwrap();
            cache.insert(server.url.clone(), tools.clone());
        }

        Ok(tools)
    }

    /// Score a tool against an intent
    async fn score_tool(&self, intent: &SubIntent, tool: &McpToolInfo) -> f64 {
        let query = format!("{}", intent.description);

        // Use embeddings if available
        if let Some(ref emb) = self.embedding_provider {
            let query_emb = match emb.embed(&query).await {
                Ok(e) => e,
                Err(_) => return self.keyword_score(intent, tool),
            };

            let tool_text = format!("{} {}", tool.name, tool.description);
            let tool_emb = match emb.embed(&tool_text).await {
                Ok(e) => e,
                Err(_) => return self.keyword_score(intent, tool),
            };

            return cosine_similarity(&query_emb, &tool_emb);
        }

        self.keyword_score(intent, tool)
    }

    /// Keyword-based scoring
    fn keyword_score(&self, intent: &SubIntent, tool: &McpToolInfo) -> f64 {
        let tool_lower = format!("{} {}", tool.name, tool.description).to_lowercase();
        let desc_lower = intent.description.to_lowercase();

        let mut score = 0.0;

        // Word overlap
        let words: Vec<&str> = desc_lower.split_whitespace().collect();
        let mut matches = 0;
        for word in &words {
            if word.len() > 2 && tool_lower.contains(word) {
                matches += 1;
            }
        }
        if !words.is_empty() {
            score = matches as f64 / words.len() as f64;
        }

        // Action keyword boost
        if let IntentType::ApiCall { ref action } = intent.intent_type {
            for kw in action.matching_keywords() {
                if tool.name.to_lowercase().starts_with(kw) {
                    score += 0.3;
                    break;
                }
            }
        }

        // Resource type matching (issues, pull_requests, etc.)
        if let Some(resource) = intent.extracted_params.get("resource") {
            if tool.name.to_lowercase().contains(&resource.to_lowercase()) {
                score += 0.25;
            }
        }

        score.min(1.0)
    }

    /// Extract arguments from intent to pass to tool
    fn extract_arguments(
        &self,
        intent: &SubIntent,
        _tool: &McpToolInfo,
    ) -> HashMap<String, String> {
        let mut args = HashMap::new();

        // Copy all non-internal params
        for (key, value) in &intent.extracted_params {
            if !key.starts_with('_') {
                args.insert(key.clone(), value.clone());
            }
        }

        // If tool has input schema, we could validate/transform args here
        // For now, pass through as-is

        args
    }
}

#[async_trait(?Send)]
impl ResolutionStrategy for McpResolution {
    fn name(&self) -> &str {
        "mcp"
    }

    fn can_handle(&self, intent: &SubIntent) -> bool {
        // Can handle API calls if domain hint suggests MCP
        if !matches!(intent.intent_type, IntentType::ApiCall { .. }) {
            return false;
        }

        // Need a domain hint that maps to MCP servers
        if let Some(ref domain) = intent.domain_hint {
            !domain.likely_mcp_servers().is_empty()
        } else {
            false
        }
    }

    async fn resolve(
        &self,
        intent: &SubIntent,
        _context: &ResolutionContext,
    ) -> Result<ResolvedCapability, ResolutionError> {
        // Get domain hint
        let domain = intent
            .domain_hint
            .as_ref()
            .ok_or_else(|| ResolutionError::NotFound("No domain hint".to_string()))?;

        // Get MCP server for domain
        let server = self
            .discovery
            .get_server_for_domain(domain)
            .await
            .ok_or_else(|| {
                ResolutionError::NotFound(format!("No MCP server for domain {:?}", domain))
            })?;

        // Discover tools
        let tools = self.get_tools(&server).await?;

        if tools.is_empty() {
            return Err(ResolutionError::NotFound(format!(
                "No tools found on MCP server: {}",
                server.name
            )));
        }

        // Check if LLM already suggested a tool (grounded decomposition)
        if let Some(suggested_tool) = intent.extracted_params.get("_suggested_tool") {
            // Direct lookup by tool name - trust the LLM's grounded choice
            if let Some(tool) = tools.iter().find(|t| t.name == *suggested_tool) {
                ccos_println!("   🎯 Using LLM-suggested tool: {}", suggested_tool);

                let capability_id = self
                    .discovery
                    .register_tool(tool)
                    .await
                    .map_err(|e| ResolutionError::McpError(e))?;

                let arguments = self.extract_arguments(intent, tool);

                return Ok(ResolvedCapability::Remote {
                    capability_id,
                    server_url: tool.server.url.clone(),
                    arguments,
                    input_schema: tool.input_schema.clone(),
                    confidence: 1.0, // High confidence - LLM chose from exact tool list
                });
            } else {
                ccos_println!(
                    "   ⚠️ Suggested tool '{}' not found, falling back to scoring",
                    suggested_tool
                );
            }
        }

        // Check if LLM explicitly said "no tool" (grounded decomposition returned null)
        if intent.extracted_params.contains_key("_grounded_no_tool") {
            ccos_println!(
                "   ⚠️ Grounded planner found no tool for this step. Skipping fallback scoring."
            );
            return Err(ResolutionError::GroundedNoTool(
                "Grounded planner explicitly returned no tool".to_string(),
            ));
        }

        // Fallback: Score all tools (for non-grounded decomposition)
        let mut scored: Vec<(McpToolInfo, f64)> = Vec::new();
        for tool in tools {
            let score = self.score_tool(intent, &tool).await;
            if score >= self.min_score {
                scored.push((tool, score));
            }
        }

        // Sort by score
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        if scored.is_empty() {
            return Err(ResolutionError::NotFound(format!(
                "No MCP tools above threshold for: {}",
                intent.description
            )));
        }

        let (best_tool, best_score) = scored.remove(0);

        // Register the tool as a capability
        let capability_id = self
            .discovery
            .register_tool(&best_tool)
            .await
            .map_err(|e| ResolutionError::McpError(e))?;

        let arguments = self.extract_arguments(intent, &best_tool);

        Ok(ResolvedCapability::Remote {
            capability_id,
            server_url: best_tool.server.url.clone(),
            arguments,
            input_schema: best_tool.input_schema.clone(),
            confidence: best_score,
        })
    }

    async fn list_available_tools(&self, domain_hints: Option<&[DomainHint]>) -> Vec<ToolSummary> {
        // Try to get tools from all known MCP servers
        let mut all_tools = Vec::new();

        let servers_to_query = if let Some(hints) = domain_hints {
            if hints.is_empty() {
                // Empty hints -> unknown domain -> search ALL servers
                self.discovery.list_known_servers().await
            } else {
                // Specific hints -> search matching servers
                let mut servers = Vec::new();
                for hint in hints {
                    if let Some(s) = self.discovery.get_server_for_domain(hint).await {
                        servers.push(s);
                    } else if let DomainHint::Generic = hint {
                        // If Generic is requested, maybe search everything too?
                        // Or just rely on builtin.
                        // For now, let's assume Generic doesn't map to a specific MCP server unless configured.
                    }
                }

                // If specific hints yielded nothing (e.g. unknown domain but not empty hints?), fallback to all
                if servers.is_empty() {
                    self.discovery.list_known_servers().await
                } else {
                    servers
                }
            }
        } else {
            // No hints provided (None) -> search ALL servers
            self.discovery.list_known_servers().await
        };

        for server in servers_to_query {
            // Deduplicate by URL to avoid querying same server multiple times
            if all_tools.iter().any(|t: &ToolSummary| {
                // Check if we already have tools from this domain/server?
                // Hard to check from ToolSummary alone without metadata.
                // But `get_tools` handles caching, so it's cheap to call.
                false
            }) {
                continue;
            }

            match self.get_tools(&server).await {
                Ok(tools) => {
                    for tool in &tools {
                        // Register each tool in marketplace and catalog
                        // This ensures CatalogResolution can find them by ID
                        if let Err(e) = self.discovery.register_tool(tool).await {
                            log::warn!("Failed to register tool '{}': {}", tool.name, e);
                        }

                        // Convert to ToolSummary for grounded decomposition
                        let cached = CachedToolInfo::from(tool);
                        all_tools.push(cached.to_tool_summary());
                    }
                }
                Err(e) => {
                    log::debug!("Failed to get tools for {}: {}", server.name, e);
                }
            }
        }

        // Deduplicate tools by name/id
        all_tools.sort_by(|a, b| a.name.cmp(&b.name));
        all_tools.dedup_by(|a, b| a.name == b.name);

        all_tools
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockMcpDiscovery;

    #[async_trait(?Send)]
    impl McpDiscovery for MockMcpDiscovery {
        async fn get_server_for_domain(&self, domain: &DomainHint) -> Option<McpServerInfo> {
            // Match on domain string for config-driven approach
            if domain.to_domain_string() == "github" {
                Some(McpServerInfo {
                    name: "github".to_string(),
                    url: "https://api.github.com/mcp".to_string(),
                    namespace: "github".to_string(),
                })
            } else {
                None
            }
        }

        async fn discover_tools(
            &self,
            _server: &McpServerInfo,
        ) -> Result<Vec<McpToolInfo>, String> {
            Ok(vec![McpToolInfo {
                name: "list_issues".to_string(),
                description: "List issues in a repository".to_string(),
                input_schema: None,
                server: McpServerInfo {
                    name: "github".to_string(),
                    url: "https://api.github.com/mcp".to_string(),
                    namespace: "github".to_string(),
                },
            }])
        }

        async fn register_tool(&self, tool: &McpToolInfo) -> Result<String, String> {
            Ok(format!("mcp.{}.{}", tool.server.namespace, tool.name))
        }

        async fn list_known_servers(&self) -> Vec<McpServerInfo> {
            vec![]
        }
    }

    #[tokio::test]
    async fn test_mcp_resolution() {
        use crate::planner::modular_planner::types::ApiAction;

        let discovery = Arc::new(MockMcpDiscovery);
        let strategy = McpResolution::new(discovery);
        let context = ResolutionContext::new();

        let intent = SubIntent::new(
            "List issues from repository",
            IntentType::ApiCall {
                action: ApiAction::List,
            },
        )
        .with_domain(DomainHint::Custom("github".to_string()))
        .with_param("owner", "mandubian")
        .with_param("repo", "ccos");

        let result = strategy
            .resolve(&intent, &context)
            .await
            .expect("Should resolve");

        match result {
            ResolvedCapability::Remote { capability_id, .. } => {
                assert_eq!(capability_id, "mcp.github.list_issues");
            }
            _ => panic!("Expected Remote capability"),
        }
    }
}
