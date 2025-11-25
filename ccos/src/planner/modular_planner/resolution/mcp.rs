//! MCP resolution strategy
//!
//! Discovers and resolves capabilities from MCP servers.

use std::collections::HashMap;
use std::sync::Arc;
use async_trait::async_trait;

use super::{ResolutionContext, ResolutionError, ResolutionStrategy, ResolvedCapability};
use crate::planner::modular_planner::types::{IntentType, SubIntent, DomainHint};
use crate::planner::modular_planner::decomposition::grounded_llm::{EmbeddingProvider, cosine_similarity};

// Imports for RuntimeMcpDiscovery
use crate::synthesis::mcp_registry_client::McpRegistryClient;
use crate::synthesis::mcp_session::{MCPSessionManager, MCPServerInfo as SessionServerInfo};
use crate::capability_marketplace::CapabilityMarketplace;
use crate::synthesis::mcp_introspector::{MCPIntrospector, DiscoveredMCPTool};

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
}

/// Runtime implementation of McpDiscovery using real MCP servers
pub struct RuntimeMcpDiscovery {
    registry_client: McpRegistryClient,
    session_manager: Arc<MCPSessionManager>,
    marketplace: Arc<CapabilityMarketplace>,
}

impl RuntimeMcpDiscovery {
    pub fn new(
        session_manager: Arc<MCPSessionManager>,
        marketplace: Arc<CapabilityMarketplace>,
    ) -> Self {
        Self {
            registry_client: McpRegistryClient::new(),
            session_manager,
            marketplace,
        }
    }

