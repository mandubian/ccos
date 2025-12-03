//! LLM-Enhanced Discovery Service
//!
//! This module provides intelligent discovery capabilities using LLM for:
//! - Intent analysis: Understanding user goals beyond keyword matching
//! - Query expansion: Generating multiple search queries for better coverage
//! - Semantic ranking: Scoring discovery results based on relevance to intent
//!
//! The goal here is to enhance server/capability discovery, not complex multi-step
//! planning. A "goal" in this context is a discovery intent like "track project progress"
//! or "send SMS notifications" that maps to servers and capabilities.

use crate::arbiter::{get_default_llm_provider, LlmProvider};
use crate::discovery::registry_search::RegistrySearchResult;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use serde::{Deserialize, Serialize};

/// Result of analyzing a user's discovery goal with an LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentAnalysis {
    /// The primary action implied by the goal (e.g., "list", "search", "send", "track")
    pub primary_action: String,
    /// The main target or object (e.g., "issues", "SMS", "weather")
    pub target_object: String,
    /// Domain keywords extracted from the goal
    pub domain_keywords: Vec<String>,
    /// Synonyms and related terms for better matching
    pub synonyms: Vec<String>,
    /// Implied concepts not explicitly stated (e.g., "track progress" implies "issues", "tasks")
    pub implied_concepts: Vec<String>,
    /// Expanded search queries to use for registry search
    pub expanded_queries: Vec<String>,
    /// Confidence score (0.0-1.0) in the analysis
    pub confidence: f64,
}

/// Result of LLM-based ranking of a discovery candidate
#[derive(Debug, Clone)]
pub struct RankedResult {
    /// Original search result
    pub result: RegistrySearchResult,
    /// LLM-assigned relevance score (0.0-1.0)
    pub llm_score: f64,
    /// Reasoning for the score
    pub reasoning: String,
    /// Whether the LLM recommends this result
    pub recommended: bool,
}

/// LLM-Enhanced Discovery Service
pub struct LlmDiscoveryService {
    provider: Box<dyn LlmProvider + Send + Sync>,
}

impl LlmDiscoveryService {
    /// Create a new LlmDiscoveryService with the default LLM provider
    pub async fn new() -> RuntimeResult<Self> {
        let provider = get_default_llm_provider()
            .await
            .ok_or_else(|| RuntimeError::Generic(
                "No LLM provider configured. Set OPENAI_API_KEY, ANTHROPIC_API_KEY, or OPENROUTER_API_KEY.".to_string()
            ))?;
        
        Ok(Self { provider })
    }

    /// Create a new LlmDiscoveryService with a specific provider
    pub fn with_provider(provider: Box<dyn LlmProvider + Send + Sync>) -> Self {
        Self { provider }
    }

    /// Analyze a user's discovery goal to extract intent and generate expanded queries
    pub async fn analyze_goal(&self, goal: &str) -> RuntimeResult<IntentAnalysis> {
        let prompt = self.build_intent_analysis_prompt(goal);
        
        let response = self.provider.generate_text(&prompt).await?;
        
        self.parse_intent_analysis(&response, goal)
    }

    /// Rank discovery results using LLM semantic understanding
    /// 
    /// # Arguments
    /// * `goal` - The original user goal
    /// * `intent` - The analyzed intent (optional, for richer context)
    /// * `results` - Discovery results to rank (should be pre-filtered to top N for cost control)
    /// 
    /// # Returns
    /// Ranked results with LLM scores and reasoning
    pub async fn rank_results(
        &self,
        goal: &str,
        intent: Option<&IntentAnalysis>,
        results: Vec<RegistrySearchResult>,
    ) -> RuntimeResult<Vec<RankedResult>> {
        if results.is_empty() {
            return Ok(Vec::new());
        }

        // Limit candidates to avoid excessive LLM cost
        let max_candidates = 10;
        let candidates: Vec<_> = results.into_iter().take(max_candidates).collect();

        let prompt = self.build_ranking_prompt(goal, intent, &candidates);
        
        let response = self.provider.generate_text(&prompt).await?;
        
        self.parse_ranking_response(&response, candidates)
    }

