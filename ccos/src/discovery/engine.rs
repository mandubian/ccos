//! Discovery engine for finding and synthesizing capabilities

use crate::arbiter::delegating_arbiter::DelegatingArbiter;
use crate::capability_marketplace::CapabilityMarketplace;
use crate::capability_marketplace::types::CapabilityManifest;
use crate::discovery::introspection_cache::IntrospectionCache;
use crate::discovery::need_extractor::CapabilityNeed;
use crate::discovery::recursive_synthesizer::RecursiveSynthesizer;
use crate::intent_graph::IntentGraph;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use std::sync::{Arc, Mutex};

/// Statistics for MCP discovery summary
#[derive(Debug, Default)]
struct MCPDiscoveryStats {
    total_servers: usize,
    skipped_no_url: usize,
    skipped_websocket: usize,
    skipped_invalid: usize,
    introspected: usize,
    cached: usize,
    failed: usize,
    tools_found: usize,
    matched_servers: Vec<String>, // Server names that had matches
}

/// Discovery engine that orchestrates the search for capabilities
pub struct DiscoveryEngine {
    marketplace: Arc<CapabilityMarketplace>,
    intent_graph: Arc<Mutex<IntentGraph>>,
    /// Optional delegating arbiter for recursive synthesis
    delegating_arbiter: Option<Arc<DelegatingArbiter>>,
    /// Optional introspection cache for MCP/OpenAPI results
    introspection_cache: Option<Arc<IntrospectionCache>>,
}

impl DiscoveryEngine {
    /// Create a new discovery engine
    pub fn new(
        marketplace: Arc<CapabilityMarketplace>,
        intent_graph: Arc<Mutex<IntentGraph>>,
    ) -> Self {
        Self {
            marketplace,
            intent_graph,
            delegating_arbiter: None,
            introspection_cache: None,
        }
    }
    
    /// Create a new discovery engine with delegating arbiter for recursive synthesis
    pub fn new_with_arbiter(
        marketplace: Arc<CapabilityMarketplace>,
        intent_graph: Arc<Mutex<IntentGraph>>,
        delegating_arbiter: Option<Arc<DelegatingArbiter>>,
    ) -> Self {
        Self {
            marketplace,
            intent_graph,
            delegating_arbiter,
            introspection_cache: None,
        }
    }
    
    /// Create a discovery engine with introspection cache
    pub fn with_cache(mut self, cache: Arc<IntrospectionCache>) -> Self {
        self.introspection_cache = Some(cache);
        self
    }
    
    /// Attempt to find a capability using the discovery priority chain
    pub async fn discover_capability(&self, need: &CapabilityNeed) -> RuntimeResult<DiscoveryResult> {
        // Print capability section header
        eprintln!("\n{}", "═".repeat(80));
        eprintln!("🔍 DISCOVERY: {}", need.capability_class);
        eprintln!("{}", "─".repeat(80));
        eprintln!("  Rationale: {}", need.rationale);
        eprintln!("  Inputs: {:?}", need.required_inputs);
        eprintln!("  Outputs: {:?}", need.expected_outputs);
        eprintln!("{}", "─".repeat(80));
        
        // 1. Try local marketplace search first
        eprintln!("  [1/4] Searching local marketplace...");
        if let Some(manifest) = self.search_marketplace(need).await? {
            // Check if the capability is incomplete
            let is_incomplete = manifest.metadata.get("status")
                .map(|s| s == "incomplete")
                .unwrap_or(false);
            
            if is_incomplete {
                eprintln!("  ⚠️  Found incomplete capability: {}", manifest.id);
                eprintln!("{}", "═".repeat(80));
                return Ok(DiscoveryResult::Incomplete(manifest));
            } else {
                eprintln!("  ✓ Found: {}", manifest.id);
                eprintln!("{}", "═".repeat(80));
                return Ok(DiscoveryResult::Found(manifest));
            }
        }
        eprintln!("  ✗ Not found");
        
        // 2. Try MCP registry search
        eprintln!("  [2/4] Searching MCP registry...");
        if let Some(manifest) = self.search_mcp_registry(need).await? {
            // Check if the capability is incomplete (shouldn't happen for MCP, but check anyway)
            let is_incomplete = manifest.metadata.get("status")
                .map(|s| s == "incomplete")
                .unwrap_or(false);
            
            // Register the discovered MCP capability in marketplace for future searches
            if let Err(e) = self.marketplace.register_capability_manifest(manifest.clone()).await {
                eprintln!("  ⚠  Warning: Failed to register MCP capability: {}", e);
            } else {
                eprintln!("       Registered MCP capability in marketplace");
            }
            eprintln!("{}", "═".repeat(80));
            
            if is_incomplete {
                eprintln!("  ⚠️  Found incomplete MCP capability: {}", manifest.id);
                return Ok(DiscoveryResult::Incomplete(manifest));
            } else {
                eprintln!("  ✓ Found: {}", manifest.id);
                return Ok(DiscoveryResult::Found(manifest));
            }
        }
        eprintln!("  ✗ Not found");
        
        // 3. Try OpenAPI introspection
        eprintln!("  [3/4] Searching OpenAPI services...");
        if let Some(manifest) = self.search_openapi(need).await? {
            eprintln!("  ✓ Found: {}", manifest.id);
            eprintln!("{}", "═".repeat(80));
            return Ok(DiscoveryResult::Found(manifest));
        }
        eprintln!("  ✗ Not found");
        
        // 4. Try recursive synthesis (if delegating arbiter is available)
        eprintln!("  [4/4] Attempting recursive synthesis...");
        if let Some(ref arbiter) = self.delegating_arbiter {
            eprintln!("       Synthesizing capability: {}", need.capability_class);
            
            let context = DiscoveryContext::new(5); // Default max depth of 5
            let mut synthesizer = RecursiveSynthesizer::new(
                DiscoveryEngine::new(
                    Arc::clone(&self.marketplace),
                    Arc::clone(&self.intent_graph),
                ),
                Some(Arc::clone(arbiter)),
                5, // max depth
            );
            
            match synthesizer.synthesize_as_intent(need, &context).await {
                Ok(synthesized) => {
                    // Check if the synthesized capability is incomplete
                    let is_incomplete = synthesized.manifest.metadata.get("status")
                        .map(|s| s == "incomplete")
                        .unwrap_or(false);
                    
                    if is_incomplete {
                        eprintln!("  ⚠️  Synthesized incomplete capability: {}", synthesized.manifest.id);
                        // Register the incomplete capability in the marketplace
                        if let Err(e) = self.marketplace.register_capability_manifest(synthesized.manifest.clone()).await {
                            eprintln!("  ⚠  Warning: Failed to register: {}", e);
                        } else {
                            eprintln!("       Registered as incomplete: {}", synthesized.manifest.id);
                        }
                        eprintln!("{}", "═".repeat(80));
                        return Ok(DiscoveryResult::Incomplete(synthesized.manifest));
                    } else {
                        eprintln!("  ✓ Synthesized: {}", synthesized.manifest.id);
                        // Register the synthesized capability in the marketplace
                        if let Err(e) = self.marketplace.register_capability_manifest(synthesized.manifest.clone()).await {
                            eprintln!("  ⚠  Warning: Failed to register: {}", e);
                        } else {
                            eprintln!("       Registered as: {}", synthesized.manifest.id);
                        }
                        eprintln!("{}", "═".repeat(80));
                        // Mark as synthesized (not just found)
                        return Ok(DiscoveryResult::Found(synthesized.manifest));
                    }
                }
                Err(e) => {
                    eprintln!("  ✗ Synthesis failed: {}", e);
                }
            }
        } else {
            eprintln!("  ⚠  No arbiter available");
        }
        
        eprintln!("{}", "═".repeat(80));
        eprintln!("  ✗ Discovery failed for: {}", need.capability_class);
        eprintln!("{}", "═".repeat(80));
        
        // 5. Not found
        Ok(DiscoveryResult::NotFound)
    }
    
