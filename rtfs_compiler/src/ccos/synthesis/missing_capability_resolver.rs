//! Missing Capability Resolution System
//!
//! This module implements Phase 2 of the missing capability resolution plan:
//! - Runtime trap for missing capability errors
//! - Resolution queue for background processing
//! - Integration with marketplace discovery

use crate::ast::TypeExpr;
use crate::ccos::capability_marketplace::types::{
    CapabilityKind, CapabilityManifest, CapabilityQuery,
};
use crate::ccos::capability_marketplace::CapabilityMarketplace;
use crate::ccos::checkpoint_archive::CheckpointArchive;
use crate::ccos::synthesis::feature_flags::{FeatureFlagChecker, MissingCapabilityConfig};
use crate::runtime::error::{RuntimeError, RuntimeResult};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};

/// Represents a missing capability that needs resolution
#[derive(Debug, Clone)]
pub struct MissingCapabilityRequest {
    /// The capability ID that was requested but not found
    pub capability_id: String,
    /// Arguments that were passed to the capability (for context)
    pub arguments: Vec<crate::runtime::values::Value>,
    /// Context from the execution (plan_id, intent_id, etc.)
    pub context: HashMap<String, String>,
    /// Timestamp when the request was created
    pub requested_at: std::time::SystemTime,
    /// Number of resolution attempts made
    pub attempt_count: u32,
}

/// Result of a capability resolution attempt
#[derive(Debug, Clone)]
pub enum ResolutionResult {
    /// Capability was successfully resolved and registered
    Resolved {
        capability_id: String,
        resolution_method: String,
        provider_info: Option<String>,
    },
    /// Resolution failed and should be retried later
    Failed {
        capability_id: String,
        reason: String,
        retry_after: Option<std::time::Duration>,
    },
    /// Resolution permanently failed (e.g., invalid capability ID)
    PermanentlyFailed {
        capability_id: String,
        reason: String,
    },
}

/// An MCP server with its relevance score
#[derive(Debug, Clone)]
pub struct RankedMcpServer {
    pub server: crate::ccos::synthesis::mcp_registry_client::McpServer,
    pub score: f64,
}

/// Queue for managing missing capability resolution requests
#[derive(Debug)]
pub struct MissingCapabilityQueue {
    /// Pending resolution requests
    queue: VecDeque<MissingCapabilityRequest>,
    /// Capabilities currently being resolved (to avoid duplicates)
    in_progress: HashSet<String>,
    /// Failed resolutions with retry information
    failed_resolutions: HashMap<String, (MissingCapabilityRequest, std::time::SystemTime)>,
    /// Maximum number of resolution attempts per capability
    max_attempts: u32,
}

impl MissingCapabilityQueue {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
            in_progress: HashSet::new(),
            failed_resolutions: HashMap::new(),
            max_attempts: 3,
        }
    }

    /// Add a missing capability request to the resolution queue
    pub fn enqueue(&mut self, request: MissingCapabilityRequest) {
        // Don't add if already in progress or permanently failed
        if self.in_progress.contains(&request.capability_id) {
            return;
        }

        if self.failed_resolutions.contains_key(&request.capability_id) {
            return;
        }

        // Check if already in queue
        if self
            .queue
            .iter()
            .any(|r| r.capability_id == request.capability_id)
        {
            return;
        }

        self.queue.push_back(request);
    }

    /// Get the next resolution request from the queue
    pub fn dequeue(&mut self) -> Option<MissingCapabilityRequest> {
        if let Some(request) = self.queue.pop_front() {
            self.in_progress.insert(request.capability_id.clone());
            Some(request)
        } else {
            None
        }
    }

    /// Mark a capability as completed (resolved or permanently failed)
    pub fn mark_completed(&mut self, capability_id: &str, result: ResolutionResult) {
        self.in_progress.remove(capability_id);

        match result {
            ResolutionResult::PermanentlyFailed { .. } => {
                // Add to failed_resolutions to prevent future attempts
                if let Some(request) = self.queue.iter().find(|r| r.capability_id == capability_id)
                {
                    self.failed_resolutions.insert(
                        capability_id.to_string(),
                        (request.clone(), std::time::SystemTime::now()),
                    );
                }
            }
            ResolutionResult::Failed {
                retry_after: _retry_after,
                ..
            } => {
                // Remove from in_progress, will be retried later
                self.in_progress.remove(capability_id);

                // If retry_after is specified, we could implement a delayed retry mechanism
                // For now, we'll just remove from in_progress and let it be re-queued
            }
            ResolutionResult::Resolved { .. } => {
                // Successfully resolved, remove from failed_resolutions if present
                self.failed_resolutions.remove(capability_id);
            }
        }
    }

    /// Check if there are pending resolution requests
    pub fn has_pending(&self) -> bool {
        !self.queue.is_empty()
    }

    /// Get queue statistics
    pub fn stats(&self) -> QueueStats {
        QueueStats {
            pending_count: self.queue.len(),
            in_progress_count: self.in_progress.len(),
            failed_count: self.failed_resolutions.len(),
        }
    }
}

/// Statistics about the resolution queue
#[derive(Debug, Clone)]
pub struct QueueStats {
    pub pending_count: usize,
    pub in_progress_count: usize,
    pub failed_count: usize,
}

/// Main resolver that coordinates missing capability resolution
pub struct MissingCapabilityResolver {
    /// The resolution queue
    queue: Arc<Mutex<MissingCapabilityQueue>>,
    /// Reference to the capability marketplace for discovery and registration
    marketplace: Arc<CapabilityMarketplace>,
    /// Reference to the checkpoint archive for auto-resume functionality
    checkpoint_archive: Arc<CheckpointArchive>,
    /// Configuration for resolution behavior
    config: ResolverConfig,
    /// Feature flag checker for controlling system behavior
    feature_checker: FeatureFlagChecker,
}