    /// Build the prompt for intent analysis
    fn build_intent_analysis_prompt(&self, goal: &str) -> String {
        format!(r#"You are an expert at analyzing user goals for API and tool discovery.

Given a user's goal, extract the intent and generate search queries for finding relevant APIs and MCP servers.

User Goal: "{goal}"

Analyze this goal and respond with ONLY a JSON object in this exact format:
{{
  "primary_action": "the main verb/action (e.g., list, get, search, send, track, create)",
  "target_object": "the main target (e.g., issues, pull requests, SMS, weather, users)",
  "domain_keywords": ["key", "domain", "words"],
  "synonyms": ["alternative", "terms", "for", "target"],
  "implied_concepts": ["concepts", "not", "stated", "but", "implied"],
  "expanded_queries": ["query 1 for registry search", "query 2 with synonyms", "query 3 with domain"],
  "confidence": 0.85
}}

Guidelines:
- primary_action: Extract the core action verb
- target_object: The main thing being acted upon
- domain_keywords: Keywords that identify the domain (github, weather, messaging, etc.)
- synonyms: Alternative names for the target (PRs for pull requests, SMS for text messages)
- implied_concepts: What's implied but not said (e.g., "track progress" implies issues, tasks, kanban)
- expanded_queries: 2-4 diverse search queries that could find relevant servers/APIs
- confidence: How confident you are in your analysis (0.0-1.0)

Respond with ONLY the JSON object, no markdown formatting or explanation."#)
    }

    /// Build the prompt for ranking discovery results
    fn build_ranking_prompt(
        &self,
        goal: &str,
        intent: Option<&IntentAnalysis>,
        candidates: &[RegistrySearchResult],
    ) -> String {
        let intent_context = if let Some(i) = intent {
            format!(
                r#"
Intent Analysis:
- Action: {}
- Target: {}
- Keywords: {}
- Implied: {}"#,
                i.primary_action,
                i.target_object,
                i.domain_keywords.join(", "),
                i.implied_concepts.join(", ")
            )
        } else {
            String::new()
        };

        let candidates_json: Vec<serde_json::Value> = candidates
            .iter()
            .enumerate()
            .map(|(i, r)| {
                serde_json::json!({
                    "index": i,
                    "name": r.server_info.name,
                    "description": r.server_info.description.as_deref().unwrap_or("No description"),
                    "endpoint": r.server_info.endpoint,
                })
            })
            .collect();

        format!(
            r#"You are an expert at evaluating API and MCP server relevance for user goals.

User Goal: "{goal}"
{intent_context}

Candidate Servers:
{candidates}

For each candidate, determine how well it matches the user's goal.
Respond with ONLY a JSON array of objects:
[
  {{
    "index": 0,
    "score": 0.95,
    "reasoning": "Brief explanation of why this score",
    "recommended": true
  }},
  ...
]

Scoring Guidelines:
- 0.9-1.0: Excellent match, directly addresses the goal
- 0.7-0.89: Good match, clearly relevant
- 0.5-0.69: Partial match, might be useful
- 0.3-0.49: Weak match, tangentially related
- 0.0-0.29: Poor match, not relevant
- recommended: true if score >= 0.6

Consider:
- Does the server's purpose align with the goal?
- Does the description mention relevant concepts?
- Is this the right domain (e.g., GitHub for code, Twilio for SMS)?

Respond with ONLY the JSON array, no markdown formatting."#,
            goal = goal,
            intent_context = intent_context,
            candidates = serde_json::to_string_pretty(&candidates_json).unwrap_or_default()
        )
    }

    /// Parse the LLM response for intent analysis
    fn parse_intent_analysis(&self, response: &str, goal: &str) -> RuntimeResult<IntentAnalysis> {
        // Extract JSON from response (handle markdown code blocks)
        let json_str = extract_json(response);
        
        match serde_json::from_str::<IntentAnalysis>(json_str) {
            Ok(analysis) => Ok(analysis),
            Err(e) => {
                log::warn!("Failed to parse LLM intent analysis: {}. Response: {}", e, response);
                // Return a fallback analysis based on simple keyword extraction
                Ok(self.fallback_intent_analysis(goal))
            }
        }
    }

    /// Parse the LLM response for ranking
    fn parse_ranking_response(
        &self,
        response: &str,
        candidates: Vec<RegistrySearchResult>,
    ) -> RuntimeResult<Vec<RankedResult>> {
        let json_str = extract_json(response);
        
        #[derive(Deserialize)]
        struct RankingEntry {
            index: usize,
            score: f64,
            reasoning: String,
            #[serde(default)]
            recommended: bool,
        }

        let rankings: Vec<RankingEntry> = match serde_json::from_str(json_str) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("Failed to parse LLM ranking response: {}. Response: {}", e, response);
                // Fallback: return candidates with default scores
                return Ok(candidates
                    .into_iter()
                    .map(|result| RankedResult {
                        result,
                        llm_score: 0.5,
                        reasoning: "LLM parsing failed, using default score".to_string(),
                        recommended: false,
                    })
                    .collect());
            }
        };

        // Map rankings back to candidates
        let mut ranked: Vec<RankedResult> = Vec::with_capacity(candidates.len());
        
        for (i, result) in candidates.into_iter().enumerate() {
            let ranking = rankings.iter().find(|r| r.index == i);
            let (score, reasoning, recommended) = if let Some(r) = ranking {
                (r.score, r.reasoning.clone(), r.recommended || r.score >= 0.6)
            } else {
                (0.5, "No LLM ranking provided".to_string(), false)
            };
            
            ranked.push(RankedResult {
                result,
                llm_score: score,
                reasoning,
                recommended,
            });
        }

        // Sort by LLM score descending
        ranked.sort_by(|a, b| b.llm_score.partial_cmp(&a.llm_score).unwrap_or(std::cmp::Ordering::Equal));

