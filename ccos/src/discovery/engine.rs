//! Discovery engine for finding capabilities (no synthesis - that's delegated to planner)

use crate::capability_marketplace::types::CapabilityManifest;
use crate::capability_marketplace::CapabilityMarketplace;
use crate::cognitive_engine::delegating_engine::DelegatingCognitiveEngine;
use crate::discovery::config::DiscoveryConfig;
use crate::discovery::discovery_agent::DiscoveryAgent;
use crate::discovery::introspection_cache::IntrospectionCache;
use crate::discovery::need_extractor::CapabilityNeed;
// Note: RecursiveSynthesizer removed - synthesis is delegated to planner.synthesize_capability
use crate::intent_graph::IntentGraph;
use crate::synthesis::primitives::PrimitiveContext;
use crate::synthesis::schema_serializer::type_expr_to_rtfs_compact;
use crate::utils::value_conversion;
use regex;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use serde_json::Value as JsonValue;
use std::path::Path;
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
    delegating_arbiter: Option<Arc<DelegatingCognitiveEngine>>,
    /// Optional introspection cache for MCP/OpenAPI results
    introspection_cache: Option<Arc<IntrospectionCache>>,
    /// Configuration for discovery behavior
    config: DiscoveryConfig,
    /// Discovery agent for intelligent hint generation and ranking
    discovery_agent: Option<Arc<DiscoveryAgent>>,
}

impl DiscoveryEngine {
    /// Create a new discovery engine
    pub fn new(
        marketplace: Arc<CapabilityMarketplace>,
        intent_graph: Arc<Mutex<IntentGraph>>,
    ) -> Self {
        let config = DiscoveryConfig::from_env();
        let discovery_agent = Some(Arc::new(DiscoveryAgent::new(
            Arc::clone(&marketplace),
            config.clone(),
        )));
        if std::env::var("CCOS_DEBUG").is_ok() {
            crate::ccos_println!("🔍 DiscoveryEngine: Discovery agent initialized");
        }
        Self {
            marketplace,
            intent_graph,
            delegating_arbiter: None,
            introspection_cache: None,
            config,
            discovery_agent,
        }
    }

    /// Create a new discovery engine with AgentConfig
    pub fn new_with_agent_config(
        marketplace: Arc<CapabilityMarketplace>,
        intent_graph: Arc<Mutex<IntentGraph>>,
        agent_config: &crate::config::types::AgentConfig,
    ) -> Self {
        let config = DiscoveryConfig::from_agent_config(&agent_config.discovery);
        let discovery_agent = Some(Arc::new(DiscoveryAgent::new(
            Arc::clone(&marketplace),
            config.clone(),
        )));
        Self {
            marketplace,
            intent_graph,
            delegating_arbiter: None,
            introspection_cache: None,
            config,
            discovery_agent,
        }
    }

    /// Create a new discovery engine with delegating arbiter for recursive synthesis
    pub fn new_with_arbiter(
        marketplace: Arc<CapabilityMarketplace>,
        intent_graph: Arc<Mutex<IntentGraph>>,
        delegating_arbiter: Option<Arc<DelegatingCognitiveEngine>>,
    ) -> Self {
        let config = DiscoveryConfig::from_env();
        let discovery_agent = Some(Arc::new(DiscoveryAgent::new(
            Arc::clone(&marketplace),
            config.clone(),
        )));
        Self {
            marketplace,
            intent_graph,
            delegating_arbiter,
            introspection_cache: None,
            config,
            discovery_agent,
        }
    }

    /// Create a new discovery engine with delegating arbiter and AgentConfig
    pub fn new_with_arbiter_and_agent_config(
        marketplace: Arc<CapabilityMarketplace>,
        intent_graph: Arc<Mutex<IntentGraph>>,
        delegating_arbiter: Option<Arc<DelegatingCognitiveEngine>>,
        agent_config: &crate::config::types::AgentConfig,
    ) -> Self {
        let config = DiscoveryConfig::from_agent_config(&agent_config.discovery);
        let discovery_agent = Some(Arc::new(DiscoveryAgent::new(
            Arc::clone(&marketplace),
            config.clone(),
        )));
        Self {
            marketplace,
            intent_graph,
            delegating_arbiter,
            introspection_cache: None,
            config,
            discovery_agent,
        }
    }

    /// Create a discovery engine with custom configuration
    pub fn with_config(mut self, config: DiscoveryConfig) -> Self {
        self.config = config;
        self
    }

    /// Get the current configuration
    pub fn get_config(&self) -> &DiscoveryConfig {
        &self.config
    }

    /// Create a discovery engine with introspection cache
    pub fn with_cache(mut self, cache: Arc<IntrospectionCache>) -> Self {
        self.introspection_cache = Some(cache);
        self
    }

    /// Attempt to find a capability using the discovery priority chain
    pub async fn discover_capability(
        &self,
        need: &CapabilityNeed,
    ) -> RuntimeResult<DiscoveryResult> {
        // Enhance rationale if it's too generic (improves semantic matching)
        let enhanced_need = if need.rationale.starts_with("Need for capability:") {
            let enhanced_rationale =
                self.generate_enhanced_rationale(&need.capability_class, &need.rationale);
            CapabilityNeed::new(
                need.capability_class.clone(),
                need.required_inputs.clone(),
                need.expected_outputs.clone(),
                enhanced_rationale,
            )
        } else {
            need.clone()
        };

        let need = &enhanced_need;

        // Print capability section header
        crate::ccos_println!("\n{}", "═".repeat(80));
        crate::ccos_println!("🔍 DISCOVERY: {}", need.capability_class);
        crate::ccos_println!("{}", "─".repeat(80));
        crate::ccos_println!("  Rationale: {}", need.rationale);
        crate::ccos_println!("  Inputs: {:?}", need.required_inputs);
        crate::ccos_println!("  Outputs: {:?}", need.expected_outputs);
        crate::ccos_println!("{}", "─".repeat(80));

        // 1. Try local marketplace search first
        crate::ccos_println!("  [1/4] Searching local marketplace...");
        if let Some(manifest) = self.search_marketplace(need).await? {
            // Check if the capability is incomplete
            let is_incomplete = manifest
                .metadata
                .get("status")
                .map(|s| s == "incomplete")
                .unwrap_or(false);

            if is_incomplete {
                crate::ccos_println!("  ⚠️  Found incomplete capability: {}", manifest.id);
                crate::ccos_println!("{}", "═".repeat(80));
                return Ok(DiscoveryResult::Incomplete(manifest));
            } else {
                crate::ccos_println!("  ✓ Found: {}", manifest.id);
                crate::ccos_println!("{}", "═".repeat(80));
                return Ok(DiscoveryResult::Found(manifest));
            }
        }
        crate::ccos_println!("  ✗ Not found");

        // 2. Try MCP registry search (before local synthesis - MCP capabilities are real implementations)
        crate::ccos_println!("  [2/4] Searching MCP registry...");
        if let Some(manifest) = self.search_mcp_registry(need).await? {
            // Check if the capability is incomplete (shouldn't happen for MCP, but check anyway)
            let is_incomplete = manifest
                .metadata
                .get("status")
                .map(|s| s == "incomplete")
                .unwrap_or(false);

            // Save the discovered MCP capability to disk for persistence
            if let Err(e) = self.save_mcp_capability(&manifest).await {
                crate::ccos_println!("  ⚠️  Failed to save MCP capability to disk: {}", e);
            } else {
                crate::ccos_println!("  💾 Saved MCP capability to disk");
            }

            // Register the discovered MCP capability in marketplace for future searches
            if let Err(e) = self
                .marketplace
                .register_capability_manifest(manifest.clone())
                .await
            {
                crate::ccos_println!("  ⚠  Warning: Failed to register MCP capability: {}", e);
            } else {
                crate::ccos_println!("       Registered MCP capability in marketplace");
            }
            crate::ccos_println!("{}", "═".repeat(80));

            if is_incomplete {
                crate::ccos_println!("  ⚠️  Found incomplete MCP capability: {}", manifest.id);
                return Ok(DiscoveryResult::Incomplete(manifest));
            } else {
                crate::ccos_println!("  ✓ Found: {}", manifest.id);
                return Ok(DiscoveryResult::Found(manifest));
            }
        }
        crate::ccos_println!("  ✗ Not found");

        // Note: LocalSynthesizer (rule-based) has been removed.
        // Step 3 now relies on recursive synthesis which uses LLM-based approach.
        crate::ccos_println!(
            "  [3/4] Rule-based local synthesis removed, proceeding to recursive synthesis..."
        );

        // Note: Synthesis has been removed from discovery.
        // Discovery only finds existing capabilities (marketplace, MCP).
        // Missing capabilities are returned as NotFound for the planner
        // to handle via planner.synthesize_capability.
        crate::ccos_println!("  [4/4] Discovery complete - capability not found");
        crate::ccos_println!("{}", "═".repeat(80));
        crate::ccos_println!(
            "  ✗ Discovery failed for: {} (delegate to synthesis)",
            need.capability_class
        );
        crate::ccos_println!("{}", "═".repeat(80));

        // Not found - let planner handle synthesis
        Ok(DiscoveryResult::NotFound)
    }