/// Configuration for the missing capability resolver
#[derive(Debug, Clone)]
pub struct ResolverConfig {
    /// Maximum number of resolution attempts per capability
    pub max_attempts: u32,
    /// Whether to enable automatic resolution attempts
    pub auto_resolve: bool,
    /// Whether to log detailed resolution information
    pub verbose_logging: bool,
}

impl Default for ResolverConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            auto_resolve: true,
            verbose_logging: false,
        }
    }
}

impl MissingCapabilityResolver {
    /// Create a new missing capability resolver
    pub fn new(
        marketplace: Arc<CapabilityMarketplace>,
        checkpoint_archive: Arc<CheckpointArchive>,
        config: ResolverConfig,
        feature_config: MissingCapabilityConfig,
    ) -> Self {
        Self {
            queue: Arc::new(Mutex::new(MissingCapabilityQueue::new())),
            marketplace,
            checkpoint_archive,
            config,
            feature_checker: FeatureFlagChecker::new(feature_config),
        }
    }

    /// Handle a missing capability error by adding it to the resolution queue
    pub fn handle_missing_capability(
        &self,
        capability_id: String,
        arguments: Vec<crate::runtime::values::Value>,
        context: HashMap<String, String>,
    ) -> RuntimeResult<()> {
        eprintln!(
            "🔍 HANDLE MISSING: Attempting to handle missing capability '{}'",
            capability_id
        );

        // Check if missing capability resolution is enabled
        if !self.feature_checker.is_enabled() {
            eprintln!("❌ HANDLE MISSING: Feature is disabled");
            return Ok(()); // Silently ignore if disabled
        }

        if !self.feature_checker.is_runtime_detection_enabled() {
            eprintln!("❌ HANDLE MISSING: Runtime detection is disabled");
            return Ok(()); // Silently ignore if runtime detection is disabled
        }

        eprintln!("✅ HANDLE MISSING: Feature is enabled, proceeding with queue");
        let request = MissingCapabilityRequest {
            capability_id: capability_id.clone(),
            arguments,
            context,
            requested_at: std::time::SystemTime::now(),
            attempt_count: 0,
        };

        {
            let mut queue = self.queue.lock().unwrap();
            queue.enqueue(request);
        }

        if self.config.verbose_logging {
            eprintln!(
                "🔍 MISSING CAPABILITY: Added '{}' to resolution queue",
                capability_id
            );
        }

        // Emit audit event for missing capability
        self.emit_missing_capability_audit(&capability_id)?;

        Ok(())
    }

    /// Attempt to resolve a missing capability using various discovery methods
    pub async fn resolve_capability(
        &self,
        request: &MissingCapabilityRequest,
    ) -> RuntimeResult<ResolutionResult> {
        let capability_id = &request.capability_id;

        // Check if auto-resolution is enabled
        if !self.feature_checker.is_auto_resolution_enabled() {
            return Ok(ResolutionResult::Failed {
                capability_id: capability_id.clone(),
                reason: "Auto-resolution is disabled".to_string(),
                retry_after: None,
            });
        }

        eprintln!(
            "🔍 RESOLVING: Attempting to resolve capability '{}'",
            capability_id
        );

        // Phase 2: Marketplace Discovery
        // Check if capability already exists in marketplace (race condition check)
        {
            let capabilities = self.marketplace.capabilities.read().await;
            eprintln!(
                "🔍 DEBUG: Checking marketplace for '{}' - found {} capabilities",
                capability_id,
                capabilities.len()
            );
            if capabilities.contains_key(capability_id) {
                eprintln!(
                    "✅ RESOLUTION: Capability '{}' already exists in marketplace",
                    capability_id
                );
                return Ok(ResolutionResult::Resolved {
                    capability_id: capability_id.clone(),
                    resolution_method: "marketplace_found".to_string(),
                    provider_info: Some("already_registered".to_string()),
                });
            }
            eprintln!(
                "✅ Capability '{}' is missing from marketplace - proceeding with discovery",
                capability_id
            );
        }

        // Try to find similar capabilities using marketplace discovery
        eprintln!(
            "🔍 DISCOVERY: Starting discovery for capability '{}'",
            capability_id
        );
        let discovery_result = self.discover_capability(capability_id).await?;
        eprintln!(
            "🔍 DISCOVERY: Discovery result for '{}': {:?}",
            capability_id,
            discovery_result.is_some()
        );

        match discovery_result {
            Some(manifest) => {
                eprintln!(
                    "✅ DISCOVERY: Successfully discovered capability '{}'",
                    capability_id
                );
                // Register the discovered capability
                self.marketplace
                    .register_capability_manifest(manifest.clone())
                    .await?;

                // Trigger auto-resume for any checkpoints waiting for this capability
                self.trigger_auto_resume_for_capability(capability_id)
                    .await?;

                Ok(ResolutionResult::Resolved {
                    capability_id: capability_id.clone(),
                    resolution_method: "marketplace_discovery".to_string(),
                    provider_info: Some(format!("{:?}", manifest.provider)),
                })
            }
            None => {
                // No capability found through discovery
                Ok(ResolutionResult::Failed {
                    capability_id: capability_id.clone(),
                    reason: "No matching capability found through discovery".to_string(),
                    retry_after: Some(std::time::Duration::from_secs(60)), // Retry in 1 minute
                })
            }
        }
    }

    /// Discover a capability using marketplace discovery mechanisms
    async fn discover_capability(
        &self,
        capability_id: &str,
    ) -> RuntimeResult<Option<CapabilityManifest>> {
        if self.config.verbose_logging {
            eprintln!(
                "🔍 DISCOVERY: Starting fan-out discovery for '{}'",
                capability_id
            );
        }

        // Phase 2: Fan-out Discovery Pipeline
        // Try multiple discovery methods in order of preference

        // 1. Exact match in marketplace (race condition check)
        if let Some(manifest) = self.discover_exact_match(capability_id).await? {
            return Ok(Some(manifest));
        }

        // 2. Partial name matching in marketplace
        if let Some(manifest) = self.discover_partial_match(capability_id).await? {
            return Ok(Some(manifest));
        }

        // 3. Local manifest scanning
        if let Some(manifest) = self.discover_local_manifests(capability_id).await? {
            return Ok(Some(manifest));
        }

        // 4. MCP server discovery
        if let Some(manifest) = self.discover_mcp_servers(capability_id).await? {
            return Ok(Some(manifest));
        }

        // 5. Web search discovery (if enabled)
        if self.feature_checker.is_web_search_enabled() {
            if let Some(manifest) = self.discover_via_web_search(capability_id).await? {
                return Ok(Some(manifest));
            }
        }

        // 6. Network catalog queries (if configured)
        if let Some(manifest) = self.discover_network_catalogs(capability_id).await? {
            return Ok(Some(manifest));
        }

        if self.config.verbose_logging {
            eprintln!("🔍 DISCOVERY: No matches found for '{}'", capability_id);
        }

        Ok(None)
    }

