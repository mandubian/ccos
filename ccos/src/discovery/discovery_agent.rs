//! Discovery Agent for Intelligent Capability Discovery
//!
//! This agent provides unified hint generation, auto-learned context vocabularies,
//! and embedding-backed ranking to improve capability discovery without hard-coded rules.

use crate::capability_marketplace::types::CapabilityManifest;
use crate::capability_marketplace::CapabilityMarketplace;
use crate::discovery::config::DiscoveryConfig;
use crate::discovery::embedding_service::EmbeddingService;
use crate::discovery::need_extractor::CapabilityNeed;
use crate::types::Intent;
use rtfs::runtime::error::RuntimeResult;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

/// Result from discovery agent suggesting capabilities
#[derive(Debug, Clone)]
pub struct DiscoverySuggestion {
    /// Human-readable hints for logging/debugging
    pub hints: Vec<String>,
    /// Registry search queries (ordered by priority)
    pub registry_queries: Vec<String>,
    /// Ranked capability candidates with scores
    pub ranked_capabilities: Vec<RankedCapability>,
}

/// A capability candidate with its relevance score
#[derive(Debug, Clone)]
pub struct RankedCapability {
    pub manifest: CapabilityManifest,
    pub score: f64,
    pub source: CapabilitySource,
    pub match_reason: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CapabilitySource {
    Local,
    MCP,
}

/// Discovery agent that learns from marketplace and uses embeddings
pub struct DiscoveryAgent {
    marketplace: Arc<CapabilityMarketplace>,
    config: DiscoveryConfig,
    /// Auto-learned context vocabulary (platforms, services, domains)
    /// Maps namespace/prefix -> frequency-weighted tokens
    context_vocab: Arc<tokio::sync::RwLock<HashMap<String, Vec<ContextToken>>>>,
    /// Optional embedding service for semantic matching (wrapped in Mutex for mutability)
    embedding_service: Option<Arc<Mutex<EmbeddingService>>>,
}

#[derive(Debug, Clone)]
struct ContextToken {
    token: String,
    frequency: usize,
    #[allow(dead_code)]
    sources: HashSet<String>, // capability IDs that contributed this token
}

impl DiscoveryAgent {
    /// Create a new discovery agent
    pub fn new(marketplace: Arc<CapabilityMarketplace>, config: DiscoveryConfig) -> Self {
        let embedding_service = EmbeddingService::from_settings(Some(&config))
            .map(|service| Arc::new(Mutex::new(service)));
        Self {
            marketplace,
            config,
            context_vocab: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            embedding_service,
        }
    }

    /// Learn context vocabulary from current marketplace capabilities
    /// This should be called periodically or when new capabilities are registered
    pub async fn learn_vocabulary(&self) -> RuntimeResult<()> {
        let capabilities = self.marketplace.list_capabilities().await;
        let mut vocab: HashMap<String, HashMap<String, usize>> = HashMap::new();

        for manifest in &capabilities {
            // Extract namespace (first part of capability ID)
            let namespace = manifest
                .id
                .split('.')
                .next()
                .unwrap_or("general")
                .to_string();

            // Extract tokens from ID, name, and description
            let text = format!("{} {} {}", manifest.id, manifest.name, manifest.description)
                .to_ascii_lowercase();

            // Tokenize: split on dots, dashes, underscores, and whitespace
            let tokens: Vec<String> = text
                .split(|c: char| c == '.' || c == '_' || c == '-' || c.is_whitespace())
                .filter(|t| t.len() > 2) // Filter out very short tokens
                .map(|t| t.to_string())
                .collect();

            // Count token frequencies per namespace
            let namespace_vocab = vocab.entry(namespace.clone()).or_insert_with(HashMap::new);
            for token in tokens {
                *namespace_vocab.entry(token).or_insert(0) += 1;
            }
        }

        // Convert to ContextToken structures
        let mut context_vocab = HashMap::new();
        for (namespace, token_counts) in vocab {
            let mut context_tokens: Vec<ContextToken> = token_counts
                .into_iter()
                .map(|(token, frequency)| ContextToken {
                    token,
                    frequency,
                    sources: HashSet::new(), // Could track sources if needed
                })
                .collect();

            // Sort by frequency (descending) and take top tokens
            context_tokens.sort_by(|a, b| b.frequency.cmp(&a.frequency));
            context_tokens.truncate(20); // Keep top 20 per namespace

            context_vocab.insert(namespace, context_tokens);
        }

        *self.context_vocab.write().await = context_vocab;
        Ok(())
    }

