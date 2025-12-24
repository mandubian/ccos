//! Semantic resolution strategy
//!
//! Uses embedding-based similarity to find matching capabilities in the catalog.

use async_trait::async_trait;
use std::sync::Arc;

use super::{ResolutionContext, ResolutionError, ResolutionStrategy, ResolvedCapability};
use crate::planner::modular_planner::decomposition::grounded_llm::{
    cosine_similarity, EmbeddingProvider,
};
use crate::planner::modular_planner::types::{ApiAction, IntentType, SubIntent};

/// Capability info for matching
#[derive(Debug, Clone)]
pub struct CapabilityInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub input_schema: Option<String>,
    /// Domains this capability belongs to
    pub domains: Vec<String>,
    /// Categories for this capability
    pub categories: Vec<String>,
}

/// Catalog trait for querying available capabilities
#[async_trait(?Send)]
pub trait CapabilityCatalog: Send + Sync {
    /// List all capabilities matching domain hint
    async fn list_capabilities(&self, domain: Option<&str>) -> Vec<CapabilityInfo>;

    /// Get capability by ID
    async fn get_capability(&self, id: &str) -> Option<CapabilityInfo>;

    /// Search capabilities by query string
    async fn search(&self, query: &str, limit: usize) -> Vec<CapabilityInfo>;

    /// Search capabilities by domain and optional category
    async fn search_by_domain(
        &self,
        domains: &[String],
        categories: Option<&[String]>,
        limit: usize,
    ) -> Vec<CapabilityInfo> {
        // Default implementation falls back to listing and filtering
        let all = self
            .list_capabilities(domains.first().map(|s| s.as_str()))
            .await;
        all.into_iter()
            .filter(|c| {
                // Check domain match
                let domain_match = domains.is_empty()
                    || c.domains.iter().any(|d| {
                        domains.iter().any(|fd| {
                            d == fd
                                || d.starts_with(&format!("{}.", fd))
                                || fd.starts_with(&format!("{}.", d))
                        })
                    });

                // Check category match if specified
                let category_match = categories
                    .map(|cats| cats.is_empty() || c.categories.iter().any(|c| cats.contains(c)))
                    .unwrap_or(true);

                domain_match && category_match
            })
            .take(limit)
            .collect()
    }
}

/// Semantic resolution using embeddings and catalog search.
pub struct SemanticResolution {
    catalog: Arc<dyn CapabilityCatalog>,
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    /// Minimum similarity score to accept match
    min_score: f64,
}

impl SemanticResolution {
    pub fn new(catalog: Arc<dyn CapabilityCatalog>) -> Self {
        Self {
            catalog,
            embedding_provider: None,
            min_score: 0.3,
        }
    }

    pub fn with_embedding(mut self, provider: Arc<dyn EmbeddingProvider>) -> Self {
        self.embedding_provider = Some(provider);
        self
    }

    pub fn with_min_score(mut self, score: f64) -> Self {
        self.min_score = score;
        self
    }

    /// Build search query from intent
    fn build_query(&self, intent: &SubIntent) -> String {
        let mut parts = vec![intent.description.clone()];

        // Add action keywords
        if let IntentType::ApiCall { ref action } = intent.intent_type {
            for kw in action.matching_keywords() {
                parts.push(kw.to_string());
            }
        }

        // Add domain hint
        if let Some(ref domain) = intent.domain_hint {
            for server in domain.likely_mcp_servers() {
                parts.push(server.to_string());
            }
        }

        // Add extracted params context
        for (key, value) in &intent.extracted_params {
            if !key.starts_with('_') {
                parts.push(format!("{}: {}", key, value));
            }
        }

        parts.join(" ")
    }

    /// Score a capability against an intent
    async fn score_capability(&self, intent: &SubIntent, capability: &CapabilityInfo) -> f64 {
        let query = self.build_query(intent);

        // If we have embeddings, use semantic similarity
        if let Some(ref emb) = self.embedding_provider {
            let query_emb = match emb.embed(&query).await {
                Ok(e) => e,
                Err(_) => return self.keyword_score(intent, capability),
            };

            let cap_text = format!("{} {}", capability.name, capability.description);
            let cap_emb = match emb.embed(&cap_text).await {
                Ok(e) => e,
                Err(_) => return self.keyword_score(intent, capability),
            };

            return cosine_similarity(&query_emb, &cap_emb);
        }

        // Fallback to keyword matching
        self.keyword_score(intent, capability)
    }

    /// Simple keyword-based scoring fallback
    fn keyword_score(&self, intent: &SubIntent, capability: &CapabilityInfo) -> f64 {
        let cap_name_lower = capability.name.to_lowercase();
        let cap_desc_lower = capability.description.to_lowercase();
        let desc_lower = intent.description.to_lowercase();

        let mut score = 0.0;
        let words: Vec<&str> = desc_lower.split_whitespace().collect();

        // Extract action keywords if this is an API call intent
        let action_keywords: Vec<&'static str> = match &intent.intent_type {
            IntentType::ApiCall { action } => action.matching_keywords().to_vec(),
            _ => Vec::new(),
        };

        // Identify significant nouns (words that are likely object targets, not prepositions/articles)
        let stop_words = [
            "the", "a", "an", "in", "on", "to", "for", "my", "your", "new", "from", "with",
        ];
        let significant_words: Vec<&str> = words
            .iter()
            .filter(|w| w.len() > 2 && !stop_words.contains(&w.to_lowercase().as_str()))
            .cloned()
            .collect();