    /// Discover exact match in marketplace
    async fn discover_exact_match(
        &self,
        capability_id: &str,
    ) -> RuntimeResult<Option<CapabilityManifest>> {
        // Check if capability already exists in marketplace
        let capabilities = self.marketplace.capabilities.read().await;
        eprintln!(
            "🔍 DEBUG: Checking marketplace for '{}' - found {} capabilities",
            capability_id,
            capabilities.len()
        );
        eprintln!(
            "🔍 DEBUG: Available capabilities: {:?}",
            capabilities.keys().collect::<Vec<_>>()
        );

        if let Some(manifest) = capabilities.get(capability_id) {
            eprintln!(
                "🔍 DISCOVERY: Found exact match in marketplace: '{}'",
                capability_id
            );
            return Ok(Some(manifest.clone()));
        }
        eprintln!("🔍 DEBUG: No exact match found for '{}'", capability_id);
        Ok(None)
    }

    /// Discover partial matches in marketplace using name similarity
    async fn discover_partial_match(
        &self,
        capability_id: &str,
    ) -> RuntimeResult<Option<CapabilityManifest>> {
        // Try to find primitive capabilities that might match
        let query = CapabilityQuery::new()
            .with_kind(CapabilityKind::Primitive)
            .with_limit(100);

        let available_capabilities = self.marketplace.list_capabilities_with_query(&query).await;

        // Simple partial matching for demonstration
        // In a real implementation, this would use more sophisticated matching
        for capability in available_capabilities {
            if self.is_partial_match(capability_id, &capability.id) {
                if self.config.verbose_logging {
                    eprintln!(
                        "🔍 DISCOVERY: Found partial match: '{}' -> '{}'",
                        capability_id, capability.id
                    );
                }
                return Ok(Some(capability));
            }
        }

        Ok(None)
    }

    /// Check if two capability IDs are partial matches
    fn is_partial_match(&self, requested: &str, available: &str) -> bool {
        // Simple partial matching logic
        // This could be enhanced with more sophisticated algorithms

        // Check if one is a prefix of the other
        if requested.starts_with(available) || available.starts_with(requested) {
            return true;
        }

        // Check if they share common segments (e.g., "travel.flights" vs "travel.hotels")
        let requested_segments: Vec<&str> = requested.split('.').collect();
        let available_segments: Vec<&str> = available.split('.').collect();

        if requested_segments.len() >= 2 && available_segments.len() >= 2 {
            // Check if domain matches (first segment)
            if requested_segments[0] == available_segments[0] {
                return true;
            }
        }

        false
    }

    /// Discover capabilities from local manifest files
    async fn discover_local_manifests(
        &self,
        capability_id: &str,
    ) -> RuntimeResult<Option<CapabilityManifest>> {
        if self.config.verbose_logging {
            eprintln!(
                "🔍 DISCOVERY: Scanning local manifests for '{}'",
                capability_id
            );
        }

        // TODO: Implement local manifest scanning
        // This would scan filesystem for capability manifest files
        // and check if any match the requested capability_id

        // For now, return None as this is not implemented
        Ok(None)
    }

    /// Discover capabilities from MCP servers via the official MCP Registry
    async fn discover_mcp_servers(
        &self,
        capability_id: &str,
    ) -> RuntimeResult<Option<CapabilityManifest>> {
        if self.config.verbose_logging {
            eprintln!(
                "🔍 DISCOVERY: Querying MCP Registry for '{}'",
                capability_id
            );
        }

        let registry_client = crate::ccos::synthesis::mcp_registry_client::McpRegistryClient::new();

        // Try semantic search first if the query looks like a description
        let servers = if self.is_semantic_query(capability_id) {
            eprintln!(
                "🔍 SEMANTIC SEARCH: Detected semantic query '{}'",
                capability_id
            );
            let keywords = self.extract_search_keywords(capability_id);
            eprintln!("🔍 SEMANTIC SEARCH: Extracted keywords: {:?}", keywords);
            self.semantic_search_servers(&keywords).await?
        } else {
            // Traditional exact capability ID search
            registry_client
                .find_capability_providers(capability_id)
                .await?
        };

        // Process the servers
        if self.config.verbose_logging {
            eprintln!(
                "🔍 DISCOVERY: Found {} MCP servers for '{}'",
                servers.len(),
                capability_id
            );
        }

        // Rank and filter servers
        let ranked_servers = self.rank_mcp_servers(capability_id, servers);

        if ranked_servers.is_empty() {
            if self.config.verbose_logging {
                eprintln!(
                    "❌ DISCOVERY: No suitable MCP servers found for '{}'",
                    capability_id
                );
            }
            return Ok(None);
        }

        // If multiple good options, ask user to choose
        let selected_server = if ranked_servers.len() > 1 {
            self.interactive_server_selection(capability_id, &ranked_servers)
                .await?
        } else {
            &ranked_servers[0]
        };

        if self.config.verbose_logging {
            eprintln!(
                "✅ DISCOVERY: Selected MCP server '{}' for capability '{}'",
                selected_server.server.name, capability_id
            );
        }

        // Convert MCP server to CCOS capability manifest
        match registry_client.convert_to_capability_manifest(&selected_server.server, capability_id)
        {
            Ok(manifest) => return Ok(Some(manifest)),
            Err(e) => {
                if self.config.verbose_logging {
                    eprintln!("⚠️ DISCOVERY: Failed to convert MCP server: {}", e);
                }
                return Ok(None);
            }
        }
    }

