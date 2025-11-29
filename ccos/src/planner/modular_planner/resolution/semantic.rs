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
        let cap_lower = format!("{} {}", capability.name, capability.description).to_lowercase();
        let desc_lower = intent.description.to_lowercase();

        let mut score = 0.0;
        let mut matches = 0;
        let words: Vec<&str> = desc_lower.split_whitespace().collect();

        for word in &words {
            if word.len() > 2 && cap_lower.contains(word) {
                matches += 1;
            }
        }

        if !words.is_empty() {
            score = matches as f64 / words.len() as f64;
        }

        // Boost for action match
        if let IntentType::ApiCall { ref action } = intent.intent_type {
            for kw in action.matching_keywords() {
                if capability.name.to_lowercase().contains(kw) {
                    score += 0.2;
                    break;
                }
            }
        }

        // Boost for domain match
        if let Some(ref domain) = intent.domain_hint {
            for server in domain.likely_mcp_servers() {
                if capability.id.to_lowercase().contains(server) {
                    score += 0.15;
                    break;
                }
            }
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
                },
                CapabilityInfo {
                    id: "mcp.github.create_issue".to_string(),
                    name: "create_issue".to_string(),
                    description: "Create a new issue".to_string(),
                    input_schema: None,
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