    /// Search the local marketplace for a matching capability
    /// Uses hybrid matching: description-first (what it does), then name-based
    async fn search_marketplace(&self, need: &CapabilityNeed) -> RuntimeResult<Option<CapabilityManifest>> {
        // First, try exact class match
        if let Some(manifest) = self.marketplace.get_capability(&need.capability_class).await {
            // Verify inputs/outputs compatibility
            if self.is_compatible(&manifest, need) {
                return Ok(Some(manifest));
            }
        }
        
        // Semantic search for approximate matches using description/rationale
        let all_capabilities = self.marketplace.list_capabilities().await;
        let mut best_match: Option<(CapabilityManifest, f64, String)> = None; // (manifest, score, match_type)
        let threshold = 0.5;
        
        // First pass: description-based matching (what the capability does)
        for manifest in &all_capabilities {
            let desc_score = crate::discovery::capability_matcher::calculate_description_match_score(
                &need.rationale,
                &manifest.description,
                &manifest.name,
            );
            
            // Debug logging for top candidates
            if desc_score >= 0.3 || manifest.id.contains("github") || manifest.description.contains("issue") {
                eprintln!("  [DEBUG] Description match: {} → {} (score: {:.3})", 
                    need.rationale, manifest.id, desc_score);
                eprintln!("         Need rationale: {}", need.rationale);
                eprintln!("         Manifest desc: {}", manifest.description);
            }
            
            if desc_score >= threshold {
                match &best_match {
                    Some((_, best_score, _)) if desc_score > *best_score => {
                        best_match = Some((manifest.clone(), desc_score, "description".to_string()));
                    }
                    None => {
                        best_match = Some((manifest.clone(), desc_score, "description".to_string()));
                    }
                    _ => {}
                }
            }
        }
        
        // Second pass: name-based matching (for cases where description is vague)
        for manifest in &all_capabilities {
            let name_score = crate::discovery::capability_matcher::calculate_semantic_match_score(
                &need.capability_class,
                &manifest.id,
                &manifest.name,
            );
            
            if name_score >= threshold {
                match &best_match {
                    Some((_, best_score, _)) if name_score > *best_score => {
                        best_match = Some((manifest.clone(), name_score, "name".to_string()));
                    }
                    None => {
                        best_match = Some((manifest.clone(), name_score, "name".to_string()));
                    }
                    _ => {}
                }
            }
        }
        
        if let Some((manifest, score, match_type)) = best_match {
            eprintln!("  ✓ Marketplace semantic match ({}): {} (score: {:.2})", match_type, manifest.id, score);
            return Ok(Some(manifest));
        }
        
        Ok(None)
    }
    
    /// Check if a capability manifest is compatible with the need
    fn is_compatible(&self, _manifest: &CapabilityManifest, _need: &CapabilityNeed) -> bool {
        // For now, just check that it has inputs and outputs
        // TODO: Implement proper schema compatibility checking
        true
    }
    
    /// Get the marketplace (for cloning into recursive synthesizer)
    pub fn get_marketplace(&self) -> Arc<CapabilityMarketplace> {
        Arc::clone(&self.marketplace)
    }
    
    /// Get the intent graph (for cloning into recursive synthesizer)
    pub fn get_intent_graph(&self) -> Arc<Mutex<IntentGraph>> {
        Arc::clone(&self.intent_graph)
    }
    