    /// Check if a capability name/description matches the requested capability ID
    /// Rank MCP servers by relevance and quality
    fn rank_mcp_servers(
        &self,
        capability_id: &str,
        servers: Vec<crate::ccos::synthesis::mcp_registry_client::McpServer>,
    ) -> Vec<RankedMcpServer> {
        let mut ranked: Vec<RankedMcpServer> = servers
            .into_iter()
            .map(|server| {
                let score = self.calculate_server_score(capability_id, &server);
                RankedMcpServer { server, score }
            })
            .collect();

        // Sort by score (highest first)
        ranked.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Filter out servers with very low scores
        ranked
            .into_iter()
            .filter(|ranked| ranked.score >= 0.3) // Minimum threshold
            .collect()
    }

    /// Calculate a relevance score for an MCP server
    fn calculate_server_score(
        &self,
        capability_id: &str,
        server: &crate::ccos::synthesis::mcp_registry_client::McpServer,
    ) -> f64 {
        let mut score = 0.0;
        let requested_lower = capability_id.to_lowercase();
        let name_lower = server.name.to_lowercase();
        let desc_lower = server.description.to_lowercase();

        // Exact name match (highest priority)
        if name_lower == requested_lower {
            score += 10.0;
        }
        // Exact match in description
        else if desc_lower.contains(&format!(" {} ", requested_lower)) {
            score += 8.0;
        }
        // Partial name match
        else if name_lower.contains(&requested_lower) {
            score += 6.0;
        }
        // Reverse partial match (requested contains server name)
        else if requested_lower.contains(&name_lower) {
            score += 4.0;
        }
        // Description contains capability
        else if desc_lower.contains(&requested_lower) {
            score += 3.0;
        }

        // Penalize overly specific servers (plugins, extensions, etc.)
        if name_lower.contains("plugin")
            || name_lower.contains("extension")
            || name_lower.contains("specific")
            || name_lower.contains("custom")
        {
            score -= 2.0;
        }

        // Bonus for general-purpose servers
        if name_lower.contains("api")
            || name_lower.contains("sdk")
            || name_lower.contains("client")
            || name_lower.contains("service")
            || name_lower.contains("provider")
        {
            score += 1.0;
        }

        // Bonus for official/well-known providers (generic pattern matching)
        if self.is_official_provider(&name_lower, &requested_lower) {
            score += 2.0;
        }

        // Normalize score to 0-1 range
        (score / 10.0f64).min(1.0f64).max(0.0f64)
    }

    /// Check if a server appears to be an official provider for the requested capability
    fn is_official_provider(&self, server_name: &str, requested_capability: &str) -> bool {
        // Extract domain/service name from requested capability
        let requested_parts: Vec<&str> = requested_capability.split('.').collect();
        if requested_parts.is_empty() {
            return false;
        }

        // Check if server name contains the main domain/service
        let main_domain = requested_parts[0];
        server_name.contains(main_domain)
    }

    /// Check if a query looks like a semantic description rather than a capability ID
    fn is_semantic_query(&self, query: &str) -> bool {
        let query_lower = query.to_lowercase();

        // Check for semantic indicators
        let semantic_indicators = [
            "official",
            "api",
            "service",
            "client",
            "sdk",
            "provider",
            "weather",
            "github",
            "database",
            "payment",
            "email",
            "storage",
            "authentication",
            "analytics",
            "monitoring",
            "logging",
        ];

        // If it contains spaces or semantic keywords, treat as semantic query
        query.contains(' ') || 
        semantic_indicators.iter().any(|indicator| query_lower.contains(indicator)) ||
        // If it doesn't look like a capability ID (no dots, not camelCase)
        (!query.contains('.') && !query.chars().any(|c| c.is_uppercase()))
    }

    /// Extract semantic keywords from a capability search query
    fn extract_search_keywords(&self, query: &str) -> Vec<String> {
        let query_lower = query.to_lowercase();
        let mut keywords = Vec::new();

        // Split by common separators
        let parts: Vec<&str> = query_lower
            .split(|c: char| c.is_whitespace() || c == '.' || c == '-' || c == '_')
            .filter(|part| !part.is_empty() && part.len() > 1)
            .collect();

        // Add all parts as keywords
        keywords.extend(parts.iter().map(|s| s.to_string()));

        // Add common API/service keywords if not present
        let api_keywords = ["api", "service", "client", "sdk", "official", "provider"];
        for keyword in &api_keywords {
            if !keywords.contains(&keyword.to_string()) && query_lower.contains(keyword) {
                keywords.push(keyword.to_string());
            }
        }

        keywords
    }

    /// Perform semantic search across MCP servers using keywords
    async fn semantic_search_servers(
        &self,
        keywords: &[String],
    ) -> RuntimeResult<Vec<crate::ccos::synthesis::mcp_registry_client::McpServer>> {
        let registry_client = crate::ccos::synthesis::mcp_registry_client::McpRegistryClient::new();
        let mut all_servers = Vec::new();

        // Search for each keyword
        for keyword in keywords {
            match registry_client.find_capability_providers(keyword).await {
                Ok(servers) => {
                    all_servers.extend(servers);
                }
                Err(e) => {
                    eprintln!(
                        "⚠️ SEMANTIC SEARCH: Failed to search for '{}': {}",
                        keyword, e
                    );
                }
            }
        }

        // Remove duplicates based on server name
        let mut unique_servers = Vec::new();
        let mut seen_names = std::collections::HashSet::new();

        for server in all_servers {
            if seen_names.insert(server.name.clone()) {
                unique_servers.push(server);
            }
        }

        Ok(unique_servers)
    }

