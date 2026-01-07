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
        format!(
            r#"You are an expert at analyzing user goals for API and tool discovery.

Given a user's goal, extract the intent and generate search queries for finding relevant APIs and MCP servers.

User Goal: "{goal}"

Analyze this goal and respond with ONLY a JSON object in this exact format:
{{
  "primary_action": "the main verb/action (e.g., list, get, search, send, track, create)",
  "target_object": "the main target (e.g., issues, pull requests, SMS, weather, users)",
  "domain_keywords": ["key", "domain", "words", "and", "service", "names"],
  "synonyms": ["alternative", "terms", "for", "target"],
  "implied_concepts": ["concepts", "not", "stated", "but", "implied"],
  "expanded_queries": ["query 1 for registry search", "query 2 with synonyms", "query 3 with service name"],
  "confidence": 0.85
}}

Guidelines:
- primary_action: Extract the core action verb
- target_object: The main thing being acted upon
- domain_keywords: Keywords AND well-known service names for this domain. Examples:
  * For bugs/issues: include "github", "jira", "linear", "gitlab"
  * For messaging: include "slack", "discord", "twilio", "telegram"
  * For weather: include "openweathermap", "weatherapi"
  * For databases: include "postgres", "mysql", "mongodb", "sqlite"
  * For email: include "sendgrid", "mailgun", "ses"
- synonyms: Alternative names for the target (PRs for pull requests, SMS for text messages)
- implied_concepts: What's implied but not said (e.g., "track progress" implies issues, tasks, kanban)
- expanded_queries: 3-5 search queries that MUST include:
  * At least one query with the target object name (e.g., "issues", "bugs")
  * At least one query with a well-known service name (e.g., "github", "jira")
  * At least one query with a synonym or related concept
- confidence: How confident you are in your analysis (0.0-1.0)

IMPORTANT: The MCP registry searches by name and description, so include specific service names in your queries.

Respond with ONLY the JSON object, no markdown formatting or explanation."#
        )
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
                log::warn!(
                    "Failed to parse LLM intent analysis: {}. Response: {}",
                    e,
                    response
                );
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
                log::warn!(
                    "Failed to parse LLM ranking response: {}. Response: {}",
                    e,
                    response
                );
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
                (
                    r.score,
                    r.reasoning.clone(),
                    r.recommended || r.score >= 0.6,
                )
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
        ranked.sort_by(|a, b| {
            b.llm_score
                .partial_cmp(&a.llm_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(ranked)
    }

    /// Fallback intent analysis when LLM parsing fails
    pub fn fallback_intent_analysis(&self, goal: &str) -> IntentAnalysis {
        let goal_lower = goal.to_lowercase();
        let words: Vec<&str> = goal_lower.split_whitespace().collect();

        // Simple action extraction
        let action_verbs = [
            "list", "get", "search", "find", "create", "send", "track", "check", "fetch",
        ];
        let primary_action = words
            .iter()
            .find(|w| action_verbs.contains(w))
            .map(|s| s.to_string())
            .unwrap_or_else(|| "search".to_string());

        // Extract non-verb words as keywords
        let stopwords = [
            "the", "a", "an", "for", "in", "on", "to", "and", "or", "of", "with",
        ];
        let domain_keywords: Vec<String> = words
            .iter()
            .filter(|w| !action_verbs.contains(w) && !stopwords.contains(w) && w.len() > 2)
            .map(|s| s.to_string())
            .collect();

        let target_object = domain_keywords
            .first()
            .cloned()
            .unwrap_or_else(|| "items".to_string());

        IntentAnalysis {
            primary_action,
            target_object: target_object.clone(),
            domain_keywords: domain_keywords.clone(),
            synonyms: Vec::new(),
            implied_concepts: Vec::new(),
            expanded_queries: vec![goal.to_string(), domain_keywords.join(" ")],
            confidence: 0.3,
        }
    }

    /// Search for external APIs matching a query
    ///
    /// This uses LLM to:
    /// 1. Generate potential API service names for the query
    /// 2. If url_hint provided, parse documentation to extract API info
    /// 3. Return discovered API endpoints for approval
    pub async fn search_external_apis(
        &self,
        query: &str,
        url_hint: Option<&str>,
    ) -> RuntimeResult<Vec<ExternalApiResult>> {
        // If URL hint provided, try to parse it directly
        if let Some(url) = url_hint {
            return self.discover_api_from_url(url, query).await;
        }

        // Otherwise, use LLM to suggest APIs
        let prompt = format!(
            r#"You are an expert at finding REST APIs and web services.

Given a user's goal, suggest up to 3 well-known APIs that could help.

User Goal: "{query}"

Respond with ONLY a JSON array (no markdown):
[
  {{
    "name": "Service Name",
    "endpoint": "https://api.example.com",
    "docs_url": "https://example.com/docs/api",
    "description": "What this API provides",
    "auth_env_var": "SERVICE_API_KEY"
  }}
]

Guidelines:
- Only suggest real, well-known APIs (OpenWeatherMap, GitHub, Twilio, etc.)
- endpoint: the actual API base URL (e.g., https://api.openweathermap.org)
- docs_url: the API documentation/quickstart page URL (IMPORTANT for introspection)
- auth_env_var should be a conventional env var name
- If unsure, return an empty array []

Respond with ONLY the JSON array."#
        );

        let response = self.provider.generate_text(&prompt).await?;
        let json_str = extract_json(&response);

        #[derive(serde::Deserialize)]
        struct ApiSuggestion {
            name: String,
            endpoint: String,
            #[serde(default)]
            docs_url: Option<String>,
            description: String,
            #[serde(default)]
            auth_env_var: Option<String>,
        }

        let suggestions: Vec<ApiSuggestion> = match serde_json::from_str(json_str) {
            Ok(s) => s,
            Err(e) => {
                log::warn!(
                    "Failed to parse API suggestions: {}. Response: {}",
                    e,
                    response
                );
                return Ok(Vec::new());
            }
        };

        Ok(suggestions
            .into_iter()
            .map(|s| ExternalApiResult {
                name: s.name,
                endpoint: s.endpoint,
                docs_url: s.docs_url,
                description: s.description,
                auth_env_var: s.auth_env_var,
                source: "llm_suggestion".to_string(),
            })
            .collect())
    }

    /// Discover APIs from a documentation URL
    async fn discover_api_from_url(
        &self,
        url: &str,
        query: &str,
    ) -> RuntimeResult<Vec<ExternalApiResult>> {
        // Fetch the page content with browser-like headers to avoid bot detection
        let client = reqwest::Client::new();
        let response = client
            .get(url)
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
            .header("Accept-Language", "en-US,en;q=0.5")
            .send()
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to fetch URL {}: {}", url, e)))?;

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        // Read the response body once
        let text = response
            .text()
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to read response: {}", e)))?;

        // Check if it's an OpenAPI spec
        if content_type.contains("json") || url.ends_with(".json") {
            // Try to parse as OpenAPI
            if text.contains("\"openapi\"") || text.contains("\"swagger\"") {
                // It's an OpenAPI spec - extract info
                if let Ok(spec) = serde_json::from_str::<serde_json::Value>(&text) {
                    let title = spec["info"]["title"].as_str().unwrap_or("API");
                    let description = spec["info"]["description"].as_str().unwrap_or("");

                    // Extract server URL from spec or use the original URL's base
                    let api_url = spec["servers"][0]["url"]
                        .as_str()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| url.split("/openapi").next().unwrap_or(url).to_string());

                    return Ok(vec![ExternalApiResult {
                        name: title.to_string(),
                        endpoint: api_url,
                        docs_url: Some(url.to_string()),
                        description: description.to_string(),
                        auth_env_var: None,
                        source: "openapi_spec".to_string(),
                    }]);
                }
            }
        }

        // For HTML pages, use LLM to extract API info

        // Check for Cloudflare bot protection
        if text.contains("Just a moment...") && text.contains("cf_chl") {
            return Err(RuntimeError::Generic(format!(
                "URL {} is protected by Cloudflare bot detection. Try using a browser-based approach or provide an OpenAPI spec URL directly.",
                url
            )));
        }

        // Check if text is too short or looks like an error page
        if text.len() < 500 {
            return Err(RuntimeError::Generic(format!(
                "Page content too short ({} bytes) - may be an error page or empty response",
                text.len()
            )));
        }

        // Truncate text to avoid token limits
        let truncated_text = if text.len() > 10000 {
            &text[..10000]
        } else {
            &text
        };

        // Log first part of content for debugging
        eprintln!(
            "[CCOS] HTML content preview for {}: {}...",
            url,
            &text[..text.len().min(200)]
        );

        let prompt = format!(
            r#"You are analyzing an API documentation page to extract API information.

URL: {url}
User's Goal: {query}

Page Content (truncated):
{content}

Extract API information and respond with ONLY a JSON array (no markdown):
[
  {{
    "name": "API Name",
    "endpoint": "https://api.domain.com/...",
    "description": "What the API does",
    "auth_env_var": "SUGGESTED_API_KEY"
  }}
]

Guidelines:
- Extract the actual API endpoint URL, not the docs URL
- Look for base URLs in examples, code snippets, or API reference sections
- If it's OpenWeatherMap, the endpoint is https://api.openweathermap.org
- If it's GitHub, the endpoint is https://api.github.com
- If unsure, return an empty array []

Respond with ONLY the JSON array."#,
            url = url,
            query = query,
            content = truncated_text
        );

        let llm_response = self.provider.generate_text(&prompt).await?;
        let json_str = extract_json(&llm_response);

        #[derive(serde::Deserialize)]
        struct LlmApiInfo {
            name: String,
            endpoint: String,
            description: String,
            #[serde(default)]
            auth_env_var: Option<String>,
        }

        let apis: Vec<LlmApiInfo> = match serde_json::from_str(json_str) {
            Ok(a) => a,
            Err(e) => {
                log::warn!(
                    "Failed to parse API info from LLM: {}. Response: {}",
                    e,
                    llm_response
                );
                return Ok(Vec::new());
            }
        };

        Ok(apis
            .into_iter()
            .map(|a| ExternalApiResult {
                name: a.name,
                endpoint: a.endpoint,
                docs_url: Some(url.to_string()),
                description: a.description,
                auth_env_var: a.auth_env_var,
                source: "html_docs".to_string(),
            })
            .collect())
    }
}

/// Result from external API discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalApiResult {
    /// Name of the API/service
    pub name: String,
    /// Base endpoint URL
    pub endpoint: String,
    /// Documentation URL for introspection (quickstart, API reference page)
    pub docs_url: Option<String>,
    /// Description of what the API does
    pub description: String,
    /// Suggested environment variable for auth
    pub auth_env_var: Option<String>,
    /// Source of discovery (llm_suggestion, openapi_spec, html_docs)
    pub source: String,
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

        async fn validate_plan(
            &self,
            _plan_content: &str,
        ) -> Result<crate::arbiter::llm_provider::ValidationResult, RuntimeError> {
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