    /// Find related capabilities in marketplace by namespace/pattern to provide as examples
    /// Returns up to `max_examples` capabilities that share the namespace or related keywords
    pub async fn find_related_capabilities(
        &self,
        capability_class: &str,
        max_examples: usize,
    ) -> Vec<CapabilityManifest> {
        // Extract namespace from capability class (e.g., "restaurant.api.search" -> "restaurant")
        let namespace = capability_class.split('.').next().unwrap_or("");
        
        if namespace.is_empty() {
            return vec![];
        }
        
        // Search for capabilities with the same namespace prefix using glob pattern
        // e.g., "restaurant.*" matches "restaurant.api.search", "restaurant.booking.reserve", etc.
        let pattern = format!("{}.*", namespace);
        self.marketplace.search_by_id(&pattern).await
            .into_iter()
            .take(max_examples)
            .collect()
    }
    
    /// Search MCP registry for a capability
    pub async fn search_mcp_registry(&self, need: &CapabilityNeed) -> RuntimeResult<Option<CapabilityManifest>> {
        // Use MCP registry client to search for servers
        let registry_client = crate::synthesis::mcp_registry_client::McpRegistryClient::new();
        
        // Extract search keywords from capability class
        // e.g., "restaurant.api.reserve" -> search for "restaurant" and "reserve"
        let keywords: Vec<&str> = need.capability_class.split('.').collect();
        let search_query = keywords.join(" "); // Use space-separated keywords for search
        
        eprintln!("  → MCP registry search query: '{}'", search_query);
        
        // First, check curated overrides (capabilities/mcp/overrides.json)
        let curated_servers = self.load_curated_overrides_for(&need.capability_class)?;
        let mut servers = if !curated_servers.is_empty() {
            eprintln!("  → Found {} curated override(s) for '{}'", curated_servers.len(), need.capability_class);
            curated_servers
        } else {
            Vec::new()
        };
        
        // Then search MCP registry for matching servers
        let registry_servers = match registry_client.search_servers(&search_query).await {
            Ok(registry_servers) => {
                eprintln!("  → Found {} MCP server(s) from registry for '{}'", registry_servers.len(), search_query);
                registry_servers
            },
            Err(e) => {
                eprintln!("  → MCP registry search failed: {}", e);
                eprintln!("     ⚠️  Could not connect to MCP registry or search failed");
                Vec::new()
            }
        };
        
        // Merge curated (prioritized) with registry results, avoiding duplicates
        let mut seen_names = std::collections::HashSet::new();
        for server in &servers {
            seen_names.insert(server.name.clone());
        }
        for server in registry_servers {
            if !seen_names.contains(&server.name) {
                servers.push(server);
            }
        }
        
        // If no servers found with full query, try searching with just the first keyword
        // e.g., if "github issues list" finds nothing, try just "github"
        if servers.is_empty() && !keywords.is_empty() {
            let first_keyword = keywords[0];
            eprintln!("  → No servers found, trying simpler query: '{}'", first_keyword);
            let fallback_servers = match registry_client.search_servers(first_keyword).await {
                Ok(fallback_servers) => {
                    eprintln!("  → Found {} MCP server(s) for '{}'", fallback_servers.len(), first_keyword);
                    fallback_servers
                },
                Err(_) => Vec::new(),
            };
            servers.extend(fallback_servers);
        }
        
        if servers.is_empty() {
            eprintln!("     ⚠️  No MCP servers found in registry");
            eprintln!("     💡 The MCP registry may not have GitHub servers configured");
            eprintln!("     💡 Alternative: Use known MCP server URLs directly");
            return Ok(None);
        }
        
        // Introspect each server to find matching tools
        let introspector = crate::synthesis::mcp_introspector::MCPIntrospector::new();
        
        // Statistics for summary
        let mut stats = MCPDiscoveryStats {
            total_servers: servers.len(),
            skipped_no_url: 0,
            skipped_websocket: 0,
            skipped_invalid: 0,
            introspected: 0,
            cached: 0,
            failed: 0,
            tools_found: 0,
            matched_servers: Vec::new(),
        };
        
        if servers.len() > 1 {
            eprintln!("  → Searching {} MCP server(s)...", servers.len());
        }
        
        for server in servers.iter() {
            
            // Try to get server URL from remotes first, then check for environment variable overrides
            let mut server_url = server.remotes.as_ref()
                .and_then(|remotes| remotes.first())
                .map(|remote| remote.url.clone());
            
            // For servers without remotes (stdio-based), check for environment variable overrides
            // e.g., GITHUB_MCP_URL for GitHub MCP server
            if server_url.is_none() {
                // Derive a simpler env var name from server name
                // "github/github-mcp" -> "GITHUB_MCP_URL"
                // "github/github-mcp" -> extract namespace: "github" -> "GITHUB_MCP_URL"
                let env_var_name = if let Some(slash_pos) = server.name.find('/') {
                    // Extract namespace part (before first slash)
                    let namespace = &server.name[..slash_pos];
                    format!("{}_MCP_URL", namespace.replace("-", "_").to_uppercase())
                } else {
                    // No slash, use full name
                    format!("{}_MCP_URL", 
                        server.name
                            .replace("-", "_")
                            .to_uppercase()
                    )
                };
                
                // Also check generic MCP_SERVER_URL and alternative formats
                let env_vars_to_check = vec![
                    env_var_name.clone(),
                    "MCP_SERVER_URL".to_string(),
                    format!("{}_URL", server.name.replace("/", "_").replace("-", "_").to_uppercase()),
                ];
                
                for env_var in env_vars_to_check {
                    if let Ok(url) = std::env::var(&env_var) {
                        if !url.is_empty() {
                            eprintln!("     → Found server URL from environment: {} = {}", env_var, url);
                            server_url = Some(url);
                            break;
                        }
                    }
                }
                
                // If still no URL, this is a stdio-based server that requires local setup
                if server_url.is_none() {
                    stats.skipped_no_url += 1;
                    // Only log details for single server searches
                    if servers.len() == 1 {
                        eprintln!("     ⚠️  No remote URL found (stdio-based server, requires local npm package)");
                        if let Some(ref packages) = server.packages {
                            if let Some(pkg) = packages.first() {
                                eprintln!("     → Package: {}@{} (registry: {})", 
                                    pkg.identifier,
                                    pkg.version.as_ref().unwrap_or(&"latest".to_string()),
                                    pkg.registry_base_url.as_ref().unwrap_or(&"unknown".to_string())
                                );
                                let suggested_env_var = if let Some(slash_pos) = server.name.find('/') {
                                    let namespace = &server.name[..slash_pos];
                                    format!("{}_MCP_URL", namespace.replace("-", "_").to_uppercase())
                                } else {
                                    format!("{}_MCP_URL", 
                                        server.name.replace("-", "_").to_uppercase()
                                    )
                                };
                                eprintln!("     💡 Set {} environment variable to point to a remote MCP endpoint", suggested_env_var);
                                eprintln!("     💡 Or add a 'remotes' entry to overrides.json with an HTTP/HTTPS URL");
                            }
                        }
                    }
                    continue;
                }
            }
            
            if let Some(url) = server_url {
                // Validate URL is a valid MCP endpoint
                // Skip WebSocket URLs (wss:///ws://) - they require different connection method
                if url.starts_with("ws://") || url.starts_with("wss://") {
                    stats.skipped_websocket += 1;
                    if servers.len() == 1 {
                        eprintln!("     ⚠️  Skipping: WebSocket URLs not supported for HTTP-based introspection");
                        eprintln!("     → URL: {}", url);
                    }
                    continue;
                }
                
                // Only support HTTP/HTTPS for introspection (mcp:// is also valid but less common)
                if !url.starts_with("http://") 
                    && !url.starts_with("https://")
                    && !url.starts_with("mcp://") {
                    stats.skipped_invalid += 1;
                    if servers.len() == 1 {
                        eprintln!("     ⚠️  Skipping: Invalid URL scheme (expected http/https): {}", url);
                    }
                    continue;
                }
                
                // Filter out common repository URLs that aren't MCP endpoints
                if url.contains("github.com/") && !url.contains("/api/") && !url.contains("mcp") {
                    stats.skipped_invalid += 1;
                    if servers.len() == 1 {
                        eprintln!("     ⚠️  Skipping: Appears to be a repository URL, not an MCP endpoint");
                        eprintln!("     → URL: {}", url);
                    }
                    continue;
                }
                
                // Only show detailed URL for single server
                if servers.len() == 1 {
                    eprintln!("     → Server: {} ({})", server.name, url);
                }
                
                // Build auth headers from environment (if available)
                let mut auth_headers = std::collections::HashMap::new();
                if let Ok(token) = std::env::var("GITHUB_TOKEN").or_else(|_| std::env::var("MCP_AUTH_TOKEN")) {
                    auth_headers.insert("Authorization".to_string(), format!("Bearer {}", token));
                }
                
                // Check cache first if available
                let introspection_result = if let Some(ref cache) = self.introspection_cache {
                    match cache.get_mcp(&url) {
                        Ok(Some(cached)) => {
                            stats.cached += 1;
                            stats.tools_found += cached.tools.len();
                            if servers.len() == 1 {
                                eprintln!("     ✓ Using cached introspection ({} tools)", cached.tools.len());
                            }
                            Ok(cached)
                        },
                        Ok(None) | Err(_) => {
                            // Cache miss - introspect the server with auth
                            let result = if auth_headers.is_empty() {
                                introspector.introspect_mcp_server(&url, &server.name).await
                            } else {
                                introspector.introspect_mcp_server_with_auth(
                                    &url,
                                    &server.name,
                                    Some(auth_headers.clone()),
                                ).await
                            };
                            // Cache the result if successful
                            match &result {
                                Ok(introspection) => {
                                    stats.introspected += 1;
                                    stats.tools_found += introspection.tools.len();
                                    if servers.len() == 1 {
                                        eprintln!("     ✓ Introspected successfully ({} tools)", introspection.tools.len());
                                    }
                                    let _ = cache.put_mcp(&url, introspection);
                                }
                                Err(_) => {
                                    stats.failed += 1;
                                    if servers.len() == 1 {
                                        eprintln!("     ✗ Introspection failed");
                                    }
                                }
                            }
                            result
                        }
                    }
                } else {
                    // No cache - just introspect with auth if available
                    let result = if auth_headers.is_empty() {
                        introspector.introspect_mcp_server(&url, &server.name).await
                    } else {
                        introspector.introspect_mcp_server_with_auth(
                            &url,
                            &server.name,
                            Some(auth_headers.clone()),
                        ).await
                    };
                    match &result {
                        Ok(introspection) => {
                            stats.introspected += 1;
                            stats.tools_found += introspection.tools.len();
                            if servers.len() == 1 {
                                eprintln!("     ✓ Introspected successfully ({} tools)", introspection.tools.len());
                            }
                        }
                        Err(_) => {
                            stats.failed += 1;
                            if servers.len() == 1 {
                                eprintln!("     ✗ Introspection failed");
                            }
                        }
                    }
                    result
                };
                
                // Process the introspection result
                match introspection_result {
                    Ok(introspection) => {
                        // Create all capabilities from this server's tools
                        match introspector.create_capabilities_from_mcp(&introspection) {
                            Ok(capabilities) => {
                                // Use hybrid semantic matching: description-first, then name-based
                                let mut best_match: Option<(CapabilityManifest, f64, String)> = None; // (manifest, score, match_type)
                                let threshold = 0.5; // Minimum score to consider a match
                                
                                // First pass: description-based semantic matching (what the capability does)
                                // This is better because LLM generates rationale/description, not exact names
                                // Try embedding-based matching if available, fallback to keyword-based
                                let mut embedding_service = crate::discovery::embedding_service::EmbeddingService::from_env();
                                
                                for manifest in &capabilities {
                                    let desc_score = if let Some(ref mut emb_svc) = embedding_service {
                                        // Use embedding-based matching (more accurate)
                                        crate::discovery::capability_matcher::calculate_description_match_score_with_embedding_async(
                                            &need.rationale,
                                            &manifest.description,
                                            &manifest.name,
                                            Some(emb_svc),
                                        ).await
                                    } else {
                                        // Fallback to keyword-based matching
                                        crate::discovery::capability_matcher::calculate_description_match_score(
                                            &need.rationale,
                                            &manifest.description,
                                            &manifest.name,
                                        )
                                    };
                                    
                                    if desc_score >= threshold {
                                        match &best_match {
                                            Some((_, best_score, _)) if desc_score > *best_score => {
                                                best_match = Some((manifest.clone(), desc_score, "description".to_string()));
                                            }
                                            None => {
                                                best_match = Some((manifest.clone(), desc_score, "description".to_string()));
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                
                                // Second pass: name-based semantic matching (for cases where description is vague)
                                for manifest in &capabilities {
                                    let name_score = crate::discovery::capability_matcher::calculate_semantic_match_score(
                                        &need.capability_class,
                                        &manifest.id,
                                        &manifest.name,
                                    );
                                    
                                    if name_score >= threshold {
                                        match &best_match {
                                            Some((_, best_score, _)) if name_score > *best_score => {
                                                best_match = Some((manifest.clone(), name_score, "name".to_string()));
                                            }
                                            None => {
                                                best_match = Some((manifest.clone(), name_score, "name".to_string()));
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                
                                // Return the best match if found
                                if let Some((manifest, score, match_type)) = best_match {
                                    stats.matched_servers.push(server.name.clone());
                                    if servers.len() == 1 {
                                        eprintln!("  ✓ Semantic match found ({}): {} (score: {:.2})", match_type, manifest.id, score);
                                    }
                                    return Ok(Some(manifest));
                                }
                                
                                // Fallback to simple substring matching for compatibility
                                let capability_name_parts: Vec<&str> = need.capability_class.split('.').collect();
                                let last_part = capability_name_parts.last().unwrap_or(&"");
                                
                                for manifest in &capabilities {
                                    let manifest_id_lower = manifest.id.to_lowercase();
                                    let manifest_name_lower = manifest.name.to_lowercase();
                                    
                                    // Check if capability ID or name matches
                                    let capability_match = capability_name_parts.iter().any(|part| {
                                        manifest_id_lower.contains(&part.to_lowercase()) ||
                                        manifest_name_lower.contains(&part.to_lowercase())
                                    }) || manifest_id_lower.contains(&last_part.to_lowercase()) ||
                                    manifest_name_lower.contains(&last_part.to_lowercase());
                                    
                                    if capability_match {
                                        stats.matched_servers.push(server.name.clone());
                                        if servers.len() == 1 {
                                            eprintln!("  ✓ Substring match found: {}", manifest.id);
                                        }
                                        return Ok(Some(manifest.clone()));
                                    }
                                }
                            }
                            Err(e) => {
                                if servers.len() == 1 {
                                    eprintln!("     ✗ Failed to create capabilities from MCP: {}", e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        if servers.len() == 1 {
                            eprintln!("     ✗ Server introspection failed: {}", e);
                        }
                    }
                }
            }
        }
        
        // Print summary for multiple servers
        if stats.total_servers > 1 {
            eprintln!("  → Summary: {} server(s) searched", stats.total_servers);
            if stats.introspected > 0 {
                eprintln!("     • {} introspected successfully ({} tools)", stats.introspected, stats.tools_found);
            }
            if stats.cached > 0 {
                eprintln!("     • {} from cache", stats.cached);
            }
            if stats.failed > 0 {
                eprintln!("     • {} failed", stats.failed);
            }
            if stats.skipped_no_url > 0 {
                eprintln!("     • {} skipped (no remote URL)", stats.skipped_no_url);
            }
            if stats.skipped_websocket > 0 {
                eprintln!("     • {} skipped (WebSocket not supported)", stats.skipped_websocket);
            }
            if stats.skipped_invalid > 0 {
                eprintln!("     • {} skipped (invalid URL)", stats.skipped_invalid);
            }
            if !stats.matched_servers.is_empty() {
                eprintln!("     • Matched: {}", stats.matched_servers.join(", "));
            } else {
                eprintln!("     ✗ No match found");
            }
        } else if stats.total_servers == 1 {
            eprintln!("  → No match found");
        }
        
        Ok(None)
    }
    
    /// Search OpenAPI services for a capability using web search
    pub async fn search_openapi(&self, need: &CapabilityNeed) -> RuntimeResult<Option<CapabilityManifest>> {
        // Use web search to find actual OpenAPI specs online
        let mut web_searcher = crate::synthesis::web_search_discovery::WebSearchDiscovery::new("auto".to_string());
        
        // Search for the capability
        let search_results = match web_searcher.search_for_api_specs(&need.capability_class).await {
            Ok(results) => results,
            Err(_) => {
                return Ok(None);
            }
        };
        
        if search_results.is_empty() {
            return Ok(None);
        }
        
        // Try to introspect from the top results
        let introspector = crate::synthesis::api_introspector::APIIntrospector::new();
        
        for result in search_results.iter().take(5) { // Limit to top 5 results
            // Extract base URL from the result URL
            let base_url = self.extract_base_url_from_result(&result.url);
            
            // Check cache first if available
            let introspection_result = if let Some(ref cache) = self.introspection_cache {
                match cache.get_openapi(&base_url) {
                    Ok(Some(cached)) => Ok(cached),
                    Ok(None) | Err(_) => {
                        // Cache miss or error - introspect from discovery
                        let result_introspection = introspector.introspect_from_discovery(&base_url, &need.capability_class).await;
                        // Cache the result if successful
                        if let Ok(ref introspection) = result_introspection {
                            let _ = cache.put_openapi(&base_url, introspection);
                        }
                        result_introspection
                    }
                }
            } else {
                // No cache - just introspect
                introspector.introspect_from_discovery(&base_url, &need.capability_class).await
            };
            
            // Process the introspection result
            match introspection_result {
                Ok(introspection) => {
                    // Create capabilities from introspection
                    match introspector.create_capabilities_from_introspection(&introspection) {
                        Ok(capabilities) => {
                            // Find a matching capability
                            let capability_name_parts: Vec<&str> = need.capability_class.split('.').collect();
                            let last_part = capability_name_parts.last().unwrap_or(&"");
                            
                            for manifest in capabilities {
                                let manifest_id_lower = manifest.id.to_lowercase();
                                let manifest_name_lower = manifest.name.to_lowercase();
                                
                                // Check if capability ID or name matches
                                let capability_match = capability_name_parts.iter().any(|part| {
                                    manifest_id_lower.contains(&part.to_lowercase()) ||
                                    manifest_name_lower.contains(&part.to_lowercase())
                                }) || manifest_id_lower.contains(&last_part.to_lowercase()) ||
                                manifest_name_lower.contains(&last_part.to_lowercase());
                                
                                if capability_match {
                                    return Ok(Some(manifest));
                                }
                            }
                        }
                        Err(_) => {
                            continue;
                        }
                    }
                }
                Err(_) => {
                    continue;
                }
            }
        }
        
        Ok(None)
    }
    
    /// Extract base URL from a web search result URL
    fn extract_base_url_from_result(&self, url: &str) -> String {
        // Parse URL to extract base URL
        if let Ok(parsed_url) = url::Url::parse(url) {
            // For OpenAPI spec URLs, try to find the base URL
            // Common patterns: /swagger.json, /openapi.json, /api-docs, etc.
            let path = parsed_url.path();
            if path.ends_with("/swagger.json") || path.ends_with("/openapi.json") {
                // Remove the spec file path to get base URL
                if let Some(base_path) = path.strip_suffix("/swagger.json") {
                    return format!("{}://{}{}", parsed_url.scheme(), parsed_url.host_str().unwrap_or(""), base_path);
                } else if let Some(base_path) = path.strip_suffix("/openapi.json") {
                    return format!("{}://{}{}", parsed_url.scheme(), parsed_url.host_str().unwrap_or(""), base_path);
                }
            }
            // For other paths, use the origin
            format!("{}://{}", parsed_url.scheme(), parsed_url.host_str().unwrap_or(""))
        } else {
            // Fallback: try to extract a sensible base URL
            url.to_string()
        }
    }
    
    /// Create an incomplete capability manifest for capabilities that couldn't be found
    pub fn create_incomplete_capability(need: &CapabilityNeed) -> CapabilityManifest {
        use crate::capability_marketplace::types::{LocalCapability, ProviderType};
        use std::sync::Arc;
        
        let capability_id = need.capability_class.clone();
        let stub_handler: Arc<dyn Fn(&rtfs::runtime::values::Value) -> RuntimeResult<rtfs::runtime::values::Value> + Send + Sync> = 
            Arc::new(move |_input: &rtfs::runtime::values::Value| -> RuntimeResult<rtfs::runtime::values::Value> {
                Err(RuntimeError::Generic(
                    format!("Capability {} is marked as incomplete/not_found and needs implementation", capability_id)
                ))
            });
        
        let mut manifest = CapabilityManifest::new(
            need.capability_class.clone(),
            format!("[INCOMPLETE] {}", need.capability_class),
            format!("Capability needed but not found: {}", need.rationale),
            ProviderType::Local(LocalCapability {
                handler: stub_handler,
            }),
            "0.0.0-incomplete".to_string(),
        );
        
        // Add metadata to mark it as incomplete
        manifest.metadata.insert(
            "status".to_string(),
            "incomplete".to_string(),
        );
        manifest.metadata.insert(
            "discovery_method".to_string(),
            "not_found_after_all_searches".to_string(),
        );
        manifest.metadata.insert(
            "required_inputs".to_string(),
            need.required_inputs.join(","),
        );
        manifest.metadata.insert(
            "expected_outputs".to_string(),
            need.expected_outputs.join(","),
        );
        
        manifest
    }
    
    /// Load curated MCP server overrides from a local JSON file and select those matching the capability id
    fn load_curated_overrides_for(
        &self,
        capability_id: &str,
    ) -> RuntimeResult<Vec<crate::synthesis::mcp_registry_client::McpServer>> {
        use std::fs;
        use std::path::Path;
        
        // Define the override file structure
        #[derive(serde::Deserialize)]
        struct CuratedOverrides {
            pub entries: Vec<CuratedEntry>,
        }
        
        #[derive(serde::Deserialize)]
        struct CuratedEntry {
            pub matches: Vec<String>,
            pub server: crate::synthesis::mcp_registry_client::McpServer,
        }
        
        let root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        // Try workspace root 'capabilities/mcp/overrides.json'. If we are inside rtfs_compiler, go up one level
        let overrides_path = if root.ends_with("rtfs_compiler") {
            root.parent()
                .unwrap_or(&root)
                .join("capabilities/mcp/overrides.json")
        } else {
            root.join("capabilities/mcp/overrides.json")
        };
        
        if !Path::new(&overrides_path).exists() {
            return Ok(Vec::new());
        }
        
        let content = fs::read_to_string(&overrides_path).map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to read curated overrides file '{}': {}",
                overrides_path.display(),
                e
            ))
        })?;
        
        let parsed: CuratedOverrides = serde_json::from_str(&content).map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to parse curated overrides JSON '{}': {}",
                overrides_path.display(),
                e
            ))
        })?;
        
        let mut matched = Vec::new();
        for entry in parsed.entries.iter() {
            if entry
                .matches
                .iter()
                .any(|pat| Self::pattern_match(pat, capability_id))
            {
                matched.push(entry.server.clone());
            }
        }
        
        Ok(matched)
    }
    
    /// Simple wildcard pattern matching supporting:
    /// - exact match
    /// - suffix '*' (prefix match)
    /// - '*' anywhere (contains match)
    fn pattern_match(pattern: &str, text: &str) -> bool {
        if pattern == text {
            return true;
        }
        if pattern.ends_with('*') {
            let prefix = &pattern[..pattern.len() - 1];
            return text.starts_with(prefix);
        }
        if pattern.starts_with('*') {
            let suffix = &pattern[1..];
            return text.ends_with(suffix);
        }
        if pattern.contains('*') {
            let parts: Vec<&str> = pattern.split('*').collect();
            if parts.len() == 2 {
                return text.starts_with(parts[0]) && text.ends_with(parts[1]);
            }
        }
        text.contains(pattern)
    }
    
    /// Collect discovery hints for all capabilities in a plan
    /// Returns hints about found capabilities, missing capabilities, and suggestions
    pub async fn collect_discovery_hints(
        &self,
        capability_ids: &[String],
    ) -> RuntimeResult<DiscoveryHints> {
        let mut found = Vec::new();
        let mut missing = Vec::new();
        let mut suggestions = Vec::new();
        
        for cap_id in capability_ids {
            // Create a minimal CapabilityNeed for this capability ID
            let need = CapabilityNeed::new(
                cap_id.clone(),
                Vec::new(), // Don't know inputs yet
                Vec::new(), // Don't know outputs yet
                format!("Need for capability: {}", cap_id),
            );
            
            match self.discover_capability(&need).await? {
                DiscoveryResult::Found(manifest) => {
                    // Extract hints from manifest
                    let hints = self.extract_capability_hints(&manifest);
                    let parameters = self.extract_parameters_from_manifest(&manifest);
                    
                    found.push(FoundCapability {
                        id: manifest.id.clone(),
                        name: manifest.name.clone(),
                        description: manifest.description.clone(),
                        provider: self.format_provider(&manifest.provider),
                        parameters,
                        hints,
                    });
                }
                DiscoveryResult::Incomplete(_) | DiscoveryResult::NotFound => {
                    missing.push(cap_id.clone());
                    
                    // Check if there's a related capability that could work
                    if let Some(related) = self.find_related_capability(cap_id).await? {
                        suggestions.push(format!(
                            "{} not found, but {} might work: {}",
                            cap_id, related.id, related.description
                        ));
                    }
                }
            }
        }
        
        // Generate suggestions based on found capabilities
        for found_cap in &found {
            // Check if any found capability might help with missing ones
            for missing_id in &missing {
                // Simple heuristic: if capability names share keywords, suggest it
                let found_keywords: Vec<&str> = found_cap.id.split(&['.', '_'][..]).collect();
                let missing_keywords: Vec<&str> = missing_id.split(&['.', '_'][..]).collect();
                
                let common_keywords: Vec<&str> = found_keywords.iter()
                    .filter(|k| missing_keywords.contains(k) && k.len() > 2)
                    .copied()
                    .collect();
                
                if !common_keywords.is_empty() && !found_cap.hints.is_empty() {
                    suggestions.push(format!(
                        "{} not found, but {} (found) might help: {}",
                        missing_id,
                        found_cap.id,
                        found_cap.hints[0]
                    ));
                }
            }
        }
        
        Ok(DiscoveryHints {
            found_capabilities: found,
            missing_capabilities: missing,
            suggestions,
        })
    }
    
    /// Extract hints from a capability manifest
    /// Generic implementation that extracts information from metadata and schemas
    fn extract_capability_hints(&self, manifest: &CapabilityManifest) -> Vec<String> {
        let mut hints = Vec::new();
        
        // Extract provider-specific information
        match &manifest.provider {
            crate::capability_marketplace::types::ProviderType::MCP(mcp) => {
                hints.push(format!("MCP tool: {}", mcp.tool_name));
                if let Some(url) = manifest.metadata.get("mcp_server_url") {
                    hints.push(format!("Server: {}", url));
                }
            }
            crate::capability_marketplace::types::ProviderType::OpenApi(openapi) => {
                hints.push(format!("OpenAPI endpoint: {}", openapi.base_url));
                if let Some(spec_url) = &openapi.spec_url {
                    hints.push(format!("Spec: {}", spec_url));
                }
            }
            _ => {}
        }
        
        // Extract any parameter hints from metadata
        if let Some(hint) = manifest.metadata.get("parameter_hints") {
            hints.push(hint.clone());
        }
        
        // Extract usage hints from metadata
        if let Some(hint) = manifest.metadata.get("usage_hints") {
            hints.push(hint.clone());
        }
        
        // Extract from description field in metadata (if different from main description)
        if let Some(desc) = manifest.metadata.get("mcp_tool_description") {
            if desc != &manifest.description {
                hints.push(desc.clone());
            }
        }
        
        hints
    }
    
    /// Extract parameter names from a capability manifest
    fn extract_parameters_from_manifest(&self, manifest: &CapabilityManifest) -> Vec<String> {
        let mut parameters = Vec::new();
        
        // Try to extract from input schema if available
        if let Some(ref schema) = manifest.input_schema {
            parameters.extend(self.extract_params_from_type_expr(schema));
        }
        
        // Also check metadata for parameter hints
        if let Some(params_str) = manifest.metadata.get("parameters") {
            parameters.extend(
                params_str.split(',')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
            );
        }
        
        // For MCP capabilities, check tool description in metadata
        if let Some(tool_desc) = manifest.metadata.get("mcp_tool_description") {
            // Try to extract parameter names from description
            // Common patterns: "state (open|closed|all)", "labels: array", etc.
            // This is a simple heuristic - could be improved
        }
        
        // Remove duplicates while preserving order
        let mut seen = std::collections::HashSet::new();
        parameters.retain(|p| seen.insert(p.clone()));
        
        parameters
    }
    
    /// Extract parameter names from a TypeExpr (simple implementation)
    fn extract_params_from_type_expr(&self, expr: &rtfs::ast::TypeExpr) -> Vec<String> {
        let mut params = Vec::new();
        
        match expr {
            rtfs::ast::TypeExpr::Map { entries, .. } => {
                for entry in entries {
                    // Extract keyword name (remove the ':' prefix if present)
                    let param_name = entry.key.0.clone();
                    params.push(param_name);
                }
            }
            _ => {
                // For other types, we can't easily extract parameter names
                // This is a limitation - we'd need more schema information
            }
        }
        
        params
    }
    
    /// Find a related capability that might work for the given capability ID
    async fn find_related_capability(
        &self,
        capability_id: &str,
    ) -> RuntimeResult<Option<CapabilityManifest>> {
        // Try to find a capability in the marketplace with similar keywords
        let keywords: Vec<&str> = capability_id.split(&['.', '_'][..])
            .filter(|k| k.len() > 2)
            .collect();
        
        if keywords.is_empty() {
            return Ok(None);
        }
        
        let all_capabilities = self.marketplace.list_capabilities().await;
        
        // Search for capabilities with overlapping keywords
        let mut best_match: Option<(CapabilityManifest, usize)> = None;
        for manifest in all_capabilities {
            let manifest_keywords: Vec<&str> = manifest.id.split(&['.', '_'][..])
                .filter(|k| k.len() > 2)
                .collect();
            
            let overlap = keywords.iter()
                .filter(|k| manifest_keywords.contains(k))
                .count();
            
            if overlap > 0 {
                match best_match {
                    Some((_, best_overlap)) if overlap > best_overlap => {
                        best_match = Some((manifest, overlap));
                    }
                    None => {
                        best_match = Some((manifest, overlap));
                    }
                    _ => {}
                }
            }
        }
        
        Ok(best_match.map(|(manifest, _)| manifest))
    }
    
    /// Format provider type as string for hints
    fn format_provider(&self, provider: &crate::capability_marketplace::types::ProviderType) -> String {
        match provider {
            crate::capability_marketplace::types::ProviderType::MCP(_) => "MCP".to_string(),
            crate::capability_marketplace::types::ProviderType::OpenApi(_) => "OpenAPI".to_string(),
            crate::capability_marketplace::types::ProviderType::Local(_) => "Local".to_string(),
            crate::capability_marketplace::types::ProviderType::Http(_) => "HTTP".to_string(),
            _ => "Unknown".to_string(),
        }
    }
}

/// Result of a discovery attempt
#[derive(Debug, Clone)]
pub enum DiscoveryResult {
    /// Capability found
    Found(CapabilityManifest),
    /// Capability not found - needs synthesis or user input
    NotFound,
    /// Capability needed but not found after all searches - marked as incomplete
    Incomplete(CapabilityManifest), // Manifest with incomplete/not_found status
}

/// Discovery hints for re-planning when capabilities are missing
#[derive(Debug, Clone)]
pub struct DiscoveryHints {
    pub found_capabilities: Vec<FoundCapability>,
    pub missing_capabilities: Vec<String>,
    pub suggestions: Vec<String>,
}

/// Information about a found capability for re-planning hints
#[derive(Debug, Clone)]
pub struct FoundCapability {
    pub id: String,
    pub name: String,
    pub description: String,
    pub provider: String, // "MCP", "OpenAPI", "Local", etc.
    pub parameters: Vec<String>, // Available parameters
    pub hints: Vec<String>, // Usage hints
}

/// Discovery context for tracking discovery attempts
#[derive(Debug, Clone)]
pub struct DiscoveryContext {
    pub max_depth: usize,
    pub current_depth: usize,
    pub visited_intents: Vec<String>,
}

impl DiscoveryContext {
    /// Create a new discovery context
    pub fn new(max_depth: usize) -> Self {
        Self {
            max_depth,
            current_depth: 0,
            visited_intents: Vec::new(),
        }
    }
    
    /// Check if we can go deeper (prevent infinite recursion)
    pub fn can_go_deeper(&self) -> bool {
        self.current_depth < self.max_depth
    }
    
    /// Create a new context one level deeper
    pub fn go_deeper(&self) -> Self {
        Self {
            max_depth: self.max_depth,
            current_depth: self.current_depth + 1,
            visited_intents: self.visited_intents.clone(),
        }
    }
}