    /// Interactive server selection when multiple good options are available
    async fn interactive_server_selection<'a>(
        &self,
        capability_id: &str,
        ranked_servers: &'a [RankedMcpServer],
    ) -> RuntimeResult<&'a RankedMcpServer> {
        println!("\n🔍 Multiple MCP servers found for '{}':", capability_id);
        println!("Please select the most appropriate one:\n");

        for (i, ranked) in ranked_servers.iter().enumerate() {
            let server = &ranked.server;
            println!("{}. {} (Score: {:.2})", i + 1, server.name, ranked.score);
            println!("   Description: {}", server.description);
            if let Some(ref repository) = server.repository {
                println!("   Repository: {:?}", repository);
            }
            println!();
        }

        // For now, return the highest scored server
        // TODO: Implement actual user input in a real CLI environment
        println!(
            "🤖 Auto-selecting highest scored server: {}",
            ranked_servers[0].server.name
        );
        Ok(&ranked_servers[0])
    }

    fn is_capability_match(
        &self,
        requested: &str,
        capability_name: &str,
        capability_desc: &str,
    ) -> bool {
        let requested_lower = requested.to_lowercase();
        let name_lower = capability_name.to_lowercase();
        let desc_lower = capability_desc.to_lowercase();

        // Exact match
        if name_lower == requested_lower {
            return true;
        }

        // Partial match in capability name
        if name_lower.contains(&requested_lower) || requested_lower.contains(&name_lower) {
            return true;
        }

        // Check if requested capability is mentioned in description
        if desc_lower.contains(&requested_lower) {
            return true;
        }

        // Check for domain-based matching (e.g., "travel.flights" matches "flights")
        let requested_parts: Vec<&str> = requested.split('.').collect();
        if requested_parts.len() > 1 {
            let last_part = requested_parts.last().unwrap().to_lowercase();
            if name_lower.contains(&last_part) {
                return true;
            }
        }

        false
    }

    /// Discover capabilities via web search
    async fn discover_via_web_search(
        &self,
        capability_id: &str,
    ) -> RuntimeResult<Option<CapabilityManifest>> {
        if self.config.verbose_logging {
            eprintln!("🔍 DISCOVERY: Searching web for '{}'", capability_id);
        }

        // Use web search to find API specs and documentation
        let mut web_searcher =
            crate::ccos::synthesis::web_search_discovery::WebSearchDiscovery::new(
                "auto".to_string(),
            );

        // Search for OpenAPI specs, API docs, etc.
        let search_results = web_searcher.search_for_api_specs(capability_id).await?;

        if search_results.is_empty() {
            if self.config.verbose_logging {
                eprintln!(
                    "🔍 DISCOVERY: No web search results found for '{}'",
                    capability_id
                );
            }
            return Ok(None);
        }

        // Try to convert the best result to a capability manifest
        if let Some(best_result) = search_results.first() {
            if self.config.verbose_logging {
                eprintln!(
                    "🔍 DISCOVERY: Found web result: {} ({})",
                    best_result.title, best_result.result_type
                );
            }

            // Try to import as OpenAPI if it's an OpenAPI spec
            if best_result.result_type == "openapi_spec" {
                return self
                    .import_openapi_from_url(&best_result.url, capability_id)
                    .await;
            }

            // Try to import as generic HTTP API
            return self
                .import_http_api_from_url(&best_result.url, capability_id)
                .await;
        }

        Ok(None)
    }

    /// Import OpenAPI spec from URL
    async fn import_openapi_from_url(
        &self,
        url: &str,
        capability_id: &str,
    ) -> RuntimeResult<Option<CapabilityManifest>> {
        if self.config.verbose_logging {
            eprintln!(
                "📥 DISCOVERY: Attempting to import OpenAPI spec from: {}",
                url
            );
        }

        // Create OpenAPI importer
        let importer =
            crate::ccos::synthesis::openapi_importer::OpenAPIImporter::new(url.to_string());

        // Create complete RTFS capability from OpenAPI spec
        match importer.create_rtfs_capability(url, capability_id).await {
            Ok(manifest) => {
                if self.config.verbose_logging {
                    eprintln!(
                        "✅ DISCOVERY: Created RTFS capability from OpenAPI: {}",
                        manifest.id
                    );
                    if let Some(rtfs_code) = manifest.metadata.get("rtfs_code") {
                        eprintln!("📝 RTFS Code generated ({} chars)", rtfs_code.len());
                    }
                }
                Ok(Some(manifest))
            }
            Err(e) => {
                if self.config.verbose_logging {
                    eprintln!(
                        "⚠️ DISCOVERY: Failed to create RTFS capability from OpenAPI: {}",
                        e
                    );
                }
                Ok(None)
            }
        }
    }

    /// Import generic HTTP API from URL
    async fn import_http_api_from_url(
        &self,
        url: &str,
        capability_id: &str,
    ) -> RuntimeResult<Option<CapabilityManifest>> {
        if self.config.verbose_logging {
            eprintln!("📥 DISCOVERY: Attempting to import HTTP API from: {}", url);
        }

        let base_url = Self::infer_base_url(url);
        let provider_slug = Self::infer_provider_slug(capability_id, &base_url);
        let env_var_name = Self::env_var_name_for_slug(&provider_slug);
        let primary_query_param =
            Self::infer_primary_query_param(&provider_slug, capability_id, &base_url);
        let fallback_query_param = Self::infer_secondary_query_param(&primary_query_param);

        // Create a generic HTTP API capability
        let manifest = crate::ccos::capability_marketplace::types::CapabilityManifest {
            id: capability_id.to_string(),
            name: format!("{} API", capability_id),
            description: format!("HTTP API discovered from {}", url),
            version: "1.0.0".to_string(),
            provider: crate::ccos::capability_marketplace::types::ProviderType::Http(
                crate::ccos::capability_marketplace::types::HttpCapability {
                    base_url: base_url.clone(),
                    auth_token: None,
                    timeout_ms: 30000,
                },
            ),
            input_schema: None,  // TODO: Parse from API docs
            output_schema: None, // TODO: Parse from API docs
            attestation: None,
            provenance: Some(
                crate::ccos::capability_marketplace::types::CapabilityProvenance {
                    source: "web_search_discovery".to_string(),
                    version: Some("1.0.0".to_string()),
                    content_hash: format!("web_{}", url.replace("/", "_")),
                    custody_chain: vec!["web_search".to_string()],
                    registered_at: chrono::Utc::now(),
                },
            ),
            permissions: vec!["network.http".to_string()],
            effects: vec!["network_request".to_string()],
            metadata: {
                let mut meta = std::collections::HashMap::new();
                meta.insert("discovery_method".to_string(), "web_search".to_string());
                meta.insert("source_url".to_string(), url.to_string());
                meta.insert("base_url".to_string(), base_url.clone());
                meta.insert("provider_slug".to_string(), provider_slug.clone());
                meta.insert("api_type".to_string(), "http_rest".to_string());
                meta.insert("auth_env_var".to_string(), env_var_name.clone());
                meta.insert("auth_query_param".to_string(), primary_query_param.clone());
                meta.insert(
                    "auth_secondary_query_param".to_string(),
                    fallback_query_param.clone(),
                );
                meta
            },
            agent_metadata: None,
        };

        if self.config.verbose_logging {
            eprintln!(
                "✅ DISCOVERY: Created generic HTTP API capability: {}",
                manifest.id
            );
        }

        // Save the capability to storage
        self.save_generic_capability(&manifest, url).await?;

        Ok(Some(manifest))
    }

    /// Save a generic HTTP API capability to storage
    async fn save_generic_capability(
        &self,
        manifest: &CapabilityManifest,
        source_url: &str,
    ) -> RuntimeResult<()> {
        let storage_dir = std::env::var("CCOS_CAPABILITY_STORAGE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("./capabilities"));

        std::fs::create_dir_all(&storage_dir).map_err(|e| {
            RuntimeError::Generic(format!("Failed to create storage directory: {}", e))
        })?;

        let capability_dir = storage_dir.join(&manifest.id);
        std::fs::create_dir_all(&capability_dir).map_err(|e| {
            RuntimeError::Generic(format!("Failed to create capability directory: {}", e))
        })?;

        let input_schema_rtfs = manifest
            .input_schema
            .as_ref()
            .map(Self::type_expr_to_rtfs_string)
            .unwrap_or_else(|| ":any".to_string());

        let output_schema_rtfs = manifest
            .output_schema
            .as_ref()
            .map(Self::type_expr_to_rtfs_string)
            .unwrap_or_else(|| ":any".to_string());

        let provider_slug = manifest
            .metadata
            .get("provider_slug")
            .cloned()
            .unwrap_or_else(|| Self::infer_provider_slug(&manifest.id, source_url));

        let base_url = manifest
            .metadata
            .get("base_url")
            .cloned()
            .unwrap_or_else(|| Self::infer_base_url(source_url));

        let env_var_name = manifest
            .metadata
            .get("auth_env_var")
            .cloned()
            .unwrap_or_else(|| Self::env_var_name_for_slug(&provider_slug));

        let primary_query_param = manifest
            .metadata
            .get("auth_query_param")
            .cloned()
            .unwrap_or_else(|| {
                Self::infer_primary_query_param(&provider_slug, &manifest.id, &base_url)
            });

        let fallback_query_param = manifest
            .metadata
            .get("auth_secondary_query_param")
            .cloned()
            .unwrap_or_else(|| Self::infer_secondary_query_param(&primary_query_param));

        let timestamp = chrono::Utc::now().to_rfc3339();

        let metadata_path = capability_dir.join("capability.rtfs");
        let metadata_rtfs = format!(
            r#";; Capability metadata for {0}
;; Generated from web discovery
;; Source URL: {1}

(capability "{2}"
  :name "{3}"
  :version "{4}"
  :description "{5}"
  :source_url "{1}"
  :discovery_method "web_search"
  :created_at "{6}"
  :capability_type "generic_http_api"
  :permissions [:network.http]
  :effects [:network_request]
  :input-schema {7}
  :output-schema {8}
  :implementation
    (do
      ;; binding input convention: the host passes a single value as 'input'
      ;; input may be a string endpoint, a list of keyword pairs, or a map
      (defn ensure_url [base maybe]
        (if (or (starts-with? maybe "http://") (starts-with? maybe "https://"))
          maybe
          (if (starts-with? maybe "/")
            (str base maybe)
            (str base "/" maybe))))

      (defn normalize_to_map [in]
        (if (string? in)
          {{:method "GET" :url in}}
          (if (list? in)
            (apply hash-map in)
            (if (map? in)
              in
              {{:method "GET" :url (str in)}}))))

      (defn has_query? [url]
        (> (count (split url "?")) 1))

      (defn append_query [url param value]
        (if (has_query? url)
          (str url "&" param "=" value)
          (str url "?" param "=" value)))

      (defn ensure_parameter [url param value]
        (if (or (not value) (= value ""))
          url
          (let [string_value (str value)
                marker (str param "=")]
            (if (> (count (split url marker)) 1)
              url
              (append_query url param string_value)))))

      (let [base "{9}"
            req (normalize_to_map input)
            url (ensure_url base (get req :url))
            method (or (get req :method) "GET")
            headers (or (get req :headers) {{}})
            body (or (get req :body) "")
            token (or (get req :api_key)
                      (get req :apikey)
                      (get req :key)
                      (get req :token)
                      (get req :access_token)
                      (get req :appid)
                      (call "ccos.system.get-env" "{10}"))
            url_with_primary (ensure_parameter url "{11}" token)
            final_url (ensure_parameter url_with_primary "{12}" token)]
        (call "ccos.network.http-fetch"
          :method method
          :url final_url
          :headers headers
          :body body))))

;; Optional helpers (commented out)
;; (defn is-recent? [] (< (days-since (parse-date "{6}")) 30))
;; (defn source-domain [] (second (split "{1}" "/")))
"#,
            manifest.name,
            source_url,
            manifest.id,
            manifest.name,
            manifest.version,
            manifest.description,
            timestamp,
            input_schema_rtfs,
            output_schema_rtfs,
            base_url,
            env_var_name,
            primary_query_param,
            fallback_query_param,
        );

        std::fs::write(&metadata_path, metadata_rtfs).map_err(|e| {
            RuntimeError::Generic(format!("Failed to write capability metadata: {}", e))
        })?;

        if self.config.verbose_logging {
            eprintln!(
                "💾 Saved generic capability to: {}",
                capability_dir.display()
            );
        }

        Ok(())
    }

    fn infer_provider_slug(capability_id: &str, url: &str) -> String {
        let mut source = capability_id.to_string();

        if let Some(after_scheme) = url.split("//").nth(1) {
            let host = after_scheme.split('/').next().unwrap_or(after_scheme);
            let host = host.split(':').next().unwrap_or(host);
            if !host.is_empty() {
                source = host.replace('.', "_").replace('-', "_");
            }
        }

        source
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect::<String>()
            .trim_matches('_')
            .to_string()
    }

    fn env_var_name_for_slug(slug: &str) -> String {
        let slug = slug
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_uppercase()
                } else {
                    '_'
                }
            })
            .collect::<String>();
        let trimmed = slug.trim_matches('_');

        if trimmed.is_empty() {
            "API_KEY".to_string()
        } else {
            format!("{}_API_KEY", trimmed)
        }
    }

    fn infer_primary_query_param(provider_slug: &str, capability_id: &str, url: &str) -> String {
        let mut candidates = Vec::new();

        let slug_lower = provider_slug.to_ascii_lowercase();
        let id_lower = capability_id.to_ascii_lowercase();
        let url_lower = url.to_ascii_lowercase();

        if slug_lower.contains("token") || id_lower.contains("token") {
            candidates.extend(["token", "access_token", "auth_token"]);
        }

        if slug_lower.contains("secret") || id_lower.contains("secret") {
            candidates.extend(["secret", "client_secret"]);
        }

        if slug_lower.contains("client") || id_lower.contains("client") {
            candidates.extend(["client_id", "clientid"]);
        }

        if slug_lower.contains("app") || id_lower.contains("app") {
            candidates.extend(["app_id", "appid"]);
        }

        if url_lower.contains("token") {
            candidates.extend(["token", "access_token"]);
        }

        if url_lower.contains("client") {
            candidates.extend(["client_id", "clientid"]);
        }

        candidates.extend(["api_key", "apikey", "key", "auth", "authorization"]);

        candidates
            .into_iter()
            .find(|candidate| !candidate.is_empty())
            .unwrap_or("api_key")
            .to_string()
    }

    fn infer_secondary_query_param(primary: &str) -> String {
        match primary {
            "api_key" => "apikey".to_string(),
            "apikey" => "key".to_string(),
            "key" => "api_key".to_string(),
            "token" => "access_token".to_string(),
            "access_token" => "token".to_string(),
            "app_id" => "appid".to_string(),
            "appid" => "app_id".to_string(),
            other => other.to_string(),
        }
    }

    fn type_expr_to_rtfs_string(expr: &TypeExpr) -> String {
        expr.to_string()
    }

    fn infer_base_url(url: &str) -> String {
        if let Some(idx) = url.find("://") {
            let scheme = &url[..idx];
            let rest = &url[idx + 3..];
            let host_port = rest
                .split(|c| c == '/' || c == '?' || c == '#')
                .next()
                .unwrap_or("");

            if !host_port.is_empty() {
                let mut base = String::with_capacity(scheme.len() + host_port.len() + 3);
                base.push_str(scheme);
                base.push_str("://");
                base.push_str(host_port);
                return base.trim_end_matches('/').to_string();
            }
        }

        url.trim_end_matches('/').to_string()
    }

    /// Discover capabilities from network catalogs
    async fn discover_network_catalogs(
        &self,
        capability_id: &str,
    ) -> RuntimeResult<Option<CapabilityManifest>> {
        if self.config.verbose_logging {
            eprintln!(
                "🔍 DISCOVERY: Querying network catalogs for '{}'",
                capability_id
            );
        }

        // TODO: Implement network catalog discovery
        // This would query external capability catalogs/registries
        // and check if any match the requested capability_id

        // For now, return None as this is not implemented
        Ok(None)
    }

    /// Process pending resolution requests
    pub async fn process_queue(&self) -> RuntimeResult<()> {
        let mut processed_count = 0;
        const MAX_BATCH_SIZE: usize = 10;

        while processed_count < MAX_BATCH_SIZE {
            let request = {
                let mut queue = self.queue.lock().unwrap();
                queue.dequeue()
            };

            match request {
                Some(request) => {
                    let result = self.resolve_capability(&request).await?;

                    {
                        let mut queue = self.queue.lock().unwrap();
                        queue.mark_completed(&request.capability_id, result.clone());
                    }

                    if self.config.verbose_logging {
                        match &result {
                            ResolutionResult::Resolved {
                                resolution_method, ..
                            } => {
                                eprintln!(
                                    "✅ RESOLVED: '{}' via {}",
                                    request.capability_id, resolution_method
                                );
                                eprintln!(
                                    "CAPABILITY_AUDIT: {:?}",
                                    std::collections::HashMap::from([
                                        (
                                            "event_type".to_string(),
                                            "capability_resolved".to_string()
                                        ),
                                        (
                                            "capability_id".to_string(),
                                            request.capability_id.clone()
                                        ),
                                    ])
                                );
                            }
                            ResolutionResult::Failed {
                                reason,
                                retry_after: _retry_after,
                                ..
                            } => {
                                eprintln!("❌ FAILED: '{}' - {}", request.capability_id, reason);
                            }
                            ResolutionResult::PermanentlyFailed { reason, .. } => {
                                eprintln!(
                                    "🚫 PERMANENTLY FAILED: '{}' - {}",
                                    request.capability_id, reason
                                );
                            }
                        }
                    }

                    processed_count += 1;
                }
                None => break,
            }
        }

        Ok(())
    }

    /// Get resolver statistics
    pub fn get_stats(&self) -> QueueStats {
        let queue = self.queue.lock().unwrap();
        queue.stats()
    }

    /// Get the checkpoint archive reference
    pub fn get_checkpoint_archive(&self) -> &Arc<CheckpointArchive> {
        &self.checkpoint_archive
    }

    /// Emit audit event for missing capability
    fn emit_missing_capability_audit(&self, capability_id: &str) -> RuntimeResult<()> {
        // Create audit event data similar to dependency extractor
        let audit_data = std::collections::HashMap::from([
            ("missing_capability".to_string(), capability_id.to_string()),
            ("resolution_queued".to_string(), "true".to_string()),
            (
                "timestamp".to_string(),
                format!("{:?}", std::time::SystemTime::now()),
            ),
        ]);

        eprintln!(
            "AUDIT: missing_capability_runtime - {}",
            audit_data
                .get("missing_capability")
                .unwrap_or(&"unknown".to_string())
        );

        Ok(())
    }

    /// Trigger auto-resume for any checkpoints waiting for a specific capability
    pub async fn trigger_auto_resume_for_capability(
        &self,
        capability_id: &str,
    ) -> RuntimeResult<()> {
        if self.config.verbose_logging {
            eprintln!(
                "🔄 AUTO-RESUME: Checking for checkpoints waiting for capability '{}'",
                capability_id
            );
        }

        // Find all checkpoints waiting for this capability
        let waiting_checkpoints = self
            .checkpoint_archive
            .find_checkpoints_waiting_for_capability(capability_id);

        if waiting_checkpoints.is_empty() {
            if self.config.verbose_logging {
                eprintln!(
                    "ℹ️ AUTO-RESUME: No checkpoints waiting for capability '{}'",
                    capability_id
                );
            }
            return Ok(());
        }

        if self.config.verbose_logging {
            eprintln!(
                "🔄 AUTO-RESUME: Found {} checkpoints waiting for capability '{}'",
                waiting_checkpoints.len(),
                capability_id
            );
        }

        // For each waiting checkpoint, check if all missing capabilities are now resolved
        for checkpoint in waiting_checkpoints {
            if self.can_resume_checkpoint(&checkpoint).await? {
                if self.config.verbose_logging {
                    eprintln!("✅ AUTO-RESUME: All capabilities resolved for checkpoint '{}', ready for resume", checkpoint.checkpoint_id);
                }

                // Emit audit event for auto-resume readiness
                self.emit_auto_resume_ready_audit(&checkpoint.checkpoint_id, &checkpoint.plan_id)?;

                // Note: Actual resume is triggered by the orchestrator when it calls resume_plan
                // The checkpoint remains in the archive until explicitly resumed and removed
            } else {
                if self.config.verbose_logging {
                    eprintln!(
                        "⏳ AUTO-RESUME: Checkpoint '{}' still waiting for other capabilities",
                        checkpoint.checkpoint_id
                    );
                }
            }
        }

        Ok(())
    }

    /// Check if a checkpoint can be resumed (all missing capabilities are resolved)
    async fn can_resume_checkpoint(
        &self,
        checkpoint: &crate::ccos::checkpoint_archive::CheckpointRecord,
    ) -> RuntimeResult<bool> {
        let capabilities = self.marketplace.capabilities.read().await;

        for missing_capability in &checkpoint.missing_capabilities {
            if !capabilities.contains_key(missing_capability) {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Emit audit event when a checkpoint is ready for auto-resume
    fn emit_auto_resume_ready_audit(
        &self,
        checkpoint_id: &str,
        plan_id: &str,
    ) -> RuntimeResult<()> {
        let audit_data = serde_json::json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "checkpoint_id": checkpoint_id,
            "plan_id": plan_id,
            "event_type": "checkpoint_ready_for_resume",
            "auto_resume_triggered": true,
            "missing_capabilities_resolved": true
        });

        eprintln!("AUDIT_EVENT: {}", audit_data);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ccos::capabilities::registry::CapabilityRegistry;
    use tokio::sync::RwLock;
    use crate::ccos::synthesis::feature_flags::MissingCapabilityFeatureFlags;

    #[tokio::test]
    async fn test_missing_capability_queue() {
        let mut queue = MissingCapabilityQueue::new();
        assert!(!queue.has_pending());

        let request = MissingCapabilityRequest {
            capability_id: "test.capability".to_string(),
            arguments: vec![],
            context: HashMap::new(),
            requested_at: std::time::SystemTime::now(),
            attempt_count: 0,
        };

        queue.enqueue(request.clone());
        assert!(queue.has_pending());

        let stats = queue.stats();
        assert_eq!(stats.pending_count, 1);
        assert_eq!(stats.in_progress_count, 0);

        let dequeued = queue.dequeue();
        assert!(dequeued.is_some());
        assert_eq!(dequeued.unwrap().capability_id, "test.capability");

        let stats = queue.stats();
        assert_eq!(stats.pending_count, 0);
        assert_eq!(stats.in_progress_count, 1);
    }

    #[tokio::test]
    async fn test_missing_capability_resolver() {
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));
        let checkpoint_archive = Arc::new(CheckpointArchive::new());
        // Use a testing feature configuration that enables runtime detection
        // so that the resolver will actually enqueue missing capabilities
        // during unit tests.
    let mut test_cfg = MissingCapabilityConfig::default();
    test_cfg.feature_flags = MissingCapabilityFeatureFlags::testing();

        let resolver = MissingCapabilityResolver::new(
            marketplace,
            checkpoint_archive,
            ResolverConfig::default(),
            test_cfg,
        );

        let mut context = HashMap::new();
        context.insert("plan_id".to_string(), "test_plan".to_string());
        context.insert("intent_id".to_string(), "test_intent".to_string());

        // Test handling missing capability
        let result =
            resolver.handle_missing_capability("missing.capability".to_string(), vec![], context);
        assert!(result.is_ok());

        let stats = resolver.get_stats();
        assert_eq!(stats.pending_count, 1);
    }
}