    /// Generate unified hints and registry queries from goal and intent
    pub async fn suggest(
        &self,
        goal: &str,
        intent: Option<&Intent>,
        need: &CapabilityNeed,
    ) -> RuntimeResult<DiscoverySuggestion> {
        // 1. Extract context tokens from learned vocabulary + goal text
        let context_tokens = self.extract_context_tokens(goal, intent, need).await?;

        // 2. Generate unified hints (both for logging and for registry queries)
        let (hints, registry_queries) =
            self.generate_unified_hints(goal, intent, &context_tokens, &need.capability_class)?;

        // 3. Search local marketplace with embedding-backed ranking
        let ranked_capabilities = self
            .rank_capabilities_with_embeddings(goal, need, &hints)
            .await?;

        Ok(DiscoverySuggestion {
            hints,
            registry_queries,
            ranked_capabilities,
        })
    }

    /// Extract context tokens using learned vocabulary + goal analysis
    async fn extract_context_tokens(
        &self,
        goal: &str,
        intent: Option<&Intent>,
        need: &CapabilityNeed,
    ) -> RuntimeResult<Vec<String>> {
        let vocab = self.context_vocab.read().await;
        let goal_lower = goal.to_ascii_lowercase();
        let rationale_lower = need.rationale.to_ascii_lowercase();
        let combined_text = format!("{} {}", goal_lower, rationale_lower);

        let mut context_tokens = HashSet::new();

        // 1. Check learned vocabulary for matches
        for (_namespace, tokens) in vocab.iter() {
            for ctx_token in tokens {
                // Check if token appears in goal/rationale
                if combined_text.contains(&ctx_token.token) {
                    context_tokens.insert(ctx_token.token.clone());
                }
            }
        }

        // 2. Extract from capability class (e.g., "github.list" -> ["github", "list"])
        let class_tokens: Vec<String> = need
            .capability_class
            .split('.')
            .filter(|t| t.len() > 2)
            .map(|t| t.to_ascii_lowercase())
            .collect();
        for token in class_tokens {
            context_tokens.insert(token);
        }

        // 3. Extract from intent constraints/preferences if available
        if let Some(intent) = intent {
            for (key, value) in &intent.constraints {
                let key_lower = key.to_ascii_lowercase();
                if key_lower.len() > 2 {
                    context_tokens.insert(key_lower);
                }
                // Extract from value if it's a string
                if let Ok(value_str) = serde_json::to_string(value) {
                    let value_lower = value_str.to_ascii_lowercase();
                    for word in value_lower.split_whitespace() {
                        if word.len() > 2 {
                            context_tokens.insert(word.to_string());
                        }
                    }
                }
            }
        }

        // Filter out common stopwords
        let stopwords: HashSet<&str> = [
            "the", "a", "an", "and", "or", "but", "in", "on", "at", "to", "for", "of", "with",
            "by", "from", "as", "is", "are", "was", "were", "be", "been", "being", "have", "has",
            "had", "do", "does", "did", "will", "would", "should", "could", "may", "might", "must",
            "can",
        ]
        .iter()
        .cloned()
        .collect();

        let filtered: Vec<String> = context_tokens
            .into_iter()
            .filter(|t| !stopwords.contains(t.as_str()))
            .collect();

        Ok(filtered)
    }

    /// Generate unified hints and registry queries
    fn generate_unified_hints(
        &self,
        _goal: &str,
        _intent: Option<&Intent>,
        context_tokens: &[String],
        capability_class: &str,
    ) -> RuntimeResult<(Vec<String>, Vec<String>)> {
        let mut hints = Vec::new();
        let mut registry_queries = Vec::new();
        let mut seen = HashSet::new();

        // Generic operation verbs (these are common patterns)
        let operations = [
            "list", "search", "get", "fetch", "create", "update", "delete", "filter",
        ];

        // High-value context tokens (platforms/services) should be prioritized
        let high_value_contexts: HashSet<&str> = [
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
        ]
        .iter()
        .cloned()
        .collect();

        // Score and sort context tokens by priority
        let mut scored_tokens: Vec<(String, usize)> = context_tokens
            .iter()
            .map(|token| {
                let priority = if high_value_contexts.contains(token.as_str()) {
                    3 // Highest priority for known platforms
                } else if token.len() > 5 {
                    2 // Medium priority for longer tokens
                } else {
                    1 // Lower priority for short/generic tokens
                };
                (token.clone(), priority)
            })
            .collect();

        // Sort by priority (descending), then by length (descending)
        scored_tokens.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.0.len().cmp(&a.0.len())));