        // Score based on word matches, with strong preference for name matches
        for word in &significant_words {
            // Strong boost for matching in capability NAME (the most important signal)
            if cap_name_lower.contains(*word) {
                score += 0.5; // High value for name match
            } else if cap_desc_lower.contains(*word) {
                score += 0.1; // Lower value for description match
            }
        }

        // Boost for action keyword match in capability name (e.g., "create" in "issue_write")
        // Note: issue_write doesn't have "create" in name, but description says "Create"
        for kw in &action_keywords {
            if cap_name_lower.contains(kw) {
                score += 0.3;
                break;
            }
            // Also check description for action keywords
            if cap_desc_lower.contains(kw) {
                score += 0.15;
            }
        }

        // Boost for domain match
        if let Some(ref domain) = intent.domain_hint {
            for server in domain.likely_mcp_servers() {
                if capability.id.to_lowercase().contains(server.as_str()) {
                    score += 0.15;
                    break;
                }
            }
        }

        // Normalize by number of significant words to avoid penalizing longer queries
        if !significant_words.is_empty() {
            score = score / (1.0 + (significant_words.len() as f64 * 0.1));
        }

        score.min(1.0)
    }

    /// Extract arguments from intent params to match capability
    fn extract_arguments(&self, intent: &SubIntent) -> std::collections::HashMap<String, String> {
        intent
            .extracted_params
            .iter()
            .filter(|(k, _)| !k.starts_with('_'))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}

#[async_trait(?Send)]
impl ResolutionStrategy for SemanticResolution {
    fn name(&self) -> &str {
        "semantic"
    }

    fn can_handle(&self, intent: &SubIntent) -> bool {
        matches!(
            intent.intent_type,
            IntentType::ApiCall { .. } | IntentType::DataTransform { .. }
        )
    }

    async fn resolve(
        &self,
        intent: &SubIntent,
        _context: &ResolutionContext,
    ) -> Result<ResolvedCapability, ResolutionError> {
        // Check if LLM explicitly said "no tool" (grounded decomposition returned null)
        if intent.extracted_params.contains_key("_grounded_no_tool") {
            return Err(ResolutionError::GroundedNoTool(
                "Grounded planner explicitly returned no tool".to_string(),
            ));
        }

        let query = self.build_query(intent);

        // Search catalog
        let candidates = self.catalog.search(&query, 10).await;

        if candidates.is_empty() {
            return Err(ResolutionError::NotFound(format!(
                "No capabilities found for: {}",
                intent.description
            )));
        }

        // Score all candidates
        let mut scored: Vec<(CapabilityInfo, f64)> = Vec::new();
        for cap in candidates {
            let score = self.score_capability(intent, &cap).await;
            if score >= self.min_score {
                scored.push((cap, score));
            }
        }

        // Sort by score descending
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        if scored.is_empty() {
            return Err(ResolutionError::NotFound(format!(
                "No capabilities above threshold for: {}",
                intent.description
            )));
        }

        let (best, best_score) = scored.remove(0);
        let arguments = self.extract_arguments(intent);

        Ok(ResolvedCapability::Local {
            capability_id: best.id,
            arguments,
            confidence: best_score,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    struct MockCatalog {
        capabilities: Vec<CapabilityInfo>,
    }

    #[async_trait(?Send)]
    impl CapabilityCatalog for MockCatalog {
        async fn list_capabilities(&self, _domain: Option<&str>) -> Vec<CapabilityInfo> {
            self.capabilities.clone()
        }

        async fn get_capability(&self, id: &str) -> Option<CapabilityInfo> {
            self.capabilities.iter().find(|c| c.id == id).cloned()
        }

        async fn search(&self, query: &str, limit: usize) -> Vec<CapabilityInfo> {
            let query_lower = query.to_lowercase();
            // Split query into words for more flexible matching
            let query_words: Vec<&str> = query_lower.split_whitespace().collect();

            self.capabilities
                .iter()
                .filter(|c| {
                    let cap_text = format!("{} {}", c.name, c.description).to_lowercase();
                    // Match if any query word appears in capability text
                    query_words
                        .iter()
                        .any(|word| word.len() > 3 && cap_text.contains(*word))
                })
                .take(limit)
                .cloned()
                .collect()
        }
    }

    #[tokio::test]
    async fn test_semantic_resolution() {
        let catalog = Arc::new(MockCatalog {
            capabilities: vec![
                CapabilityInfo {
                    id: "mcp.github.list_issues".to_string(),
                    name: "list_issues".to_string(),
                    description: "List issues in a GitHub repository".to_string(),
                    input_schema: None,
                    domains: vec!["github".to_string()],
                    categories: vec!["list".to_string(), "crud".to_string()],
                },
                CapabilityInfo {
                    id: "mcp.github.create_issue".to_string(),
                    name: "create_issue".to_string(),
                    description: "Create a new issue".to_string(),
                    input_schema: None,
                    domains: vec!["github".to_string()],
                    categories: vec!["create".to_string(), "crud".to_string()],
                },
            ],
        });

        let strategy = SemanticResolution::new(catalog);
        let context = ResolutionContext::new();

        let intent = SubIntent::new(
            "List issues from repository",
            IntentType::ApiCall {
                action: ApiAction::List,
            },
        )
        .with_param("owner", "mandubian")
        .with_param("repo", "ccos");

        let result = strategy
            .resolve(&intent, &context)
            .await
            .expect("Should resolve");

        match result {
            ResolvedCapability::Local {
                capability_id,
                arguments,
                ..
            } => {
                assert_eq!(capability_id, "mcp.github.list_issues");
                assert_eq!(arguments.get("owner"), Some(&"mandubian".to_string()));
            }
            _ => panic!("Expected Local capability"),
        }
    }
}
