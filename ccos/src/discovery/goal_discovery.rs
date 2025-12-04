use crate::discovery::approval_queue::{
    ApprovalQueue, PendingDiscovery, RiskAssessment, RiskLevel,
};
use crate::discovery::capability_matcher::calculate_description_match_score_improved;
use crate::discovery::config::DiscoveryConfig;
use crate::discovery::llm_discovery::{IntentAnalysis, LlmDiscoveryService};
use crate::discovery::registry_search::{RegistrySearchResult, RegistrySearcher};
use chrono::Utc;
use rtfs::runtime::error::RuntimeResult;
use uuid::Uuid;

/// Scored search result for ranking (internal)
struct ScoredResult {
    result: RegistrySearchResult,
    score: f64,
}

/// Public type alias for scored results returned by search_and_score
pub type ScoredSearchResult = (RegistrySearchResult, f64);

/// Result of LLM-enhanced search containing both results and analysis
#[derive(Debug)]
pub struct LlmSearchResult {
    /// The scored search results
    pub results: Vec<ScoredSearchResult>,
    /// Intent analysis from LLM (if llm_enabled)
    pub intent_analysis: Option<IntentAnalysis>,
    /// Whether LLM was used
    pub llm_used: bool,
}

pub struct GoalDiscoveryAgent {
    registry_searcher: RegistrySearcher,
    approval_queue: ApprovalQueue,
    config: DiscoveryConfig,
}

impl GoalDiscoveryAgent {
    pub fn new(approval_queue: ApprovalQueue) -> Self {
        Self {
            registry_searcher: RegistrySearcher::new(),
            approval_queue,
            config: DiscoveryConfig::from_env(),
        }
    }

    /// Create with custom config
    pub fn new_with_config(approval_queue: ApprovalQueue, config: DiscoveryConfig) -> Self {
        Self {
            registry_searcher: RegistrySearcher::new(),
            approval_queue,
            config,
        }
    }

    /// Process goal and queue all matching results (original behavior)
    pub async fn process_goal(&self, goal: &str) -> RuntimeResult<Vec<String>> {
        let scored_results = self.search_and_score(goal, false).await?;

        let mut queued_ids = Vec::new();
        for (result, score) in scored_results {
            let id = self.queue_result(goal, result, score)?;
            queued_ids.push(id);
        }

        Ok(queued_ids)
    }

    /// Search and score results without queuing (for interactive mode)
    pub async fn search_and_score(
        &self,
        goal: &str,
        llm_enabled: bool,
    ) -> RuntimeResult<Vec<ScoredSearchResult>> {
        let result = self.search_and_score_with_llm(goal, llm_enabled).await?;
        Ok(result.results)
    }