        // Take top 3 context tokens for combinations (reduces noise)
        let top_contexts: Vec<&String> = scored_tokens.iter().take(3).map(|(t, _)| t).collect();

        // 1. Generate context-aware combinations (highest priority)
        // e.g., "github.list", "github.search", "list.github"
        for context in top_contexts {
            // Skip if it's already an operation
            if operations.contains(&context.as_str()) {
                continue;
            }

            // Limit to 2 most relevant operations per context
            for op in operations.iter().take(2) {
                // Format: context.operation (preferred)
                let combo = format!("{}.{}", context, op);
                if seen.insert(combo.clone()) {
                    hints.push(combo.clone());
                    registry_queries.push(format!("{} {}", context, op));
                }

                // Format: operation.context (less preferred, but still useful)
                let combo_rev = format!("{}.{}", op, context);
                if seen.insert(combo_rev.clone()) {
                    hints.push(combo_rev);
                }
            }
        }

        // 2. Add capability class tokens as hints/queries (high priority if it contains context)
        let class_parts: Vec<&str> = capability_class.split('.').collect();
        if class_parts.len() >= 2 {
            let query = class_parts.join(" ");
            // Check if capability class contains a high-value context
            let has_high_value = class_parts
                .iter()
                .any(|part| high_value_contexts.contains(part));
            if has_high_value {
                // Insert at beginning for high-value contexts
                hints.insert(0, capability_class.to_string());
                registry_queries.insert(0, query);
            } else if !seen.contains(&query) {
                hints.push(capability_class.to_string());
                registry_queries.push(query);
            }
        }

        // 3. Add single high-value context tokens only (skip generic ones)
        for (context, priority) in scored_tokens.iter().take(2) {
            if *priority >= 2
                && !operations.contains(&context.as_str())
                && seen.insert(context.clone())
            {
                hints.push(context.clone());
                registry_queries.push(context.clone());
            }
        }

        // Ensure we have at least some queries
        if registry_queries.is_empty() {
            registry_queries.push(capability_class.to_string());
        }