        Ok(ranked)
    }

    /// Fallback intent analysis when LLM parsing fails
    fn fallback_intent_analysis(&self, goal: &str) -> IntentAnalysis {
        let goal_lower = goal.to_lowercase();
        let words: Vec<&str> = goal_lower.split_whitespace().collect();
        
        // Simple action extraction
        let action_verbs = ["list", "get", "search", "find", "create", "send", "track", "check", "fetch"];
        let primary_action = words
            .iter()
            .find(|w| action_verbs.contains(w))
            .map(|s| s.to_string())
            .unwrap_or_else(|| "search".to_string());

        // Extract non-verb words as keywords
        let stopwords = ["the", "a", "an", "for", "in", "on", "to", "and", "or", "of", "with"];
        let domain_keywords: Vec<String> = words
            .iter()
            .filter(|w| !action_verbs.contains(w) && !stopwords.contains(w) && w.len() > 2)
            .map(|s| s.to_string())
            .collect();

        let target_object = domain_keywords.first().cloned().unwrap_or_else(|| "items".to_string());

        IntentAnalysis {
            primary_action,
            target_object: target_object.clone(),
            domain_keywords: domain_keywords.clone(),
            synonyms: Vec::new(),
            implied_concepts: Vec::new(),
            expanded_queries: vec![
                goal.to_string(),
                domain_keywords.join(" "),
            ],
            confidence: 0.3,
        }
    }
}

/// Extract JSON from a response that might contain markdown code blocks
fn extract_json(response: &str) -> &str {
    let trimmed = response.trim();
    
    // Handle ```json ... ``` or ``` ... ```
    if trimmed.starts_with("```") {
        let start = trimmed.find('\n').map(|i| i + 1).unwrap_or(0);
        let end = trimmed.rfind("```").unwrap_or(trimmed.len());
        if start < end {
            return trimmed[start..end].trim();
        }
    }
    
    // Find positions of first '{' and '['
    let obj_start = trimmed.find('{');
    let arr_start = trimmed.find('[');
    
    // Determine which comes first (prefer whichever appears earlier in the string)
    let (start, open_char, close_char) = match (obj_start, arr_start) {
        (Some(o), Some(a)) => {
            if o < a {
                (o, '{', '}')
            } else {
                (a, '[', ']')
            }
        }
        (Some(o), None) => (o, '{', '}'),
        (None, Some(a)) => (a, '[', ']'),
        (None, None) => return trimmed,
    };
    
    // Find matching closing character
    let mut depth = 0;
    for (i, c) in trimmed[start..].char_indices() {
        if c == open_char {
            depth += 1;
        } else if c == close_char {
            depth -= 1;
            if depth == 0 {
                return &trimmed[start..=start + i];
            }
        }
    }
    
    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_simple() {
        let response = r#"{"key": "value"}"#;
        assert_eq!(extract_json(response), r#"{"key": "value"}"#);
    }

    #[test]
    fn test_extract_json_with_markdown() {
        let response = r#"```json
{"key": "value"}
```"#;
        assert_eq!(extract_json(response), r#"{"key": "value"}"#);
    }

    #[test]
    fn test_extract_json_array() {
        let response = r#"[{"index": 0}]"#;
        assert_eq!(extract_json(response), r#"[{"index": 0}]"#);
    }

    #[test]
    fn test_fallback_intent_analysis() {
        let service = LlmDiscoveryService {
            provider: Box::new(StubProvider),
        };
        
        let analysis = service.fallback_intent_analysis("list github issues");
        assert_eq!(analysis.primary_action, "list");
        assert!(analysis.domain_keywords.contains(&"github".to_string()));
        assert!(analysis.domain_keywords.contains(&"issues".to_string()));
    }

    // Stub provider for testing
    struct StubProvider;
    
    #[async_trait::async_trait]
    impl LlmProvider for StubProvider {
        async fn generate_intent(
            &self,
            _prompt: &str,
            _context: Option<std::collections::HashMap<String, String>>,
        ) -> Result<crate::types::StorableIntent, RuntimeError> {
            Err(RuntimeError::Generic("Stub".to_string()))
        }
        
        async fn generate_plan(
            &self,
            _intent: &crate::types::StorableIntent,
            _context: Option<std::collections::HashMap<String, String>>,
        ) -> Result<crate::types::Plan, RuntimeError> {
            Err(RuntimeError::Generic("Stub".to_string()))
        }
        
        async fn validate_plan(&self, _plan_content: &str) -> Result<crate::arbiter::llm_provider::ValidationResult, RuntimeError> {
            Err(RuntimeError::Generic("Stub".to_string()))
        }
        
        async fn generate_text(&self, _prompt: &str) -> Result<String, RuntimeError> {
            Ok(r#"{"primary_action": "test", "target_object": "test", "domain_keywords": [], "synonyms": [], "implied_concepts": [], "expanded_queries": [], "confidence": 0.5}"#.to_string())
        }
        
        fn get_info(&self) -> crate::arbiter::llm_provider::LlmProviderInfo {
            crate::arbiter::llm_provider::LlmProviderInfo {
                name: "Stub".to_string(),
                version: "1.0".to_string(),
                model: "stub".to_string(),
                capabilities: vec![],
            }
        }
    }
}