    /// Search and score results with full LLM analysis info
    pub async fn search_and_score_with_llm(
        &self,
        goal: &str,
        llm_enabled: bool,
    ) -> RuntimeResult<LlmSearchResult> {
        let debug = std::env::var("CCOS_DEBUG").is_ok();

        // If LLM enabled, try to use it for enhanced discovery
        if llm_enabled {
            match self.search_with_llm(goal).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    log::warn!(
                        "LLM discovery failed, falling back to keyword matching: {}",
                        e
                    );
                    println!(
                        "⚠️  LLM analysis failed: {}. Falling back to keyword matching.",
                        e
                    );
                }
            }
        }

        // Keyword-based search (fallback or default)
        let results = self.keyword_search_and_score(goal).await?;

        if debug && llm_enabled {
            eprintln!("📊 Using keyword-based scoring (LLM fallback)");
        }

        Ok(LlmSearchResult {
            results,
            intent_analysis: None,
            llm_used: false,
        })
    }

    /// LLM-enhanced search: analyze goal, expand queries, rank semantically
    async fn search_with_llm(&self, goal: &str) -> RuntimeResult<LlmSearchResult> {
        let debug = std::env::var("CCOS_DEBUG").is_ok();

        println!("🧠 Analyzing goal with LLM...");

        // Create LLM discovery service
        let llm_service = LlmDiscoveryService::new().await?;

        // Step 1: Analyze the goal to extract intent and expand queries
        let intent = llm_service.analyze_goal(goal).await?;

        if debug {
            eprintln!("📊 LLM Intent Analysis:");
            eprintln!("   Action: {}", intent.primary_action);
            eprintln!("   Target: {}", intent.target_object);
            eprintln!("   Keywords: {:?}", intent.domain_keywords);
            eprintln!("   Implied: {:?}", intent.implied_concepts);
            eprintln!("   Queries: {:?}", intent.expanded_queries);
            eprintln!("   Confidence: {:.2}", intent.confidence);
        }

        println!(
            "   Intent: {} {} (confidence: {:.0}%)",
            intent.primary_action,
            intent.target_object,
            intent.confidence * 100.0
        );

        // Step 2: Build search query set from intent
        // Collect all unique queries: original goal + domain keywords + expanded queries
        let mut search_queries: Vec<String> = Vec::new();
        let mut seen_queries: std::collections::HashSet<String> = std::collections::HashSet::new();

        // Add original goal
        search_queries.push(goal.to_string());
        seen_queries.insert(goal.to_lowercase());

        // Add domain keywords (important: these are often single service names like "github")
        // The MCP registry works best with single-word or specific service names
        for keyword in &intent.domain_keywords {
            let lower = keyword.to_lowercase();
            if !lower.is_empty() && seen_queries.insert(lower) {
                search_queries.push(keyword.clone());
            }
        }

        // Add expanded queries
        for query in &intent.expanded_queries {
            let lower = query.to_lowercase();
            if !lower.is_empty() && seen_queries.insert(lower) {
                search_queries.push(query.clone());
            }
        }

        println!(
            "🔍 Searching registries with {} queries...",
            search_queries.len()
        );

        let mut all_results: Vec<RegistrySearchResult> = Vec::new();
        let mut seen_names: std::collections::HashSet<String> = std::collections::HashSet::new();

        // Search with all queries
        for query in &search_queries {
            match self.registry_searcher.search(query).await {
                Ok(results) => {
                    for result in results {
                        if seen_names.insert(result.server_info.name.clone()) {
                            all_results.push(result);
                        }
                    }
                }
                Err(e) => {
                    if debug {
                        eprintln!("   ⚠️  Query '{}' failed: {}", query, e);
                    }
                }
            }
        }

        if debug {
            eprintln!(
                "🔍 Total unique results from all queries: {}",
                all_results.len()
            );
        }

        if all_results.is_empty() {
            return Ok(LlmSearchResult {
                results: Vec::new(),
                intent_analysis: Some(intent),
                llm_used: true,
            });
        }

        // Step 3: Pre-filter with keyword matching to reduce LLM ranking cost
        let pre_filtered: Vec<RegistrySearchResult> = all_results
            .into_iter()
            .filter(|r| {
                let name = r.server_info.name.to_lowercase();
                let desc = r
                    .server_info
                    .description
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase();

                // Keep if any intent keyword matches name or description
                let keywords: Vec<&str> = intent
                    .domain_keywords
                    .iter()
                    .chain(intent.synonyms.iter())
                    .chain(intent.implied_concepts.iter())
                    .map(|s| s.as_str())
                    .collect();

                if keywords.is_empty() {
                    return true;
                }

                keywords
                    .iter()
                    .any(|kw| name.contains(kw) || desc.contains(kw))
            })
            .take(15) // Limit candidates for LLM ranking (cost control)
            .collect();

        if debug {
            eprintln!(
                "🔍 Pre-filtered to {} candidates for LLM ranking",
                pre_filtered.len()
            );
        }

        if pre_filtered.is_empty() {
            // Fall back to keyword scoring if no candidates pass filter
            let results = self.keyword_search_and_score(goal).await?;
            return Ok(LlmSearchResult {
                results,
                intent_analysis: Some(intent),
                llm_used: true,
            });
        }

        // Step 4: Rank candidates with LLM
        println!("🎯 Ranking {} candidates with LLM...", pre_filtered.len());

        let ranked = llm_service
            .rank_results(goal, Some(&intent), pre_filtered)
            .await?;

        // Convert ranked results to scored results, applying threshold
        let threshold = self.config.match_threshold;
        let results: Vec<ScoredSearchResult> = ranked
            .into_iter()
            .filter(|r| r.llm_score >= threshold)
            .map(|r| (r.result, r.llm_score))
            .collect();

        if debug {
            eprintln!(
                "🎯 {} results above threshold ({:.2})",
                results.len(),
                threshold
            );
        }

        Ok(LlmSearchResult {
            results,
            intent_analysis: Some(intent),
            llm_used: true,
        })
    }

    /// Keyword-based search and scoring (original implementation)
    async fn keyword_search_and_score(&self, goal: &str) -> RuntimeResult<Vec<ScoredSearchResult>> {
        // Search registries
        let results = self.registry_searcher.search(goal).await?;

        let total_candidates = results.len();
        let debug = std::env::var("CCOS_DEBUG").is_ok();

        // Pre-extract domain keywords from goal (non-action-verbs)
        let action_verbs = [
            "list", "get", "create", "update", "delete", "show", "find", "search", "fetch",
            "retrieve", "read", "post", "add", "remove", "modify", "edit",
        ];
        let goal_lower = goal.to_lowercase();
        let goal_domains: Vec<String> = goal_lower
            .split_whitespace()
            .filter(|w| !action_verbs.contains(w) && w.len() > 2)
            .map(|s| s.to_string())
            .collect();

        if debug {
            eprintln!("📊 Goal domain keywords: {:?}", goal_domains);
        }

        let mut scored_results: Vec<ScoredResult> = results
            .into_iter()
            .filter_map(|result| {
                let description = result.server_info.description.as_deref().unwrap_or("");
                let name = &result.server_info.name;

                let desc_lower = description.to_lowercase();
                let name_lower = name.to_lowercase();

                // Domain matching with multiple keywords
                let url_pattern = regex::Regex::new(r"https?://[^\s]+").unwrap();
                let desc_no_urls = url_pattern.replace_all(&desc_lower, " ");

                // Count matches in name vs description separately
                let name_matches: usize = goal_domains
                    .iter()
                    .filter(|d| name_lower.contains(d.as_str()))
                    .count();

                let desc_matches: usize = goal_domains
                    .iter()
                    .filter(|d| {
                        let word_pattern = format!(r"\b{}\b", regex::escape(d));
                        regex::Regex::new(&word_pattern)
                            .map(|re| re.is_match(&desc_no_urls))
                            .unwrap_or(false)
                    })
                    .count();

                // Determine if this is a domain match
                let domain_match = if goal_domains.is_empty() {
                    true
                } else if name_matches > 0 {
                    true
                } else {
                    let required = if goal_domains.len() <= 1 {
                        1
                    } else {
                        (goal_domains.len() * 2 + 2) / 3
                    };
                    desc_matches >= required
                };

                if !domain_match {
                    if debug {
                        eprintln!(
                            "   ❌ FILTERED {} - no domain match in '{}'",
                            name,
                            desc_lower.chars().take(50).collect::<String>()
                        );
                    }
                    return None;
                }

                // Calculate base score
                let base_score = calculate_description_match_score_improved(
                    goal,
                    description,
                    name,
                    "",
                    name,
                    &self.config,
                );

                // Boost score for name matches
                let name_domain_matches = goal_domains
                    .iter()
                    .filter(|d| name_lower.contains(d.as_str()))
                    .count();

                let name_bonus = if name_domain_matches > 0 && !goal_domains.is_empty() {
                    let match_ratio = name_domain_matches as f64 / goal_domains.len() as f64;
                    let desc_is_garbage = description.len() < 50
                        || description.starts_with("<")
                        || description.starts_with("&lt;");

                    if desc_is_garbage && match_ratio >= 0.5 {
                        0.7
                    } else {
                        match_ratio * 0.5
                    }
                } else {
                    0.0
                };

                let score = (base_score + name_bonus).min(1.0);

                if debug {
                    eprintln!(
                        "   ✅ [{:.2}] {} - base:{:.2} name_bonus:{:.2} (domains_in_name:{})",
                        score, name, base_score, name_bonus, name_domain_matches
                    );
                }

                Some(ScoredResult { result, score })
            })
            .collect();

        // Sort by score descending
        scored_results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Filter by threshold
        let threshold = self.config.match_threshold;
        let filtered_results: Vec<ScoredSearchResult> = scored_results
            .into_iter()
            .filter(|sr| sr.score >= threshold)
            .map(|sr| (sr.result, sr.score))
            .collect();

        let filtered_count = filtered_results.len();

        if debug {
            eprintln!(
                "🔍 Discovery: {} candidates → {} above threshold ({:.2})",
                total_candidates, filtered_count, threshold
            );
        }

        Ok(filtered_results)
    }

    /// Queue a single result for approval
    pub fn queue_result(
        &self,
        goal: &str,
        result: RegistrySearchResult,
        score: f64,
    ) -> RuntimeResult<String> {
        let keywords: Vec<&str> = goal.split_whitespace().collect();
        let id = format!("discovery-{}", Uuid::new_v4());

        let risk = if result.server_info.endpoint.starts_with("https://") {
            RiskLevel::Medium
        } else {
            RiskLevel::High
        };

        // Merge alternative_endpoints from RegistrySearchResult into ServerInfo
        let mut server_info = result.server_info;
        if !result.alternative_endpoints.is_empty() {
            server_info
                .alternative_endpoints
                .extend(result.alternative_endpoints);
            server_info.alternative_endpoints.sort();
            server_info.alternative_endpoints.dedup();
        }

        let discovery = PendingDiscovery {
            id: id.clone(),
            source: result.source,
            server_info,
            domain_match: keywords.iter().map(|s| s.to_string()).collect(),
            risk_assessment: RiskAssessment {
                level: risk,
                reasons: vec![
                    "external_registry".to_string(),
                    format!("relevance_score:{:.2}", score),
                ],
            },
            requested_at: Utc::now(),
            expires_at: Utc::now() + chrono::Duration::hours(24),
            requesting_goal: Some(goal.to_string()),
        };

        // add() returns the ID (existing if duplicate, new if not)
        let actual_id = self.approval_queue.add(discovery)?;
        Ok(actual_id)
    }
}
