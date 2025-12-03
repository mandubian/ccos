use crate::discovery::approval_queue::{ApprovalQueue, PendingDiscovery, RiskAssessment, RiskLevel};
use crate::discovery::capability_matcher::calculate_description_match_score_improved;
use crate::discovery::config::DiscoveryConfig;
use crate::discovery::registry_search::{RegistrySearchResult, RegistrySearcher};
use rtfs::runtime::error::RuntimeResult;
use chrono::Utc;
use uuid::Uuid;

/// Scored search result for ranking (internal)
struct ScoredResult {
    result: RegistrySearchResult,
    score: f64,
}

/// Public type alias for scored results returned by search_and_score
pub type ScoredSearchResult = (RegistrySearchResult, f64);

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
    pub async fn search_and_score(&self, goal: &str, llm_enabled: bool) -> RuntimeResult<Vec<ScoredSearchResult>> {
        // TODO: Use LLM for semantic analysis if enabled
        if llm_enabled {
            println!("⚠️ LLM analysis enabled but not yet implemented (falling back to semantic matching)");
        }

        // Search registries
        let results = self.registry_searcher.search(goal).await?;
        
        let total_candidates = results.len();
        let debug = std::env::var("CCOS_DEBUG").is_ok();
        
        // Pre-extract domain keywords from goal (non-action-verbs)
        let action_verbs = [
            "list", "get", "create", "update", "delete", "show", "find", "search",
            "fetch", "retrieve", "read", "post", "add", "remove", "modify", "edit",
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
                let name_matches: usize = goal_domains.iter()
                    .filter(|d| name_lower.contains(d.as_str()))
                    .count();
                
                let desc_matches: usize = goal_domains.iter()
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
                    goal, description, name, "", name, &self.config,
                );
                
                // Boost score for name matches
                let name_domain_matches = goal_domains.iter()
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
        scored_results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        
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
    pub fn queue_result(&self, goal: &str, result: RegistrySearchResult, score: f64) -> RuntimeResult<String> {
        let keywords: Vec<&str> = goal.split_whitespace().collect();
        let id = format!("discovery-{}", Uuid::new_v4());
        
        let risk = if result.server_info.endpoint.starts_with("https://") {
            RiskLevel::Medium
        } else {
            RiskLevel::High
        };
        
        let discovery = PendingDiscovery {
            id: id.clone(),
            source: result.source,
            server_info: result.server_info,
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
        
        self.approval_queue.add(discovery)?;
        Ok(id)
    }
}

