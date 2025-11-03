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
            eprintln!("  ✓ Found: {}", manifest.id);
            eprintln!("{}", "═".repeat(80));
            return Ok(DiscoveryResult::Found(manifest));
        }
        eprintln!("  ✗ Not found");
        
        // 2. Try MCP registry search
        eprintln!("  [2/4] Searching MCP registry...");
        if let Some(manifest) = self.search_mcp_registry(need).await? {
            eprintln!("  ✓ Found: {}", manifest.id);
            eprintln!("{}", "═".repeat(80));
            return Ok(DiscoveryResult::Found(manifest));
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
    async fn search_marketplace(&self, need: &CapabilityNeed) -> RuntimeResult<Option<CapabilityManifest>> {
        // First, try exact class match
        if let Some(manifest) = self.marketplace.get_capability(&need.capability_class).await {
            // Verify inputs/outputs compatibility
            if self.is_compatible(&manifest, need) {
                return Ok(Some(manifest));
            }
        }
        
        // TODO: Implement semantic search for approximate matches
        // For now, just return None if exact match not found
        
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
        
        // Search MCP registry for matching servers
        let servers = match registry_client.search_servers(&search_query).await {
            Ok(servers) => servers,
            Err(_) => {
                return Ok(None);
            }
        };
        
        // Introspect each server to find matching tools
        let introspector = crate::synthesis::mcp_introspector::MCPIntrospector::new();
        
        for server in &servers {
            // Try to get server URL from packages or remotes
            let server_url = server.remotes.as_ref()
                .and_then(|remotes| remotes.first())
                .map(|remote| remote.url.clone())
                .or_else(|| {
                    // Try to construct URL from packages if available
                    server.packages.as_ref()
                        .and_then(|packages| packages.first())
                        .and_then(|pkg| pkg.registry_base_url.clone())
                });
            
            if let Some(url) = server_url {
                // Check cache first if available
                let introspection_result = if let Some(ref cache) = self.introspection_cache {
                    match cache.get_mcp(&url) {
                        Ok(Some(cached)) => Ok(cached),
                        Ok(None) | Err(_) => {
                            // Cache miss or error - introspect the server
                            let result = introspector.introspect_mcp_server(&url, &server.name).await;
                            // Cache the result if successful
                            if let Ok(ref introspection) = result {
                                let _ = cache.put_mcp(&url, introspection);
                            }
                            result
                        }
                    }
                } else {
                    // No cache - just introspect
                    introspector.introspect_mcp_server(&url, &server.name).await
                };
                
                // Process the introspection result
                match introspection_result {
                    Ok(introspection) => {
                        // Create all capabilities from this server's tools
                        match introspector.create_capabilities_from_mcp(&introspection) {
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