    /// Search the local marketplace for a matching capability
    /// Uses hybrid matching: description-first (what it does), then name-based
    async fn search_marketplace(
        &self,
        need: &CapabilityNeed,
    ) -> RuntimeResult<Option<CapabilityManifest>> {
        // First, try exact class match
        if let Some(manifest) = self
            .marketplace
            .get_capability(&need.capability_class)
            .await
        {
            // Verify inputs/outputs compatibility
            if self.is_compatible(&manifest, need) {
                return Ok(Some(manifest));
            }
        }

        let all_capabilities = self.marketplace.list_capabilities().await;
        // Token-based matching: allow aliases like github.issues.list → mcp.github.github-mcp.list_issues
        let tokens: Vec<String> = need
            .capability_class
            .split(|c: char| c == '.' || c == '_' || c == '-')
            .filter(|tok| tok.len() > 1)
            .map(|tok| tok.to_ascii_lowercase())
            .collect();

        if !tokens.is_empty() {
            crate::ccos_println!(
                "  [TOKEN MATCH] Tokens for {} → {:?}",
                need.capability_class, tokens
            );
            for manifest in &all_capabilities {
                let haystack =
                    format!("{} {} {}", manifest.id, manifest.name, manifest.description)
                        .to_ascii_lowercase();

                if tokens.iter().all(|tok| haystack.contains(tok)) {
                    if self.is_compatible(manifest, need) {
                        crate::ccos_println!(
                            "  [TOKEN MATCH] {} matched manifest {}",
                            need.capability_class, manifest.id
                        );
                        return Ok(Some(manifest.clone()));
                    } else {
                        crate::ccos_println!(
                            "  [TOKEN MATCH] Candidate {} matched tokens but failed schema compatibility",
                            manifest.id
                        );
                    }
                }
            }
            crate::ccos_println!(
                "  [TOKEN MATCH] No compatible manifest found for {} using tokens {:?}",
                need.capability_class, tokens
            );
        }

        // Semantic search for approximate matches using description/rationale
        let mut best_match: Option<(CapabilityManifest, f64, String)> = None; // (manifest, score, match_type)
        let threshold = self.config.match_threshold;

        // Try embedding-based matching if enabled
        let mut embedding_service = if self.config.use_embeddings {
            crate::discovery::embedding_service::EmbeddingService::from_settings(Some(&self.config))
        } else {
            None
        };

        // First pass: description-based matching (what the capability does)
        for manifest in &all_capabilities {
            let desc_score = if let Some(ref mut emb_svc) = embedding_service {
                // Use embedding-based matching (more accurate)
                crate::catalog::matcher::calculate_description_match_score_with_embedding_async(
                    &need.rationale,
                    &manifest.description,
                    &manifest.name,
                    Some(emb_svc),
                )
                .await
            } else {
                // Use improved keyword-based matching with action verb awareness
                crate::catalog::matcher::calculate_description_match_score_improved(
                    &need.rationale,
                    &manifest.description,
                    &manifest.name,
                    &need.capability_class,
                    &manifest.id,
                    &self.config,
                )
            };

            // Debug logging for top candidates
            if desc_score >= (threshold * 0.7)
                || manifest.id.contains("github")
                || manifest.description.contains("issue")
            {
                crate::ccos_println!(
                    "  [DEBUG] Description match: {} → {} (score: {:.3})",
                    need.rationale, manifest.id, desc_score
                );
                crate::ccos_println!("         Need rationale: {}", need.rationale);
                crate::ccos_println!("         Manifest desc: {}", manifest.description);
            }

            if desc_score >= threshold {
                match &best_match {
                    Some((_, best_score, _)) if desc_score > *best_score => {
                        best_match =
                            Some((manifest.clone(), desc_score, "description".to_string()));
                    }
                    None => {
                        best_match =
                            Some((manifest.clone(), desc_score, "description".to_string()));
                    }
                    _ => {}
                }
            }
        }

        // Second pass: name-based matching (for cases where description is vague)
        // Use improved matching here too to ensure action verb validation
        for manifest in &all_capabilities {
            // For name-based matching, we still need to check description/rationale
            // but with lower weight since we're primarily matching on names
            let name_score = if let Some(ref mut emb_svc) = embedding_service {
                // Use embedding-based matching if available
                crate::catalog::matcher::calculate_description_match_score_with_embedding_async(
                    &need.rationale,
                    &manifest.description,
                    &manifest.name,
                    Some(emb_svc),
                )
                .await
            } else {
                // Use improved keyword-based matching with action verb awareness
                // This ensures "filter" doesn't match "assign" even if they share keywords
                crate::catalog::matcher::calculate_description_match_score_improved(
                    &need.rationale,
                    &manifest.description,
                    &manifest.name,
                    &need.capability_class,
                    &manifest.id,
                    &self.config,
                )
            };

            // Also calculate a name-only score for comparison
            let name_only_score = crate::catalog::matcher::calculate_semantic_match_score(
                &need.capability_class,
                &manifest.id,
                &manifest.name,
            );

            // Extract action verbs to check if they match
            let need_action_verbs = crate::catalog::matcher::extract_action_verbs(&need.rationale);
            let manifest_action_verbs = crate::catalog::matcher::extract_action_verbs(&format!(
                "{} {}",
                manifest.description, manifest.name
            ));
            let action_verb_score = crate::catalog::matcher::calculate_action_verb_match_score(
                &need_action_verbs,
                &manifest_action_verbs,
            );

            // If action verbs don't match, don't trust name-only score
            // The improved matching (name_score) already validates action verbs
            let final_score = if action_verb_score < self.config.action_verb_threshold
                && !need_action_verbs.is_empty()
            {
                // Action verbs don't match - trust only the improved matching score
                // which already penalizes action verb mismatches
                name_score
            } else {
                // Action verbs match or no action verbs specified - use the better score
                name_score.max(name_only_score * 0.8) // Slightly penalize name-only matches
            };

            if final_score >= threshold {
                match &best_match {
                    Some((_, best_score, _)) if final_score > *best_score => {
                        best_match = Some((manifest.clone(), final_score, "name".to_string()));
                    }
                    None => {
                        best_match = Some((manifest.clone(), final_score, "name".to_string()));
                    }
                    _ => {}
                }
            }
        }

        if let Some((manifest, score, match_type)) = best_match {
            crate::ccos_println!(
                "  ✓ Marketplace semantic match ({}): {} (score: {:.2})",
                match_type, manifest.id, score
            );
            return Ok(Some(manifest));
        }

        Ok(None)
    }