        Ok((hints, registry_queries))
    }

    /// Extract semantic terms (nouns/objects) from goal text that indicate what the user wants
    /// Returns terms that are likely to be in capability names (e.g., "issues", "branches", "commits")
    pub fn extract_semantic_terms(text: &str) -> Vec<String> {
        let text_lower = text.to_ascii_lowercase();
        let words: Vec<&str> = text_lower.split_whitespace().collect();
        let mut terms = Vec::new();

        // Common operation verbs to skip
        let operations: HashSet<&str> = [
            "list", "search", "get", "fetch", "create", "update", "delete", "filter", "find",
            "show", "display", "print", "return", "send", "receive",
        ]
        .iter()
        .cloned()
        .collect();

        // Common stopwords
        let stopwords: HashSet<&str> = [
            "the", "a", "an", "and", "or", "but", "in", "on", "at", "to", "for", "of", "with",
            "by", "from", "as", "is", "are", "was", "were", "be", "been", "being", "have", "has",
            "had", "do", "does", "did", "will", "would", "should", "could", "may", "might", "must",
            "can", "if", "them", "they", "their", "this", "that", "these", "those", "about",
            "speak",
        ]
        .iter()
        .cloned()
        .collect();

        for word in words {
            let cleaned = word.trim_matches(|c: char| !c.is_alphanumeric());
            // Skip if too short, is an operation, or is a stopword
            if cleaned.len() >= 4
                && !operations.contains(cleaned)
                && !stopwords.contains(cleaned)
                && cleaned.chars().all(|c| c.is_alphabetic())
            {
                terms.push(cleaned.to_string());
            }
        }

        // Remove duplicates while preserving order
        let mut seen = HashSet::new();
        terms
            .into_iter()
            .filter(|t| seen.insert(t.clone()))
            .collect()
    }

    /// Rank capabilities using embedding-based similarity
    async fn rank_capabilities_with_embeddings(
        &self,
        goal: &str,
        need: &CapabilityNeed,
        hints: &[String],
    ) -> RuntimeResult<Vec<RankedCapability>> {
        let all_capabilities = self.marketplace.list_capabilities().await;
        let mut ranked = Vec::new();

        // Build query text for embedding
        let query_text = format!("{} {}", goal, need.rationale);

        // Extract semantic terms from goal (e.g., "issues", "branches", "commits")
        let semantic_terms = Self::extract_semantic_terms(&query_text);
        if !semantic_terms.is_empty() {
            eprintln!(
                "  → Extracted semantic terms from goal: {:?}",
                semantic_terms
            );
        }

        // Get query embedding if available
        let query_embedding = if let Some(ref emb_service) = self.embedding_service {
            let mut service = emb_service.lock().unwrap();
            service.embed(&query_text).await.ok()
        } else {
            None
        };

        for manifest in all_capabilities {
            // Skip planner/ccos internal capabilities
            if manifest.id.starts_with("planner.") || manifest.id.starts_with("ccos.") {
                continue;
            }

            // Build capability text for matching
            let capability_text =
                format!("{} {} {}", manifest.id, manifest.name, manifest.description);
            let capability_lower = capability_text.to_ascii_lowercase();

            let mut score = 0.0;
            let mut match_reason = String::new();

            // 1. Semantic term matching (highest priority - boosts capabilities that match goal-specific terms)
            if !semantic_terms.is_empty() {
                let semantic_matches: usize = semantic_terms
                    .iter()
                    .filter(|term| capability_lower.contains(term.as_str()))
                    .count();

                if semantic_matches > 0 {
                    // Strong boost for semantic term matches (0.3-0.6 depending on match count)
                    let semantic_boost = (semantic_matches as f64 * 0.2).min(0.6);
                    score = semantic_boost;
                    match_reason = format!(
                        "semantic term match: {}/{} terms ({:?})",
                        semantic_matches,
                        semantic_terms.len(),
                        semantic_terms
                            .iter()
                            .filter(|t| capability_lower.contains(t.as_str()))
                            .collect::<Vec<_>>()
                    );
                }
            }

            // 2. Embedding-based matching (if available)
            if let Some(ref query_emb) = query_embedding {
                if let Some(ref emb_service) = self.embedding_service {
                    let mut service = emb_service.lock().unwrap();
                    if let Ok(cap_emb) = service.embed(&capability_text).await {
                        let similarity = EmbeddingService::cosine_similarity(query_emb, &cap_emb);
                        // Use embedding score if higher than semantic score, or add as boost
                        if similarity > self.config.match_threshold {
                            if similarity > score {
                                score = similarity;
                                match_reason = format!("embedding similarity: {:.2}", similarity);
                            } else {
                                // Add embedding as boost to semantic score
                                score += similarity * 0.3;
                                match_reason.push_str(&format!(" + embedding: {:.2}", similarity));
                            }
                        }
                    }
                }
            }

            // 3. Token-based matching (fallback or supplement)
            if score < 0.3 {
                let tokens: Vec<String> = need
                    .capability_class
                    .split('.')
                    .map(|t| t.to_ascii_lowercase())
                    .collect();

                let matching_tokens: usize = tokens
                    .iter()
                    .filter(|t| capability_lower.contains(t.as_str()))
                    .count();

                if matching_tokens > 0 {
                    let token_score = matching_tokens as f64 / tokens.len() as f64;
                    if token_score > score {
                        score = token_score;
                        match_reason =
                            format!("token match: {}/{} tokens", matching_tokens, tokens.len());
                    } else {
                        // Add as small boost
                        score += token_score * 0.1;
                    }
                }
            }

            // 4. Hint-based matching (boost score if hints match)
            let hint_matches: usize = hints
                .iter()
                .filter(|hint| capability_lower.contains(&hint.to_lowercase()))
                .count();

            if hint_matches > 0 {
                let hint_boost = (hint_matches as f64 * 0.05).min(0.2); // Smaller boost since semantic terms are more important
                score += hint_boost;
                if !match_reason.is_empty() {
                    match_reason.push_str(&format!(" + {} hint matches", hint_matches));
                } else {
                    match_reason = format!("hint matches: {}", hint_matches);
                }
            }

            if score > 0.0 {
                // Determine source
                let source = if manifest.id.starts_with("mcp.") {
                    CapabilitySource::MCP
                } else {
                    CapabilitySource::Local
                };

                ranked.push(RankedCapability {
                    manifest,
                    score,
                    source,
                    match_reason,
                });
            }
        }

        // Sort by score (descending)
        ranked.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Take top candidates
        ranked.truncate(20);

        Ok(ranked)
    }
}

#[cfg(test)]
mod tests {
    // Tests would go here when we have mock marketplace support
}