    /// Resolve MCP server URL from overrides.json
    fn resolve_server_url_from_overrides(&self, hint: &str) -> Option<(String, String)> {
        // Try to load curated overrides from 'capabilities/mcp/overrides.json' (in workspace root)
        let root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let overrides_path = if root.join("ccos/Cargo.toml").exists() {
            root.join("capabilities/mcp/overrides.json")
        } else if root.join("Cargo.toml").exists() && root.ends_with("ccos") {
            root.parent().unwrap_or(&root).join("capabilities/mcp/overrides.json")
        } else {
            root.join("capabilities/mcp/overrides.json")
        };

        if !overrides_path.exists() {
            return None;
        }

        let content = std::fs::read_to_string(&overrides_path).ok()?;
        let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
        let entries = parsed.get("entries")?.as_array()?;

        for entry in entries {
            if let Some(server) = entry.get("server") {
                if let Some(matches) = entry.get("matches").and_then(|m| m.as_array()) {
                    for pat in matches {
                        if let Some(p) = pat.as_str() {
                            let pattern_clean = p.trim_end_matches(".*").trim_end_matches('*');
                            if hint.contains(pattern_clean) || pattern_clean.contains(hint) {
                                if let Some(remotes) = server.get("remotes").and_then(|r| r.as_array()) {
                                    for remote in remotes {
                                        if let Some(url) = remote.get("url").and_then(|u| u.as_str()) {
                                            if url.starts_with("http") {
                                                let server_name = pattern_clean.to_string();
                                                return Some((url.to_string(), server_name));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }
}

#[async_trait(?Send)]
impl McpDiscovery for RuntimeMcpDiscovery {
    async fn get_server_for_domain(&self, domain: &DomainHint) -> Option<McpServerInfo> {
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

        // 1. Check overrides
        if let Some((url, name)) = self.resolve_server_url_from_overrides(hint) {
            return Some(McpServerInfo {
                name: name.clone(),
                url,
                namespace: name,
            });
        }

        // 2. Check registry
        // TODO: Implement registry search based on domain hint
        // For now, if it's GitHub, try environment variable
        if hint == "github" {
            if let Ok(endpoint) = std::env::var("GITHUB_MCP_ENDPOINT") {
                return Some(McpServerInfo {
                    name: "github".to_string(),
                    url: endpoint,
                    namespace: "github".to_string(),
                });
            }
        }

        None
    }

    async fn discover_tools(&self, server: &McpServerInfo) -> Result<Vec<McpToolInfo>, String> {
        let client_info = SessionServerInfo {
            name: "modular-planner".to_string(),
            version: "1.0.0".to_string(),
        };

        let session = self.session_manager.initialize_session(&server.url, &client_info).await
            .map_err(|e| format!("MCP init failed: {}", e))?;

        let tools_resp = self.session_manager
            .make_request(&session, "tools/list", serde_json::json!({}))
            .await
            .map_err(|e| format!("MCP tools/list failed: {}", e))?;

        let empty_vec = vec![];
        let tools_array = tools_resp
            .get("result")
            .and_then(|r| r.get("tools"))
            .and_then(|t| t.as_array())
            .unwrap_or(&empty_vec);

        let tools = tools_array.iter().map(|t| {
            let name = t.get("name").and_then(|n| n.as_str()).unwrap_or("unknown").to_string();
            let description = t.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string();
            let input_schema = t.get("inputSchema").cloned();
            
            McpToolInfo {
                name,
                description,
                input_schema,
                server: server.clone(),
            }
        }).collect();

        Ok(tools)
    }

    async fn register_tool(&self, tool: &McpToolInfo) -> Result<String, String> {
        let introspector = MCPIntrospector::new();
        
        let discovered_tool = DiscoveredMCPTool {
            tool_name: tool.name.clone(),
            description: Some(tool.description.clone()),
            input_schema: tool.input_schema.as_ref()
                .and_then(|s| MCPIntrospector::type_expr_from_json_schema(s).ok()),
            output_schema: None, // We don't have output schema from tools/list usually, could introspect
            input_schema_json: tool.input_schema.clone(),
        };

        // Create capability manifest
        let introspection_result = crate::synthesis::mcp_introspector::MCPIntrospectionResult {
            server_url: tool.server.url.clone(),
            server_name: tool.server.name.clone(),
            protocol_version: "2024-11-05".to_string(), // Assume recent version
            tools: vec![discovered_tool.clone()],
        };

        let manifest = introspector.create_capability_from_mcp_tool(
            &discovered_tool,
            &introspection_result
        ).map_err(|e| format!("Failed to create manifest: {}", e))?;

        // Register in marketplace
        self.marketplace.register_capability_manifest(manifest.clone()).await
            .map_err(|e| format!("Failed to register capability: {}", e))?;

        Ok(manifest.id)
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
    /// Cache of discovered tools
    tool_cache: std::sync::Mutex<HashMap<String, Vec<McpToolInfo>>>,
}

impl McpResolution {
    pub fn new(discovery: Arc<dyn McpDiscovery>) -> Self {
        Self {
            discovery,
            embedding_provider: None,
            min_score: 0.3,
            tool_cache: std::sync::Mutex::new(HashMap::new()),
        }
    }
    
    pub fn with_embedding(mut self, provider: Arc<dyn EmbeddingProvider>) -> Self {
        self.embedding_provider = Some(provider);
        self
    }
    
    /// Get or discover tools from cache
    async fn get_tools(&self, server: &McpServerInfo) -> Result<Vec<McpToolInfo>, ResolutionError> {
        // Check cache first
        {
            let cache = self.tool_cache.lock().unwrap();
            if let Some(tools) = cache.get(&server.url) {
                return Ok(tools.clone());
            }
        }
        
        // Discover tools
        let tools = self.discovery.discover_tools(server).await
            .map_err(|e| ResolutionError::McpError(e))?;
        
        // Cache them
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
    fn extract_arguments(&self, intent: &SubIntent, _tool: &McpToolInfo) -> HashMap<String, String> {
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
        let domain = intent.domain_hint.as_ref()
            .ok_or_else(|| ResolutionError::NotFound("No domain hint".to_string()))?;
        
        // Get MCP server for domain
        let server = self.discovery.get_server_for_domain(domain).await
            .ok_or_else(|| ResolutionError::NotFound(
                format!("No MCP server for domain {:?}", domain)
            ))?;
        
        // Discover tools
        let tools = self.get_tools(&server).await?;
        
        if tools.is_empty() {
            return Err(ResolutionError::NotFound(
                format!("No tools found on MCP server: {}", server.name)
            ));
        }
        
        // Score all tools
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
        let capability_id = self.discovery.register_tool(&best_tool).await
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
}

#[cfg(test)]
mod tests {
    use super::*;
    
    struct MockMcpDiscovery;
    
    #[async_trait(?Send)]
    impl McpDiscovery for MockMcpDiscovery {
        async fn get_server_for_domain(&self, domain: &DomainHint) -> Option<McpServerInfo> {
            match domain {
                DomainHint::GitHub => Some(McpServerInfo {
                    name: "github".to_string(),
                    url: "https://api.github.com/mcp".to_string(),
                    namespace: "github".to_string(),
                }),
                _ => None,
            }
        }
        
        async fn discover_tools(&self, _server: &McpServerInfo) -> Result<Vec<McpToolInfo>, String> {
            Ok(vec![
                McpToolInfo {
                    name: "list_issues".to_string(),
                    description: "List issues in a repository".to_string(),
                    input_schema: None,
                    server: McpServerInfo {
                        name: "github".to_string(),
                        url: "https://api.github.com/mcp".to_string(),
                        namespace: "github".to_string(),
                    },
                },
            ])
        }
        
        async fn register_tool(&self, tool: &McpToolInfo) -> Result<String, String> {
            Ok(format!("mcp.{}.{}", tool.server.namespace, tool.name))
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
            IntentType::ApiCall { action: ApiAction::List },
        )
        .with_domain(DomainHint::GitHub)
        .with_param("owner", "mandubian")
        .with_param("repo", "ccos");
        
        let result = strategy.resolve(&intent, &context).await.expect("Should resolve");
        
        match result {
            ResolvedCapability::Remote { capability_id, .. } => {
                assert_eq!(capability_id, "mcp.github.list_issues");
            }
            _ => panic!("Expected Remote capability"),
        }
    }
}