    // (helper functions moved to bottom of file)

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
        self.marketplace
            .search_by_id(&pattern)
            .await
            .into_iter()
            .take(max_examples)
            .collect()
    }

    /// Search MCP registry for a capability
    pub async fn search_mcp_registry(
        &self,
        need: &CapabilityNeed,
    ) -> RuntimeResult<Option<CapabilityManifest>> {
        crate::ccos_println!(
            "  → search_mcp_registry called for: {}",
            need.capability_class
        );
        crate::ccos_println!(
            "  → Discovery agent available: {}",
            self.discovery_agent.is_some()
        );

        // Use discovery agent if available to get intelligent suggestions
        let (keywords, registry_queries) = if let Some(ref agent) = self.discovery_agent {
            crate::ccos_println!("  → Using discovery agent for intelligent query generation...");
            // Learn vocabulary if not already done (lazy initialization)
            if let Err(e) = agent.learn_vocabulary().await {
                crate::ccos_println!("  ⚠️  Discovery agent vocabulary learning failed: {}", e);
            } else {
                crate::ccos_println!("  → Discovery agent vocabulary loaded");
            }

            // Get suggestions from agent (we'll use goal from rationale for now)
            // TODO: Pass actual goal and intent when available
            match agent.suggest(&need.rationale, None, need).await {
                Ok(suggestion) => {
                    crate::ccos_println!(
                        "  → Discovery agent suggested {} hint(s) and {} query(ies)",
                        suggestion.hints.len(),
                        suggestion.registry_queries.len()
                    );
                    if !suggestion.hints.is_empty() {
                        crate::ccos_println!("  → Hints: {:?}", suggestion.hints);
                    }
                    if !suggestion.registry_queries.is_empty() {
                        crate::ccos_println!("  → Registry queries: {:?}", suggestion.registry_queries);
                    }

                    // Use first registry query as primary, extract keywords from it
                    let primary_query = suggestion
                        .registry_queries
                        .first()
                        .cloned()
                        .unwrap_or_else(|| need.capability_class.clone());
                    let keywords: Vec<String> =
                        primary_query.split_whitespace().map(String::from).collect();
                    (keywords, suggestion.registry_queries)
                }
                Err(e) => {
                    crate::ccos_println!("  ⚠️  Discovery agent suggestion failed: {}, falling back to legacy method", e);
                    // Fallback to old method if agent fails
                    let mut keywords: Vec<String> =
                        need.capability_class.split('.').map(String::from).collect();
                    let context_tokens = extract_context_tokens(&need.rationale);
                    for token in context_tokens {
                        if !keywords.iter().any(|k| k == &token) {
                            keywords.insert(0, token);
                        }
                    }
                    let search_query = keywords.join(" ");
                    (keywords, vec![search_query])
                }
            }
        } else {
            crate::ccos_println!("  → Discovery agent not available, using legacy extraction method");
            // Fallback: use old extraction method
            let mut keywords: Vec<String> =
                need.capability_class.split('.').map(String::from).collect();
            let context_tokens = extract_context_tokens(&need.rationale);
            for token in context_tokens {
                if !keywords.iter().any(|k| k == &token) {
                    keywords.insert(0, token);
                }
            }
            let search_query = keywords.join(" ");
            (keywords, vec![search_query])
        };

        // Use MCP registry client to search for servers
        let registry_client = crate::mcp::registry::MCPRegistryClient::new();

        let search_query = keywords.join(" "); // Use space-separated keywords for search

        crate::ccos_println!("  → MCP registry search query: '{}'", search_query);

        // First, check curated overrides (capabilities/mcp/overrides.json)
        let curated_servers = self.load_curated_overrides_for(&need.capability_class)?;
        let mut servers = if !curated_servers.is_empty() {
            crate::ccos_println!(
                "  → Found {} curated override(s) for '{}'",
                curated_servers.len(),
                need.capability_class
            );
            curated_servers
        } else {
            Vec::new()
        };

        // Then search MCP registry for matching servers
        let registry_servers = match registry_client.search_servers(&search_query).await {
            Ok(registry_servers) => {
                crate::ccos_println!(
                    "  → Found {} MCP server(s) from registry for '{}'",
                    registry_servers.len(),
                    search_query
                );
                registry_servers
            }
            Err(e) => {
                crate::ccos_println!("  → MCP registry search failed: {}", e);
                crate::ccos_println!("     ⚠️  Could not connect to MCP registry or search failed");
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

        // If no servers found with full query, try agent-suggested fallback queries
        if servers.is_empty() && !registry_queries.is_empty() {
            // Try remaining registry queries from agent
            for query in registry_queries.iter().skip(1).take(3) {
                crate::ccos_println!(
                    "  → No servers found, trying agent-suggested query: '{}'",
                    query
                );
                if let Ok(fallback_servers) = registry_client.search_servers(query).await {
                    if !fallback_servers.is_empty() {
                        crate::ccos_println!(
                            "  → Found {} MCP server(s) for '{}'",
                            fallback_servers.len(),
                            query
                        );
                        servers.extend(fallback_servers);
                        break; // Stop after first successful query
                    }
                }
            }
        }

        // If still no servers, try progressively simpler queries (legacy fallback)
        // e.g., if "text filter by-content" finds nothing, try "text filter", then "filter"
        // This avoids matching completely unrelated servers like "textarttools" when searching for "text.filter"
        if servers.is_empty() && !keywords.is_empty() {
            // Try with 2 keywords first (more specific than just one)
            if keywords.len() >= 2 {
                let two_keyword_query = format!("{} {}", keywords[0], keywords[1]);
                crate::ccos_println!(
                    "  → No servers found, trying simpler query: '{}'",
                    two_keyword_query
                );
                if let Ok(fallback_servers) =
                    registry_client.search_servers(&two_keyword_query).await
                {
                    crate::ccos_println!(
                        "  → Found {} MCP server(s) for '{}'",
                        fallback_servers.len(),
                        two_keyword_query
                    );
                    servers.extend(fallback_servers);
                }
            }

            // If still no servers, try with just the most relevant keyword (usually the action word)
            // For "text.filter.by-content", prefer "filter" over "text"
            if servers.is_empty() && keywords.len() >= 2 {
                // Use the last keyword (usually the action word) instead of first
                let action_keyword = keywords.last().unwrap();
                crate::ccos_println!(
                    "  → No servers found, trying action keyword: '{}'",
                    action_keyword
                );
                let fallback_servers = match registry_client.search_servers(action_keyword).await {
                    Ok(fallback_servers) => {
                        crate::ccos_println!(
                            "  → Found {} MCP server(s) for '{}'",
                            fallback_servers.len(),
                            action_keyword
                        );
                        fallback_servers
                    }
                    Err(_) => Vec::new(),
                };
                servers.extend(fallback_servers);
            }

            // If still no servers, try the first keyword (often the context/platform)
            if servers.is_empty() {
                let first_keyword = &keywords[0];
                crate::ccos_println!(
                    "  → No servers found, trying first keyword: '{}'",
                    first_keyword
                );
                let fallback_servers = match registry_client.search_servers(first_keyword).await {
                    Ok(fallback_servers) => {
                        crate::ccos_println!(
                            "  → Found {} MCP server(s) for '{}'",
                            fallback_servers.len(),
                            first_keyword
                        );
                        fallback_servers
                    }
                    Err(_) => Vec::new(),
                };
                servers.extend(fallback_servers);
            }
        }

        // Filter servers by description relevance before introspection
        // This avoids introspecting completely unrelated servers when fallback query is too broad
        let total_before_filter = servers.len();
        if servers.len() > 5 && !keywords.is_empty() {
            // If we have many servers from a broad fallback query, filter by description relevance
            let need_keywords: Vec<String> = keywords.iter().map(|k| k.to_lowercase()).collect();
            let need_rationale_lower = need.rationale.to_lowercase();

            servers.retain(|server| {
                let server_desc_lower = server.description.to_lowercase();
                let server_name_lower = server.name.to_lowercase();

                // Check if server description/name contains any of our keywords (using whole word matching for short keywords)
                let has_keyword_match = need_keywords.iter().any(|kw| {
                    if kw.len() <= 2 {
                        // For very short keywords like "ui", use whole word matching
                        let pattern = format!(r"\b{}\b", regex::escape(kw));
                        if let Ok(re) = regex::Regex::new(&pattern) {
                            re.is_match(&server_desc_lower) || re.is_match(&server_name_lower)
                        } else {
                            server_desc_lower.contains(kw) || server_name_lower.contains(kw)
                        }
                    } else {
                        server_desc_lower.contains(kw) || server_name_lower.contains(kw)
                    }
                });

                // Also check if description relates to our rationale (basic keyword overlap)
                // Filter out common boilerplate words from rationale to avoid false positives
                let rationale_words: Vec<&str> = need_rationale_lower
                    .split_whitespace()
                    .filter(|w| {
                        let w = w.trim_matches(|c: char| !c.is_alphanumeric());
                        w != "need" && w != "step" && w != "capability" && w != "for"
                    })
                    .collect();
                let has_rationale_match = rationale_words.iter().any(|word| {
                    word.len() > 3
                        && (server_desc_lower.contains(word) || server_name_lower.contains(word))
                });

                has_keyword_match || has_rationale_match
            });

            if servers.len() < total_before_filter {
                crate::ccos_println!(
                    "  → Filtered to {} relevant server(s) based on description matching (from {})",
                    servers.len(),
                    total_before_filter
                );
            }
        }

        if servers.is_empty() {
            crate::ccos_println!("     ⚠️  No MCP servers found in registry");
            crate::ccos_println!("     💡 The MCP registry may not have GitHub servers configured");
            crate::ccos_println!("     💡 Alternative: Use known MCP server URLs directly");
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
            crate::ccos_println!("  → Searching {} MCP server(s)...", servers.len());
        }

        for server in servers.iter() {
            // Try to get server URL from remotes first, then check for environment variable overrides
            let mut server_url = server.remotes.as_ref().and_then(|remotes| {
                crate::mcp::registry::MCPRegistryClient::select_best_remote_url(remotes)
            });

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
                    format!("{}_MCP_URL", server.name.replace("-", "_").to_uppercase())
                };

                // Also check generic MCP_SERVER_URL and alternative formats
                let env_vars_to_check = vec![
                    env_var_name.clone(),
                    "MCP_SERVER_URL".to_string(),
                    format!(
                        "{}_URL",
                        server
                            .name
                            .replace("/", "_")
                            .replace("-", "_")
                            .to_uppercase()
                    ),
                ];

                for env_var in env_vars_to_check {
                    if let Ok(url) = std::env::var(&env_var) {
                        if !url.is_empty() {
                            crate::ccos_println!(
                                "     → Found server URL from environment: {} = {}",
                                env_var, url
                            );
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
                        crate::ccos_println!("     ⚠️  No remote URL found (stdio-based server, requires local npm package)");
                        if let Some(ref packages) = server.packages {
                            if let Some(pkg) = packages.first() {
                                crate::ccos_println!(
                                    "     → Package: {}@{} (registry: {})",
                                    pkg.identifier,
                                    pkg.version.as_ref().unwrap_or(&"latest".to_string()),
                                    pkg.registry_base_url
                                        .as_ref()
                                        .unwrap_or(&"unknown".to_string())
                                );
                                let suggested_env_var =
                                    if let Some(slash_pos) = server.name.find('/') {
                                        let namespace = &server.name[..slash_pos];
                                        format!(
                                            "{}_MCP_URL",
                                            namespace.replace("-", "_").to_uppercase()
                                        )
                                    } else {
                                        format!(
                                            "{}_MCP_URL",
                                            server.name.replace("-", "_").to_uppercase()
                                        )
                                    };
                                crate::ccos_println!("     💡 Set {} environment variable to point to a remote MCP endpoint", suggested_env_var);
                                crate::ccos_println!("     💡 Or add a 'remotes' entry to overrides.json with an HTTP/HTTPS URL");
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
                        crate::ccos_println!("     ⚠️  Skipping: WebSocket URLs not supported for HTTP-based introspection");
                        crate::ccos_println!("     → URL: {}", url);
                    }
                    continue;
                }

                // Only support HTTP/HTTPS for introspection (mcp:// is also valid but less common)
                if !url.starts_with("http://")
                    && !url.starts_with("https://")
                    && !url.starts_with("mcp://")
                {
                    stats.skipped_invalid += 1;
                    if servers.len() == 1 {
                        crate::ccos_println!(
                            "     ⚠️  Skipping: Invalid URL scheme (expected http/https): {}",
                            url
                        );
                    }
                    continue;
                }

                // Filter out common repository URLs that aren't MCP endpoints
                if url.contains("github.com/") && !url.contains("/api/") && !url.contains("mcp") {
                    stats.skipped_invalid += 1;
                    if servers.len() == 1 {
                        crate::ccos_println!("     ⚠️  Skipping: Appears to be a repository URL, not an MCP endpoint");
                        crate::ccos_println!("     → URL: {}", url);
                    }
                    continue;
                }

                // Only show detailed URL for single server
                if servers.len() == 1 {
                    crate::ccos_println!("     → Server: {} ({})", server.name, url);
                }

                // Build auth headers from environment (if available)
                // Generic approach: works for any MCP server
                // Priority: {NAMESPACE}_MCP_TOKEN > MCP_AUTH_TOKEN
                let mut auth_headers = std::collections::HashMap::new();
                let token = self.get_mcp_auth_token(&server.name);

                if let Some(token) = token {
                    // All MCP servers (including GitHub Copilot) use standard Authorization: Bearer
                    auth_headers.insert("Authorization".to_string(), format!("Bearer {}", token));
                    if servers.len() == 1 {
                        crate::ccos_println!("     ✓ Using authentication token from environment");
                        // Show which env var was used (without revealing token value)
                        let env_var_used = if std::env::var("GITHUB_MCP_TOKEN").is_ok() {
                            "GITHUB_MCP_TOKEN"
                        } else if std::env::var("MCP_AUTH_TOKEN").is_ok() {
                            "MCP_AUTH_TOKEN"
                        } else if std::env::var("GITHUB_PAT").is_ok() {
                            "GITHUB_PAT"
                        } else if std::env::var("GITHUB_TOKEN").is_ok() {
                            "GITHUB_TOKEN"
                        } else {
                            "unknown"
                        };
                        crate::ccos_println!("     → Token source: {}", env_var_used);
                        crate::ccos_println!(
                            "     → Using Authorization: Bearer header (standard MCP format)"
                        );
                    }
                } else if servers.len() == 1 {
                    crate::ccos_println!("     ⚠️  No authentication token found in environment");
                    let suggested_var = self.suggest_mcp_token_env_var(&server.name);
                    crate::ccos_println!(
                        "     💡 Set {} or MCP_AUTH_TOKEN for authenticated MCP servers",
                        suggested_var
                    );
                }

                let auth_headers_for_schema = if auth_headers.is_empty() {
                    None
                } else {
                    Some(auth_headers.clone())
                };

                // Check cache first if available
                let introspection_result = if let Some(ref cache) = self.introspection_cache {
                    match cache.get_mcp(&url) {
                        Ok(Some(cached)) => {
                            stats.cached += 1;
                            stats.tools_found += cached.tools.len();
                            if servers.len() == 1 {
                                crate::ccos_println!(
                                    "     ✓ Using cached introspection ({} tools)",
                                    cached.tools.len()
                                );
                            }
                            Ok(cached)
                        }
                        Ok(None) | Err(_) => {
                            // Cache miss - introspect the server with auth
                            let result = if auth_headers.is_empty() {
                                introspector.introspect_mcp_server(&url, &server.name).await
                            } else {
                                introspector
                                    .introspect_mcp_server_with_auth(
                                        &url,
                                        &server.name,
                                        Some(auth_headers.clone()),
                                    )
                                    .await
                            };
                            // Cache the result if successful
                            match &result {
                                Ok(introspection) => {
                                    stats.introspected += 1;
                                    stats.tools_found += introspection.tools.len();
                                    if servers.len() == 1 {
                                        crate::ccos_println!(
                                            "     ✓ Introspected successfully ({} tools)",
                                            introspection.tools.len()
                                        );
                                    }
                                    let _ = cache.put_mcp(&url, introspection);
                                }
                                Err(_) => {
                                    stats.failed += 1;
                                    if servers.len() == 1 {
                                        crate::ccos_println!("     ✗ Introspection failed");
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
                        introspector
                            .introspect_mcp_server_with_auth(
                                &url,
                                &server.name,
                                Some(auth_headers.clone()),
                            )
                            .await
                    };
                    match &result {
                        Ok(introspection) => {
                            stats.introspected += 1;
                            stats.tools_found += introspection.tools.len();
                            if servers.len() == 1 {
                                crate::ccos_println!(
                                    "     ✓ Introspected successfully ({} tools)",
                                    introspection.tools.len()
                                );
                            }
                        }
                        Err(_) => {
                            stats.failed += 1;
                            if servers.len() == 1 {
                                crate::ccos_println!("     ✗ Introspection failed");
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
                                let mut best_match: Option<(CapabilityManifest, f64, String)> =
                                    None; // (manifest, score, match_type)
                                let threshold = self.config.match_threshold;

                                // First pass: description-based semantic matching (what the capability does)
                                // This is better because LLM generates rationale/description, not exact names
                                // Try embedding-based matching if available, fallback to improved keyword-based
                                let mut embedding_service = if self.config.use_embeddings {
                                    crate::discovery::embedding_service::EmbeddingService::from_settings(Some(&self.config))
                                } else {
                                    None
                                };

                                // Extract semantic terms from goal/rationale for better matching
                                let goal_text =
                                    format!("{} {}", need.rationale, need.capability_class);
                                // Use agent's semantic term extraction (public static method)
                                let semantic_terms = crate::discovery::discovery_agent::DiscoveryAgent::extract_semantic_terms(&goal_text);

                                if !semantic_terms.is_empty() {
                                    crate::ccos_println!(
                                        "  → Using semantic terms for tool matching: {:?}",
                                        semantic_terms
                                    );
                                }

                                for manifest in &capabilities {
                                    let mut desc_score = if let Some(ref mut emb_svc) =
                                        embedding_service
                                    {
                                        // Use embedding-based matching (more accurate)
                                        crate::catalog::matcher::calculate_description_match_score_with_embedding_async(
                                            &need.rationale,
                                            &manifest.description,
                                            &manifest.name,
                                            Some(emb_svc),
                                        ).await
                                    } else {
                                        // Use improved keyword-based matching with action verb awareness
                                        crate::catalog::matcher::calculate_description_match_score_improved(
                                            &need.rationale,
                                            &manifest.description,
                                            &manifest.name,
                                            &need.capability_class,
                                            &manifest.id,
                                            &self.config,
                                        )
                                    };

                                    // Boost score if semantic terms match capability name/description
                                    if !semantic_terms.is_empty() {
                                        let capability_text = format!(
                                            "{} {} {}",
                                            manifest.id, manifest.name, manifest.description
                                        )
                                        .to_lowercase();
                                        let semantic_matches: usize = semantic_terms
                                            .iter()
                                            .filter(|term| capability_text.contains(term.as_str()))
                                            .count();

                                        if semantic_matches > 0 {
                                            // Strong boost: 0.2-0.5 depending on match count
                                            let semantic_boost =
                                                (semantic_matches as f64 * 0.15).min(0.5);
                                            desc_score += semantic_boost;
                                            crate::ccos_println!("  → Boosted '{}' by {:.2} for semantic term matches ({}/{})", 
                                                manifest.name, semantic_boost, semantic_matches, semantic_terms.len());
                                        }
                                    }

                                    if desc_score >= threshold {
                                        match &best_match {
                                            Some((_, best_score, _))
                                                if desc_score > *best_score =>
                                            {
                                                best_match = Some((
                                                    manifest.clone(),
                                                    desc_score,
                                                    "description".to_string(),
                                                ));
                                            }
                                            None => {
                                                best_match = Some((
                                                    manifest.clone(),
                                                    desc_score,
                                                    "description".to_string(),
                                                ));
                                            }
                                            _ => {}
                                        }
                                    }
                                }

                                // Second pass: name-based semantic matching (for cases where description is vague)
                                // Use improved matching here too to ensure action verb validation
                                for manifest in &capabilities {
                                    // Use improved matching with action verb awareness
                                    // This ensures "filter" doesn't match "assign" even if they share keywords
                                    let name_score = if let Some(ref mut emb_svc) =
                                        embedding_service
                                    {
                                        // Use embedding-based matching if available
                                        crate::catalog::matcher::calculate_description_match_score_with_embedding_async(
                                            &need.rationale,
                                            &manifest.description,
                                            &manifest.name,
                                            Some(emb_svc),
                                        ).await
                                    } else {
                                        // Use improved keyword-based matching with action verb awareness
                                        crate::catalog::matcher::calculate_description_match_score_improved(
                                            &need.rationale,
                                            &manifest.description,
                                            &manifest.name,
                                            &need.capability_class,
                                            &manifest.id,
                                            &self.config,
                                        )
                                    };

                                    // Also calculate a name-only score for comparison
                                    let name_only_score =
                                        crate::catalog::matcher::calculate_semantic_match_score(
                                            &need.capability_class,
                                            &manifest.id,
                                            &manifest.name,
                                        );

                                    // Extract action verbs to check if they match
                                    let need_action_verbs =
                                        crate::catalog::matcher::extract_action_verbs(
                                            &need.rationale,
                                        );
                                    let manifest_action_verbs =
                                        crate::catalog::matcher::extract_action_verbs(&format!(
                                            "{} {}",
                                            manifest.description, manifest.name
                                        ));
                                    let action_verb_score =
                                        crate::catalog::matcher::calculate_action_verb_match_score(
                                            &need_action_verbs,
                                            &manifest_action_verbs,
                                        );

                                    // If action verbs don't match, don't trust name-only score
                                    // The improved matching (name_score) already validates action verbs
                                    let final_score = if action_verb_score
                                        < self.config.action_verb_threshold
                                        && !need_action_verbs.is_empty()
                                    {
                                        // Action verbs don't match - trust only the improved matching score
                                        // which already penalizes action verb mismatches
                                        name_score
                                    } else {
                                        // Action verbs match or no action verbs specified - use the better score
                                        name_score.max(name_only_score * 0.8) // Slightly penalize name-only matches
                                    };

                                    if final_score >= threshold {
                                        match &best_match {
                                            Some((_, best_score, _))
                                                if final_score > *best_score =>
                                            {
                                                best_match = Some((
                                                    manifest.clone(),
                                                    final_score,
                                                    "name".to_string(),
                                                ));
                                            }
                                            None => {
                                                best_match = Some((
                                                    manifest.clone(),
                                                    final_score,
                                                    "name".to_string(),
                                                ));
                                            }
                                            _ => {}
                                        }
                                    }
                                }

                                // Return the best match if found
                                if let Some((mut manifest, score, match_type)) = best_match {
                                    stats.matched_servers.push(server.name.clone());
                                    self.enrich_manifest_output_schema(
                                        &mut manifest,
                                        &introspection,
                                        &introspector,
                                        &url,
                                        &server.name,
                                        &auth_headers_for_schema,
                                        servers.len() == 1,
                                    )
                                    .await;
                                    if servers.len() == 1 {
                                        crate::ccos_println!(
                                            "  ✓ Semantic match found ({}): {} (score: {:.2})",
                                            match_type, manifest.id, score
                                        );
                                    }
                                    return Ok(Some(manifest));
                                }

                                // Fallback to simple substring matching for compatibility
                                let capability_name_parts: Vec<&str> =
                                    need.capability_class.split('.').collect();
                                let last_part = capability_name_parts.last().unwrap_or(&"");

                                let fallback_action_verbs =
                                    crate::catalog::matcher::extract_action_verbs(&need.rationale);

                                for manifest in &capabilities {
                                    let manifest_id_lower = manifest.id.to_lowercase();
                                    let manifest_name_lower = manifest.name.to_lowercase();
                                    let manifest_desc_lower = manifest.description.to_lowercase();

                                    let verb_match = if fallback_action_verbs.is_empty() {
                                        true
                                    } else {
                                        fallback_action_verbs.iter().any(|verb| {
                                            manifest_id_lower.contains(verb)
                                                || manifest_name_lower.contains(verb)
                                                || manifest_desc_lower.contains(verb)
                                        })
                                    };

                                    // Check if capability ID or name matches
                                    let capability_match =
                                        capability_name_parts.iter().any(|part| {
                                            manifest_id_lower.contains(&part.to_lowercase())
                                                || manifest_name_lower
                                                    .contains(&part.to_lowercase())
                                        }) || manifest_id_lower.contains(&last_part.to_lowercase())
                                            || manifest_name_lower
                                                .contains(&last_part.to_lowercase())
                                            || manifest_desc_lower
                                                .contains(&last_part.to_lowercase());

                                    let action_token = last_part.to_lowercase();
                                    let action_matches = if action_token.is_empty() {
                                        true
                                    } else {
                                        manifest_id_lower.contains(&action_token)
                                            || manifest_name_lower.contains(&action_token)
                                            || manifest_desc_lower.contains(&action_token)
                                    };

                                    if capability_match {
                                        // Extra guard: ensure the manifest can plausibly satisfy the need
                                        // based on declared input/output schemas to avoid bogus matches
                                        if !verb_match
                                            || !action_matches
                                            || !manifest_satisfies_need(manifest, need)
                                        {
                                            // Skip this fallback match; try other manifests
                                            continue;
                                        }
                                        stats.matched_servers.push(server.name.clone());
                                        if servers.len() == 1 {
                                            crate::ccos_println!("  ✓ Substring match found: {}", manifest.id);
                                        }
                                        let mut matched_manifest = manifest.clone();
                                        self.enrich_manifest_output_schema(
                                            &mut matched_manifest,
                                            &introspection,
                                            &introspector,
                                            &url,
                                            &server.name,
                                            &auth_headers_for_schema,
                                            servers.len() == 1,
                                        )
                                        .await;
                                        return Ok(Some(matched_manifest));
                                    }
                                }
                            }
                            Err(e) => {
                                if servers.len() == 1 {
                                    crate::ccos_println!(
                                        "     ✗ Failed to create capabilities from MCP: {}",
                                        e
                                    );
                                }
                            }
                        }
                    }
                    Err(e) => {
                        if servers.len() == 1 {
                            crate::ccos_println!("     ✗ Server introspection failed: {}", e);
                        }
                    }
                }
            }
        }

        // Print summary for multiple servers
        if stats.total_servers > 1 {
            crate::ccos_println!("  → Summary: {} server(s) searched", stats.total_servers);
            if stats.introspected > 0 {
                crate::ccos_println!(
                    "     • {} introspected successfully ({} tools)",
                    stats.introspected, stats.tools_found
                );
            }
            if stats.cached > 0 {
                crate::ccos_println!("     • {} from cache", stats.cached);
            }
            if stats.failed > 0 {
                crate::ccos_println!("     • {} failed", stats.failed);
            }
            if stats.skipped_no_url > 0 {
                crate::ccos_println!("     • {} skipped (no remote URL)", stats.skipped_no_url);
            }
            if stats.skipped_websocket > 0 {
                crate::ccos_println!(
                    "     • {} skipped (WebSocket not supported)",
                    stats.skipped_websocket
                );
            }
            if stats.skipped_invalid > 0 {
                crate::ccos_println!("     • {} skipped (invalid URL)", stats.skipped_invalid);
            }
            if !stats.matched_servers.is_empty() {
                crate::ccos_println!("     • Matched: {}", stats.matched_servers.join(", "));
            } else {
                crate::ccos_println!("     ✗ No match found");
            }
        } else if stats.total_servers == 1 {
            crate::ccos_println!("  → No match found");
        }

        Ok(None)
    }

    /// Search OpenAPI services for a capability using web search
    pub async fn search_openapi(
        &self,
        need: &CapabilityNeed,
    ) -> RuntimeResult<Option<CapabilityManifest>> {
        // Use web search to find actual OpenAPI specs online
        let mut web_searcher =
            crate::synthesis::web_search_discovery::WebSearchDiscovery::new("auto".to_string());

        // Search for the capability
        let search_results = match web_searcher
            .search_for_api_specs(&need.capability_class)
            .await
        {
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

        for result in search_results.iter().take(5) {
            // Limit to top 5 results
            // Extract base URL from the result URL
            let base_url = self.extract_base_url_from_result(&result.url);

            // Check cache first if available
            let introspection_result = if let Some(ref cache) = self.introspection_cache {
                match cache.get_openapi(&base_url) {
                    Ok(Some(cached)) => Ok(cached),
                    Ok(None) | Err(_) => {
                        // Cache miss or error - introspect from discovery
                        let result_introspection = introspector
                            .introspect_from_discovery(&base_url, &need.capability_class)
                            .await;
                        // Cache the result if successful
                        if let Ok(ref introspection) = result_introspection {
                            let _ = cache.put_openapi(&base_url, introspection);
                        }
                        result_introspection
                    }
                }
            } else {
                // No cache - just introspect
                introspector
                    .introspect_from_discovery(&base_url, &need.capability_class)
                    .await
            };

            // Process the introspection result
            match introspection_result {
                Ok(introspection) => {
                    // Create capabilities from introspection
                    match introspector.create_capabilities_from_introspection(&introspection) {
                        Ok(capabilities) => {
                            // Find a matching capability
                            let capability_name_parts: Vec<&str> =
                                need.capability_class.split('.').collect();
                            let last_part = capability_name_parts.last().unwrap_or(&"");

                            for manifest in capabilities {
                                let manifest_id_lower = manifest.id.to_lowercase();
                                let manifest_name_lower = manifest.name.to_lowercase();

                                // Check if capability ID or name matches
                                let capability_match = capability_name_parts.iter().any(|part| {
                                    manifest_id_lower.contains(&part.to_lowercase())
                                        || manifest_name_lower.contains(&part.to_lowercase())
                                }) || manifest_id_lower
                                    .contains(&last_part.to_lowercase())
                                    || manifest_name_lower.contains(&last_part.to_lowercase());

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
                    return format!(
                        "{}://{}{}",
                        parsed_url.scheme(),
                        parsed_url.host_str().unwrap_or(""),
                        base_path
                    );
                } else if let Some(base_path) = path.strip_suffix("/openapi.json") {
                    return format!(
                        "{}://{}{}",
                        parsed_url.scheme(),
                        parsed_url.host_str().unwrap_or(""),
                        base_path
                    );
                }
            }
            // For other paths, use the origin
            format!(
                "{}://{}",
                parsed_url.scheme(),
                parsed_url.host_str().unwrap_or("")
            )
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
        let stub_handler: Arc<
            dyn Fn(&rtfs::runtime::values::Value) -> RuntimeResult<rtfs::runtime::values::Value>
                + Send
                + Sync,
        > = Arc::new(
            move |_input: &rtfs::runtime::values::Value| -> RuntimeResult<rtfs::runtime::values::Value> {
                Err(RuntimeError::Generic(format!(
                    "Capability {} is marked as incomplete/not_found and needs implementation",
                    capability_id
                )))
            },
        );

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
        manifest
            .metadata
            .insert("status".to_string(), "incomplete".to_string());
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
    ) -> RuntimeResult<Vec<crate::mcp::registry::McpServer>> {
        use std::fs;

        // Define the override file structure
        #[derive(serde::Deserialize)]
        struct CuratedOverrides {
            pub entries: Vec<CuratedEntry>,
        }

        #[derive(serde::Deserialize)]
        struct CuratedEntry {
            pub matches: Vec<String>,
            pub server: crate::mcp::registry::McpServer,
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
        let pattern_norm = pattern.to_ascii_lowercase();
        let text_norm = text.to_ascii_lowercase();

        if pattern_norm == text_norm {
            return true;
        }
        if pattern_norm.ends_with(".*") {
            let namespace = &pattern_norm[..pattern_norm.len() - 2];
            return text_norm == namespace || text_norm.starts_with(&format!("{}.", namespace));
        }
        if pattern_norm.ends_with('*') {
            let prefix = &pattern_norm[..pattern_norm.len() - 1];
            return text_norm.starts_with(prefix);
        }
        if pattern_norm.starts_with('*') {
            let suffix = &pattern_norm[1..];
            return text_norm.ends_with(suffix);
        }
        if pattern_norm.contains('*') {
            let parts: Vec<&str> = pattern_norm.split('*').collect();
            if parts.len() == 2 {
                return text_norm.starts_with(parts[0]) && text_norm.ends_with(parts[1]);
            }
        }
        text_norm.contains(&pattern_norm)
    }

    /// Collect discovery hints for all capabilities in a plan
    /// Returns hints about found capabilities, missing capabilities, and suggestions
    pub async fn collect_discovery_hints(
        &self,
        capability_ids: &[String],
    ) -> RuntimeResult<DiscoveryHints> {
        self.collect_discovery_hints_with_descriptions(
            &capability_ids
                .iter()
                .map(|id| (id.clone(), None))
                .collect::<Vec<_>>(),
        )
        .await
    }

    /// Collect discovery hints for capabilities with optional descriptions
    /// Uses provided descriptions (from LLM) as rationale when available
    pub async fn collect_discovery_hints_with_descriptions(
        &self,
        capability_info: &[(String, Option<String>)],
    ) -> RuntimeResult<DiscoveryHints> {
        let mut found = Vec::new();
        let mut missing = Vec::new();
        let mut suggestions = Vec::new();

        for (cap_id, description) in capability_info {
            // Use provided description if available, otherwise generate one
            let rationale = if let Some(desc) = description {
                // LLM provided a description - use it directly for semantic matching
                desc.clone()
            } else {
                // No description provided - enhance the capability class name
                self.generate_enhanced_rationale(
                    cap_id,
                    &format!("Need for capability: {}", cap_id),
                )
            };

            // Create a minimal CapabilityNeed for this capability ID
            let need = CapabilityNeed::new(
                cap_id.clone(),
                Vec::new(), // Don't know inputs yet
                Vec::new(), // Don't know outputs yet
                rationale,
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

                let common_keywords: Vec<&str> = found_keywords
                    .iter()
                    .filter(|k| missing_keywords.contains(k) && k.len() > 2)
                    .copied()
                    .collect();

                if !common_keywords.is_empty() && !found_cap.hints.is_empty() {
                    suggestions.push(format!(
                        "{} not found, but {} (found) might help: {}",
                        missing_id, found_cap.id, found_cap.hints[0]
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

        // Extract parameter usage hints from input schema
        if let Some(ref schema) = manifest.input_schema {
            let param_hints = self.extract_parameter_usage_hints(schema);
            hints.extend(param_hints);
        }

        if manifest.metadata.get("primitive_kind").is_some() {
            if let Some(primitive_hint) = manifest.metadata.get("primitive_kind") {
                hints.push(format!("Synthesized primitive: {}", primitive_hint));
            }

            let annotations = Self::primitive_annotations_value(manifest);
            let required_inputs = Self::schema_bindings(manifest.input_schema.as_ref());
            let expected_outputs = Self::schema_bindings(manifest.output_schema.as_ref());

            if !required_inputs.is_empty()
                || !expected_outputs.is_empty()
                || annotations != JsonValue::Null
            {
                let primitive_need = CapabilityNeed::new(
                    manifest.id.clone(),
                    required_inputs.clone(),
                    expected_outputs.clone(),
                    manifest.description.clone(),
                )
                .with_annotations(annotations.clone())
                .with_schemas(
                    manifest.input_schema.clone(),
                    manifest.output_schema.clone(),
                );

                let ctx = PrimitiveContext::from_manifest(&primitive_need, manifest, annotations);

                if !ctx.input_schemas.is_empty() {
                    let bindings: Vec<String> = ctx
                        .input_schemas
                        .keys()
                        .map(|binding| binding.trim_start_matches(':').to_string())
                        .collect();
                    if !bindings.is_empty() {
                        hints.push(format!("Inputs required: {}", bindings.join(", ")));
                    }
                }

                if let Some(metadata) = Self::primitive_metadata_value(manifest) {
                    hints.extend(Self::primitive_metadata_hints(&metadata));
                }
            }
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
            // Extract parameter hints from MCP tool description
            let param_hints = self.extract_parameter_hints_from_mcp_description(desc);
            hints.extend(param_hints);
        }

        hints
    }

    /// Extract parameter usage hints from a TypeExpr schema
    fn extract_parameter_usage_hints(&self, expr: &rtfs::ast::TypeExpr) -> Vec<String> {
        let mut hints = Vec::new();

        match expr {
            rtfs::ast::TypeExpr::Map { entries, .. } => {
                for entry in entries {
                    let param_name = value_conversion::map_key_to_string(
                        &rtfs::ast::MapKey::Keyword(entry.key.clone()),
                    );
                    // Check if this parameter has constraints or enum values
                    let ty = &*entry.value_type;
                    // For enum types, extract the values
                    if let rtfs::ast::TypeExpr::Union(variants) = ty {
                        let values: Vec<String> = variants
                            .iter()
                            .filter_map(|v| {
                                if let rtfs::ast::TypeExpr::Literal(lit) = v {
                                    match lit {
                                        rtfs::ast::Literal::String(s) => Some(s.clone()),
                                        rtfs::ast::Literal::Keyword(k) => {
                                            Some(value_conversion::map_key_to_string(
                                                &rtfs::ast::MapKey::Keyword(k.clone()),
                                            ))
                                        }
                                        _ => None,
                                    }
                                } else {
                                    None
                                }
                            })
                            .collect();
                        if !values.is_empty() {
                            hints.push(format!("{} supports: {}", param_name, values.join(", ")));
                        }
                    }
                }
            }
            _ => {}
        }

        hints
    }

    /// Extract parameter hints from MCP tool description
    /// Finds patterns like "state (open|closed|all)" and converts to usage hints
    fn extract_parameter_hints_from_mcp_description(&self, description: &str) -> Vec<String> {
        let mut hints = Vec::new();

        // Pattern: "param_name (value1|value2|value3)"
        let enum_re = regex::Regex::new(r"(\w+)\s*\(([^)]+)\)").unwrap();
        for cap in enum_re.captures_iter(description) {
            if let (Some(param), Some(values)) = (cap.get(1), cap.get(2)) {
                let param_name = param.as_str();
                let value_list = values.as_str();
                hints.push(format!("{} parameter supports: {}", param_name, value_list));
            }
        }

        hints
    }

    /// Extract parameter names from a capability manifest
    fn extract_parameters_from_manifest(&self, manifest: &CapabilityManifest) -> Vec<String> {
        let mut parameters = Vec::new();

        // Prefer primitive-aware extraction if metadata is available
        if manifest.metadata.get("primitive_kind").is_some() {
            if let Some(primitive_params) =
                self.extract_parameters_from_primitive_manifest(manifest)
            {
                if !primitive_params.is_empty() {
                    return primitive_params;
                }
            }
        }

        // Try to extract from input schema if available
        if let Some(ref schema) = manifest.input_schema {
            parameters.extend(self.extract_params_from_type_expr(schema));
        }

        // Also check metadata for parameter hints
        if let Some(params_str) = manifest.metadata.get("parameters") {
            parameters.extend(
                params_str
                    .split(',')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty()),
            );
        }

        // For MCP capabilities, check tool description in metadata
        if let Some(tool_desc) = manifest.metadata.get("mcp_tool_description") {
            // Try to extract parameter names from description
            // Common patterns: "state (open|closed|all)", "labels: array", etc.
            let extracted = self.extract_params_from_mcp_description(tool_desc);
            parameters.extend(extracted);
        }

        // For MCP capabilities, also check input_schema JSON Schema if available
        if let Some(schema_json) = manifest.metadata.get("mcp_input_schema") {
            // Try to parse JSON Schema and extract property names
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(schema_json) {
                if let Some(props) = parsed.get("properties").and_then(|p| p.as_object()) {
                    for prop_name in props.keys() {
                        parameters.push(prop_name.clone());
                    }
                }
            }
        }

        // Remove duplicates while preserving order
        let mut seen = std::collections::HashSet::new();
        parameters.retain(|p| seen.insert(p.clone()));

        parameters
    }

    fn extract_parameters_from_primitive_manifest(
        &self,
        manifest: &CapabilityManifest,
    ) -> Option<Vec<String>> {
        let annotations = Self::primitive_annotations_value(manifest);
        let required_inputs = Self::schema_bindings(manifest.input_schema.as_ref());
        let expected_outputs = Self::schema_bindings(manifest.output_schema.as_ref());

        if required_inputs.is_empty()
            && expected_outputs.is_empty()
            && annotations == JsonValue::Null
        {
            return None;
        }

        let primitive_need = CapabilityNeed::new(
            manifest.id.clone(),
            required_inputs.clone(),
            expected_outputs.clone(),
            manifest.description.clone(),
        )
        .with_annotations(annotations.clone())
        .with_schemas(
            manifest.input_schema.clone(),
            manifest.output_schema.clone(),
        );

        let ctx = PrimitiveContext::from_manifest(&primitive_need, manifest, annotations);

        let mut params: Vec<String> = ctx
            .input_schemas
            .keys()
            .map(|binding| binding.trim_start_matches(':').to_string())
            .collect();

        if params.is_empty() {
            params = primitive_need.required_inputs.clone();
        }

        if params.is_empty() {
            return None;
        }

        let mut seen = std::collections::HashSet::new();
        params.retain(|p| seen.insert(p.clone()));

        Some(params)
    }

    fn primitive_annotations_value(manifest: &CapabilityManifest) -> JsonValue {
        manifest
            .metadata
            .get("primitive_annotations")
            .and_then(|raw| serde_json::from_str::<JsonValue>(raw).ok())
            .unwrap_or(JsonValue::Null)
    }

    fn primitive_metadata_value(manifest: &CapabilityManifest) -> Option<JsonValue> {
        manifest
            .metadata
            .get("primitive_metadata")
            .and_then(|raw| serde_json::from_str::<JsonValue>(raw).ok())
    }

    fn primitive_metadata_hints(metadata: &JsonValue) -> Vec<String> {
        let mut hints = Vec::new();

        if let Some(kind) = metadata.get("primitive").and_then(|v| v.as_str()) {
            match kind {
                "filter" => {
                    if let Some(fields) = metadata.get("search_fields").and_then(|v| v.as_array()) {
                        if !fields.is_empty() {
                            let list: Vec<String> = fields
                                .iter()
                                .filter_map(|f| f.as_str().map(|s| s.to_string()))
                                .collect();
                            if !list.is_empty() {
                                hints.push(format!("Filter checks fields: {}", list.join(", ")));
                            }
                        }
                    }
                    if let Some(search_input) =
                        metadata.get("search_input").and_then(|v| v.as_str())
                    {
                        hints.push(format!("Search input binding: {}", search_input));
                    }
                }
                "map" => {
                    if let Some(mapping) = metadata.get("mapping").and_then(|v| v.as_array()) {
                        let pairs: Vec<String> = mapping
                            .iter()
                            .filter_map(|entry| {
                                entry.as_array().and_then(|vals| {
                                    if vals.len() == 2 {
                                        let to = vals[0].as_str().unwrap_or_default();
                                        let from = vals[1].as_str().unwrap_or_default();
                                        if !to.is_empty() && !from.is_empty() {
                                            Some(format!("{}←{}", to, from))
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                })
                            })
                            .collect();
                        if !pairs.is_empty() {
                            hints.push(format!("Field mapping: {}", pairs.join(", ")));
                        }
                    }
                }
                "project" => {
                    if let Some(fields) = metadata.get("fields").and_then(|v| v.as_array()) {
                        let list: Vec<String> = fields
                            .iter()
                            .filter_map(|f| f.as_str().map(|s| s.to_string()))
                            .collect();
                        if !list.is_empty() {
                            hints.push(format!("Project retains fields: {}", list.join(", ")));
                        }
                    }
                }
                "reduce" => {
                    if let Some(reducer) = metadata.get("reducer").and_then(|v| v.as_object()) {
                        if let Some(func) = reducer.get("fn").and_then(|v| v.as_str()) {
                            hints.push(format!("Reducer function: {}", func));
                        }
                        if let Some(field) = reducer
                            .get("item_field")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                        {
                            hints.push(format!("Reducer field: {}", field));
                        }
                    }
                }
                "sort" => {
                    if let Some(sort_key) = metadata.get("sort_key").and_then(|v| v.as_str()) {
                        let order = metadata
                            .get("order")
                            .and_then(|v| v.as_str())
                            .unwrap_or(":asc");
                        hints.push(format!("Sort by {} ({})", sort_key, order));
                    }
                }
                "groupBy" => {
                    if let Some(group_key) = metadata.get("group_key").and_then(|v| v.as_str()) {
                        hints.push(format!("Group by {}", group_key));
                    }
                }
                "join" => {
                    if let Some(on) = metadata.get("on").and_then(|v| v.as_array()) {
                        if on.len() == 2 {
                            let left = on[0].as_str().unwrap_or_default();
                            let right = on[1].as_str().unwrap_or_default();
                            hints.push(format!("Join keys: {} = {}", left, right));
                        }
                    }
                    if let Some(join_type) = metadata.get("type").and_then(|v| v.as_str()) {
                        hints.push(format!("Join type: {}", join_type));
                    }
                }
                _ => {}
            }
        }

        hints
    }

    fn schema_bindings(schema: Option<&rtfs::ast::TypeExpr>) -> Vec<String> {
        match schema {
            Some(rtfs::ast::TypeExpr::Map { entries, .. }) => entries
                .iter()
                .map(|entry| {
                    value_conversion::map_key_to_string(&rtfs::ast::MapKey::Keyword(
                        entry.key.clone(),
                    ))
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Extract parameter names from a TypeExpr (simple implementation)
    fn extract_params_from_type_expr(&self, expr: &rtfs::ast::TypeExpr) -> Vec<String> {
        let mut params = Vec::new();

        match expr {
            rtfs::ast::TypeExpr::Map { entries, .. } => {
                for entry in entries {
                    // Extract keyword name (remove the ':' prefix if present)
                    let param_name = value_conversion::map_key_to_string(
                        &rtfs::ast::MapKey::Keyword(entry.key.clone()),
                    );
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

    /// Extract parameter names and hints from MCP tool description
    /// Parses descriptions like "state (open|closed|all)", "labels: array", etc.
    fn extract_params_from_mcp_description(&self, description: &str) -> Vec<String> {
        let mut params = Vec::new();

        // Common patterns in MCP tool descriptions:
        // - "parameter_name (value1|value2|value3)" - enum values
        // - "parameter_name: type" - type hints
        // - "parameter_name parameter description" - parameter mentions

        // Use regex to find parameter mentions
        // Pattern: word followed by optional type/enum in parentheses or colon
        let re = regex::Regex::new(r"(\w+)\s*(?:\([^)]+\)|:\s*\w+|,)").unwrap();
        for cap in re.captures_iter(description) {
            if let Some(param) = cap.get(1) {
                let param_name = param.as_str().to_string();
                // Filter out common English words that aren't parameters
                if !matches!(
                    param_name.as_str(),
                    "the"
                        | "a"
                        | "an"
                        | "and"
                        | "or"
                        | "for"
                        | "with"
                        | "from"
                        | "to"
                        | "in"
                        | "on"
                        | "at"
                        | "by"
                ) {
                    params.push(param_name);
                }
            }
        }

        // Also look for explicit parameter mentions in common formats
        // "parameter_name" or "the parameter_name" patterns
        let explicit_re =
            regex::Regex::new(r"(?:the\s+)?(\w+)\s+(?:parameter|argument|field|option)").unwrap();
        for cap in explicit_re.captures_iter(description) {
            if let Some(param) = cap.get(1) {
                let param_name = param.as_str().to_string();
                if !params.contains(&param_name) {
                    params.push(param_name);
                }
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
        let keywords: Vec<&str> = capability_id
            .split(&['.', '_'][..])
            .filter(|k| k.len() > 2)
            .collect();

        if keywords.is_empty() {
            return Ok(None);
        }

        let all_capabilities = self.marketplace.list_capabilities().await;

        // Search for capabilities with overlapping keywords
        let mut best_match: Option<(CapabilityManifest, usize)> = None;
        for manifest in all_capabilities {
            let manifest_keywords: Vec<&str> = manifest
                .id
                .split(&['.', '_'][..])
                .filter(|k| k.len() > 2)
                .collect();

            let overlap = keywords
                .iter()
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

    /// Get MCP authentication token from environment variables
    ///
    /// Priority (generic for any MCP server):
    /// 1. Server-specific token: {NAMESPACE}_MCP_TOKEN (e.g., GITHUB_MCP_TOKEN for github/github-mcp)
    /// 2. Generic token: MCP_AUTH_TOKEN (works for any MCP server)
    ///
    /// For GitHub servers specifically, also checks (for backward compatibility):
    /// - GITHUB_PAT
    /// - GITHUB_TOKEN
    ///
    /// Returns the token if found, None otherwise
    fn get_mcp_auth_token(&self, server_name: &str) -> Option<String> {
        // Extract namespace from server name (e.g., "github/github-mcp" -> "github")
        let namespace = if let Some(slash_pos) = server_name.find('/') {
            &server_name[..slash_pos]
        } else {
            server_name
        };

        // Normalize namespace: replace hyphens with underscores and uppercase
        let normalized_namespace = namespace.replace("-", "_").to_uppercase();
        let server_specific_var = format!("{}_MCP_TOKEN", normalized_namespace);

        // Try server-specific token first (e.g., GITHUB_MCP_TOKEN, SLACK_MCP_TOKEN)
        if let Ok(token) = std::env::var(&server_specific_var) {
            if !token.is_empty() {
                return Some(token);
            }
        }

        // For GitHub servers, check legacy token names (backward compatibility)
        let namespace_lower = namespace.to_lowercase();
        if namespace_lower == "github" {
            if let Ok(token) = std::env::var("GITHUB_PAT") {
                if !token.is_empty() {
                    return Some(token);
                }
            }
            if let Ok(token) = std::env::var("GITHUB_TOKEN") {
                if !token.is_empty() {
                    return Some(token);
                }
            }
        }

        // Fall back to generic MCP auth token (works for any server)
        if let Ok(token) = std::env::var("MCP_AUTH_TOKEN") {
            if !token.is_empty() {
                return Some(token);
            }
        }

        None
    }

    /// Suggest an environment variable name for MCP authentication token
    fn suggest_mcp_token_env_var(&self, server_name: &str) -> String {
        let namespace = if let Some(slash_pos) = server_name.find('/') {
            &server_name[..slash_pos]
        } else {
            server_name
        };

        let normalized = namespace.replace("-", "_").to_uppercase();
        format!("{}_MCP_TOKEN", normalized)
    }

    /// Generate an enhanced rationale from a capability class name for better semantic matching
    /// Converts abstract names like "DelegatingAsk" into functional descriptions
    /// that semantic matching can understand
    fn generate_enhanced_rationale(&self, capability_class: &str, fallback: &str) -> String {
        let lower = capability_class.to_lowercase();

        // Generate functional descriptions based on common patterns
        if lower.contains("ask") {
            if lower.contains("user")
                || lower.contains("delegating")
                || lower.contains("interactive")
            {
                return "Ask the user a question and get their response. Prompts user for input"
                    .to_string();
            }
        }

        if lower.contains("echo") || lower.contains("print") {
            if !lower.contains("api") {
                return "Echo or print a message. Output text to console".to_string();
            }
        }

        // Extract keywords and generate a functional description
        let keywords = crate::catalog::matcher::extract_keywords(capability_class);
        if !keywords.is_empty() {
            // Try to infer function from keywords
            let action = keywords.iter().find(|k| {
                matches!(
                    k.as_str(),
                    "ask"
                        | "get"
                        | "list"
                        | "search"
                        | "find"
                        | "create"
                        | "update"
                        | "delete"
                        | "echo"
                        | "print"
                )
            });

            if let Some(action) = action {
                let other_keywords: Vec<String> =
                    keywords.iter().skip(1).take(2).cloned().collect();
                return format!("{} {} capability", action, other_keywords.join(" "));
            }
        }

        // Fallback to original
        fallback.to_string()
    }

    /// Format provider type as string for hints
    /// Save a synthesized capability to disk
    pub async fn save_synthesized_capability(
        &self,
        manifest: &CapabilityManifest,
    ) -> RuntimeResult<()> {
        use std::fs;

        let storage_dir = std::env::var("CCOS_CAPABILITY_STORAGE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| crate::utils::fs::get_configured_generated_path());

        fs::create_dir_all(&storage_dir).map_err(|e| {
            RuntimeError::Generic(format!("Failed to create storage directory: {}", e))
        })?;

        let capability_dir = storage_dir.join(&manifest.id);
        fs::create_dir_all(&capability_dir).map_err(|e| {
            RuntimeError::Generic(format!("Failed to create capability directory: {}", e))
        })?;

        // Get RTFS implementation code from metadata
        let rtfs_code = manifest
            .metadata
            .get("rtfs_implementation")
            .cloned()
            .unwrap_or_else(|| {
                format!(
                    ";; Synthesized capability: {}\n;; No RTFS implementation stored",
                    manifest.id
                )
            });

        // Get schema strings
        let input_schema_str = manifest
            .input_schema
            .as_ref()
            .map(type_expr_to_rtfs_compact)
            .unwrap_or_else(|| ":any".to_string());
        let output_schema_str = manifest
            .output_schema
            .as_ref()
            .map(type_expr_to_rtfs_compact)
            .unwrap_or_else(|| ":any".to_string());

        // Create full capability RTFS file
        let metadata_block = Self::format_metadata_map(&manifest.metadata);

        let capability_rtfs = format!(
            r#";; Synthesized capability: {}
;; Generated: {}
(capability "{}"
  :name "{}"
  :version "{}"
  :description "{}"
  :synthesis-method "local_rtfs"
  {}
  :permissions []
  :effects []
  :input-schema {}
  :output-schema {}
  :implementation
    {}
)
"#,
            manifest.id,
            chrono::Utc::now().to_rfc3339(),
            manifest.id,
            manifest.name,
            manifest.version,
            manifest.description,
            metadata_block,
            input_schema_str,
            output_schema_str,
            rtfs_code
        );

        let capability_file = capability_dir.join("capability.rtfs");
        fs::write(&capability_file, capability_rtfs).map_err(|e| {
            RuntimeError::Generic(format!("Failed to write capability file: {}", e))
        })?;

        Ok(())
    }

    // (moved inside save_mcp_capability function)

    /// Save an MCP capability to disk (similar to synthesized capabilities)
    pub async fn save_mcp_capability(&self, manifest: &CapabilityManifest) -> RuntimeResult<()> {
        use std::fs;

        // Extract MCP provider information - check both ProviderType::MCP and Local with MCP metadata
        let (server_url, tool_name) = match &manifest.provider {
            crate::capability_marketplace::types::ProviderType::MCP(mcp) => {
                (mcp.server_url.clone(), mcp.tool_name.clone())
            }
            // MCP introspection creates Local capabilities with MCP metadata
            crate::capability_marketplace::types::ProviderType::Local(_) => {
                // Check if this is an MCP capability by looking at metadata
                let server_url = manifest.metadata.get("mcp_server_url").ok_or_else(|| {
                    RuntimeError::Generic(format!(
                        "Capability {} has Local provider but missing mcp_server_url in metadata",
                        manifest.id
                    ))
                })?;
                let tool_name = manifest.metadata.get("mcp_tool_name").ok_or_else(|| {
                    RuntimeError::Generic(format!(
                        "Capability {} has Local provider but missing mcp_tool_name in metadata",
                        manifest.id
                    ))
                })?;
                (server_url.clone(), tool_name.clone())
            }
            _ => {
                return Err(RuntimeError::Generic(format!(
                    "Capability {} is not an MCP capability (provider: {:?})",
                    manifest.id, manifest.provider
                )));
            }
        };

        let storage_dir = std::env::var("CCOS_CAPABILITY_STORAGE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| crate::utils::fs::get_workspace_root().join("capabilities"));

        // Use hierarchical structure: capabilities/servers/pending/<namespace>/<tool>.rtfs
        // Parse capability ID: "mcp.namespace.tool_name" or "github.issues.list"
        let parts: Vec<&str> = manifest.id.split('.').collect();
        let capability_dir = if parts.len() >= 3 && parts[0] == "mcp" {
            // MCP capability with explicit "mcp" prefix
            let namespace = parts[1];
            storage_dir.join("servers").join("pending").join(namespace)
        } else if parts.len() >= 2 {
            // Capability like "github.issues.list"
            let namespace = parts[0];
            storage_dir.join("servers").join("pending").join(namespace)
        } else {
            // Fallback: use capability ID directly
            storage_dir.join("servers").join("pending").join("misc")
        };

        fs::create_dir_all(&capability_dir).map_err(|e| {
            RuntimeError::Generic(format!("Failed to create capability directory: {}", e))
        })?;

        // Get tool name from parts or metadata
        let tool_file_name = if parts.len() >= 3 {
            parts[2..].join("_")
        } else if parts.len() >= 2 {
            parts[1..].join("_")
        } else {
            tool_name.clone()
        };

        // Get RTFS implementation code if available, otherwise generate a placeholder
        let rtfs_code = manifest
            .metadata
            .get("rtfs_implementation")
            .cloned()
            .unwrap_or_else(|| {
                // Generate a simple MCP wrapper if no implementation is stored
                format!(
                    r#"(fn [input]
  ;; MCP Tool: {}
  ;; Runtime handles MCP protocol automatically
  (call :ccos.capabilities.mcp.call
    :server-url "{}"
    :tool-name "{}"
    :input input))"#,
                    manifest.name, server_url, tool_name
                )
            });

        // Get schema strings
        let input_schema_str = manifest
            .input_schema
            .as_ref()
            .map(type_expr_to_rtfs_compact)
            .unwrap_or_else(|| ":any".to_string());
        let output_schema_str = manifest
            .output_schema
            .as_ref()
            .map(type_expr_to_rtfs_compact)
            .unwrap_or_else(|| ":any".to_string());

        // Create full capability RTFS file
        let output_snippet_comment = if let Some(snippet) = manifest.metadata.get("output_snippet")
        {
            format!(
                ";; Sample output format:\n;; {}\n",
                snippet.lines().take(5).collect::<Vec<_>>().join("\n;; ")
            )
        } else {
            String::new()
        };

        let capability_rtfs = format!(
            r#";; MCP Capability: {}
;; Generated: {}
;; MCP Server: {}
;; Tool: {}
{}
(capability "{}"
  :name "{}"
  :version "{}"
  :description "{}"
  :provider "MCP"
  :permissions []
  :effects []
  :metadata {{
    :mcp {{
      :server_url "{}"
      :tool_name "{}"
      :requires_session "auto"
      :auth_env_var "MCP_AUTH_TOKEN"
    }}
    :discovery {{
      :method "mcp_registry"
      :created_at "{}"
      :capability_type "mcp_tool"
    }}
  }}
  :input-schema {}
  :output-schema {}
  :implementation
    {}
)
"#,
            manifest.id,
            chrono::Utc::now().to_rfc3339(),
            server_url,
            tool_name,
            output_snippet_comment,
            manifest.id,
            manifest.name,
            manifest.version,
            manifest.description,
            server_url,
            tool_name,
            chrono::Utc::now().to_rfc3339(),
            input_schema_str,
            output_schema_str,
            rtfs_code
        );

        let capability_file = capability_dir.join(format!("{}.rtfs", tool_file_name));
        fs::write(&capability_file, capability_rtfs).map_err(|e| {
            RuntimeError::Generic(format!("Failed to write capability file: {}", e))
        })?;

        Ok(())
    }

    fn format_provider(
        &self,
        provider: &crate::capability_marketplace::types::ProviderType,
    ) -> String {
        match provider {
            crate::capability_marketplace::types::ProviderType::MCP(_) => "MCP".to_string(),
            crate::capability_marketplace::types::ProviderType::OpenApi(_) => "OpenAPI".to_string(),
            crate::capability_marketplace::types::ProviderType::Local(_) => "Local".to_string(),
            crate::capability_marketplace::types::ProviderType::Http(_) => "HTTP".to_string(),
            _ => "Unknown".to_string(),
        }
    }

    fn format_metadata_map(metadata: &std::collections::HashMap<String, String>) -> String {
        if metadata.is_empty() {
            "  :metadata nil".to_string()
        } else {
            let mut entries: Vec<_> = metadata.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            let mut lines = Vec::with_capacity(entries.len());
            for (key, value) in entries {
                let escaped = value.replace('"', "\\\"");
                lines.push(format!("    :{} \"{}\"", key, escaped));
            }
            format!("  :metadata {{\n{}\n  }}", lines.join("\n"))
        }
    }

    // We deliberately avoid generating synthetic examples from the RTFS `TypeExpr`.
    // Real snippet examples should be gathered via `introspect_output_schema`
    // (this calls the MCP tool with safe inputs and returns an actual sample
    // of the tool's response). If no real sample was captured during
    // introspection, we leave the manifest without an `output_snippet` so the
    // generated RTFS file does not contain a fabricated example.

    async fn enrich_manifest_output_schema(
        &self,
        manifest: &mut CapabilityManifest,
        introspection: &crate::synthesis::mcp_introspector::MCPIntrospectionResult,
        introspector: &crate::synthesis::mcp_introspector::MCPIntrospector,
        url: &str,
        server_name: &str,
        auth_headers: &Option<std::collections::HashMap<String, String>>,
        log_errors: bool,
    ) {
        let headers = match auth_headers {
            Some(inner) if !inner.is_empty() => inner.clone(),
            _ => return,
        };

        if let Some(tool) = introspection
            .tools
            .iter()
            .find(|tool| tool.tool_name == manifest.name)
        {
            match introspector
                .introspect_output_schema(tool, url, server_name, Some(headers), None)
                .await
            {
                Ok((Some(schema), sample_opt)) => {
                    manifest.output_schema = Some(schema);
                    if let Some(snippet) = sample_opt {
                        manifest
                            .metadata
                            .insert("output_snippet".to_string(), snippet);
                    }
                }
                Ok((None, Some(sample))) => {
                    manifest
                        .metadata
                        .insert("output_snippet".to_string(), sample);
                }
                Ok((None, None)) => {}
                Err(err) => {
                    if log_errors {
                        crate::ccos_println!(
                            "     ⚠️ Output schema introspection failed for '{}': {}",
                            manifest.name, err
                        );
                    }
                }
            }
        }
    }
}
// Close impl DiscoveryEngine

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
    pub provider: String,        // "MCP", "OpenAPI", "Local", etc.
    pub parameters: Vec<String>, // Available parameters
    pub hints: Vec<String>,      // Usage hints
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

// -----------------------------------------------------------------------------
// Local helper utilities (placed at file scope to avoid nesting in impl blocks)
// -----------------------------------------------------------------------------

fn eh_normalize_identifier(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_lowercase())
        .collect()
}

fn eh_collect_schema_keys(schema: &rtfs::ast::TypeExpr, out: &mut Vec<String>) {
    match schema {
        rtfs::ast::TypeExpr::Map { entries, .. } => {
            for entry in entries {
                out.push(value_conversion::map_key_to_string(
                    &rtfs::ast::MapKey::Keyword(entry.key.clone()),
                ));
            }
        }
        rtfs::ast::TypeExpr::Vector(inner) | rtfs::ast::TypeExpr::Optional(inner) => {
            eh_collect_schema_keys(inner, out);
        }
        rtfs::ast::TypeExpr::Union(options) => {
            for opt in options {
                eh_collect_schema_keys(opt, out);
            }
        }
        _ => {}
    }
}

/// Returns true if the manifest's schemas plausibly satisfy the need.
/// - If input schema keys exist: all required inputs must be present (case-insensitive, normalized).
/// - If output schema keys exist and expected outputs are declared: require at least one overlap.
fn manifest_satisfies_need(
    manifest: &crate::capability_marketplace::types::CapabilityManifest,
    need: &super::need_extractor::CapabilityNeed,
) -> bool {
    // Check inputs if schema available
    if let Some(input_schema) = &manifest.input_schema {
        let mut keys = Vec::new();
        eh_collect_schema_keys(input_schema, &mut keys);
        if !keys.is_empty() && !need.required_inputs.is_empty() {
            let key_set: Vec<String> = keys
                .into_iter()
                .map(|k| eh_normalize_identifier(&k))
                .collect();
            for req in &need.required_inputs {
                let req_n = eh_normalize_identifier(req);
                if !key_set.iter().any(|k| k == &req_n) {
                    return false;
                }
            }
        }
    }

    // Check outputs if schema available
    if let Some(output_schema) = &manifest.output_schema {
        let mut out_keys = Vec::new();
        eh_collect_schema_keys(output_schema, &mut out_keys);
        if !out_keys.is_empty() && !need.expected_outputs.is_empty() {
            let out_set: Vec<String> = out_keys
                .into_iter()
                .map(|k| eh_normalize_identifier(&k))
                .collect();
            let any_overlap = need
                .expected_outputs
                .iter()
                .map(|o| eh_normalize_identifier(o))
                .any(|o| out_set.iter().any(|k| k == &o));
            if !any_overlap {
                return false;
            }
        }
    }

    true
}

/// Extract high-value context tokens (platforms, services) from the rationale/goal text.
fn extract_context_tokens(rationale: &str) -> Vec<String> {
    // List of known platforms/services to prioritize in search context
    // These are words that, if present in the goal, MUST be in the search query
    // to find the right tool.
    let high_value_contexts = [
        "github",
        "gitlab",
        "bitbucket",
        "jira",
        "slack",
        "discord",
        "telegram",
        "google",
        "gmail",
        "calendar",
        "drive",
        "aws",
        "azure",
        "gcp",
        "cloud",
        "postgres",
        "mysql",
        "sql",
        "redis",
        "mongo",
        "linear",
        "notion",
        "trello",
        "asana",
        "stripe",
        "paypal",
        "weather",
        "stock",
        "finance",
        "email",
        "sms",
        "filesystem",
        "file",
        "git",
        "spotify",
        "youtube",
        "huggingface",
        "openai",
        "anthropic",
        "llm",
        "ai",
    ];

    let text_lower = rationale.to_ascii_lowercase();
    let mut matches = Vec::new();

    for &context in &high_value_contexts {
        // Check if the context word appears as a distinct word in the text
        // Using a simple contains check is usually sufficient given these are unique keywords
        if text_lower.contains(context) {
            matches.push(context.to_string());
        }
    }

    // Filter out substrings (e.g. remove "git" if "github" is present)
    let mut context_tokens = Vec::new();
    for m in &matches {
        // Only check if 'm' is a substring of ANOTHER match.
        let is_substring = matches.iter().any(|other| other != m && other.contains(m));
        if !is_substring {
            context_tokens.push(m.clone());
        }
    }

    context_tokens
}
