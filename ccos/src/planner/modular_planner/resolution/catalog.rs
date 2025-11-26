//! Catalog resolution strategy
//!
//! Resolves intents using the CCOS capability catalog (registered capabilities).

use std::sync::Arc;
use async_trait::async_trait;

use super::semantic::{CapabilityCatalog, CapabilityInfo};
use super::{ResolutionContext, ResolutionError, ResolutionStrategy, ResolvedCapability};
use crate::planner::modular_planner::types::{DomainHint, IntentType, SubIntent};

/// Catalog resolution configuration
#[derive(Debug, Clone)]
pub struct CatalogConfig {
    /// Whether to validate input schema
    pub validate_schema: bool,
    /// Whether to attempt adapting missing parameters
    pub allow_adaptation: bool,
}

impl Default for CatalogConfig {
    fn default() -> Self {
        Self {
            validate_schema: true,
            allow_adaptation: true,
        }
    }
}

/// Catalog resolution strategy.
/// 
/// Searches the local capability catalog for matching capabilities.
/// This is simpler than SemanticResolution and doesn't use embeddings.
pub struct CatalogResolution {
    catalog: Arc<dyn CapabilityCatalog>,
    config: CatalogConfig,
}

impl CatalogResolution {
    pub fn new(catalog: Arc<dyn CapabilityCatalog>) -> Self {
        Self { 
            catalog,
            config: CatalogConfig::default(),
        }
    }

    pub fn with_config(mut self, config: CatalogConfig) -> Self {
        self.config = config;
        self
    }
    
    /// Map intent to built-in capability if applicable
    fn check_builtin(&self, intent: &SubIntent) -> Option<ResolvedCapability> {
        match &intent.intent_type {
            IntentType::UserInput { prompt_topic } => {
                let mut args = std::collections::HashMap::new();
                // Generate a more descriptive prompt based on common parameter names and domain
                let prompt = Self::humanize_prompt(prompt_topic, intent.domain_hint.as_ref());
                args.insert("prompt".to_string(), prompt);
                
                Some(ResolvedCapability::BuiltIn {
                    capability_id: "ccos.user.ask".to_string(),
                    arguments: args,
                })
            }
            IntentType::Output { format: _ } => {
                let mut args = std::collections::HashMap::new();
                args.insert("message".to_string(), intent.description.clone());
                
                Some(ResolvedCapability::BuiltIn {
                    capability_id: "ccos.io.println".to_string(),
                    arguments: args,
                })
            }
            _ => None,
        }
    }
    
    /// Validate and adapt arguments against capability schema
    fn validate_and_adapt(
        &self, 
        intent: &SubIntent, 
        cap: &CapabilityInfo, 
        args: &mut std::collections::HashMap<String, String>
    ) -> bool {
        if !self.config.validate_schema {
            return true;
        }

        if let Some(schema) = &cap.input_schema {
            // Basic JSON schema check for required fields
            if let Some(required) = schema.get("required").and_then(|v| v.as_array()) {
                for field in required {
                    if let Some(field_name) = field.as_str() {
                        if !args.contains_key(field_name) {
                            // Missing required field!
                            if self.config.allow_adaptation {
                                // Attempt adaptation for common patterns
                                if field_name == "prompt" || field_name == "question" {
                                    // Synthesize prompt from description
                                    args.insert(field_name.to_string(), intent.description.clone());
                                    continue;
                                }
                                if field_name == "message" {
                                    args.insert(field_name.to_string(), intent.description.clone());
                                    continue;
                                }
                            }
                            
                            // If we get here, we failed to adapt
                            // println!("DEBUG: Rejected capability {} due to missing required param: {}", cap.id, field_name);
                            return false;
                        }
                    }
                }
            }
        }
        true
    }

    /// Search catalog for matching capability
    async fn search_catalog(&self, intent: &SubIntent) -> Option<(CapabilityInfo, f64, std::collections::HashMap<String, String>)> {
        // First, check for LLM-suggested tool (from GroundedLlmDecomposition)
        if let Some(suggested_tool) = intent.extracted_params.get("_suggested_tool") {
            // Try direct lookup by suggested tool name
            let possible_ids = vec![
                suggested_tool.clone(),
                format!("mcp.github.{}", suggested_tool),
                format!("ccos.{}", suggested_tool),
            ];
            
            for possible_id in possible_ids {
                if let Some(cap) = self.catalog.get_capability(&possible_id).await {
                    let mut arguments: std::collections::HashMap<String, String> = intent.extracted_params
                        .iter()
                        .filter(|(k, _)| !k.starts_with('_'))
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    
                    if self.validate_and_adapt(intent, &cap, &mut arguments) {
                        return Some((cap, 0.95, arguments)); // High confidence for direct match
                    }
                }
            }
        }
        
        // Fall back to search
        let query = &intent.description;
        let candidates = self.catalog.search(query, 10).await; // Increased limit for better coverage
        
        if candidates.is_empty() {
            return None;
        }
        
        // Score all candidates and pick the best
        let mut scored: Vec<(CapabilityInfo, f64, std::collections::HashMap<String, String>)> = Vec::new();
        
        for cap in candidates {
            let score = self.score_capability(intent, &cap);
            if score > 0.2 {
                // Prepare arguments
                let mut arguments: std::collections::HashMap<String, String> = intent.extracted_params
                    .iter()
                    .filter(|(k, _)| !k.starts_with('_'))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();

                // Validate and adapt
                if self.validate_and_adapt(intent, &cap, &mut arguments) {
                    scored.push((cap, score, arguments));
                }
            }
        }
        
        // Return the highest-scoring match
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.into_iter().next()
    }
    
    /// Score capability match with improved algorithm
    fn score_capability(&self, intent: &SubIntent, cap: &CapabilityInfo) -> f64 {
        let cap_name_lower = cap.name.to_lowercase();
        let cap_desc_lower = cap.description.to_lowercase();
        let desc_lower = intent.description.to_lowercase();
        
        // Tokenize intent and capability name
        let intent_words: Vec<&str> = desc_lower
            .split(|c: char| c.is_whitespace() || c == '_' || c == '-' || c == '.')
            .filter(|w| w.len() > 2)
            .collect();
        
        let cap_name_words: Vec<&str> = cap_name_lower
            .split(|c: char| c == '_' || c == '-' || c == '.')
            .filter(|w| w.len() > 1)
            .collect();
        
        if intent_words.is_empty() {
            return 0.0;
        }
        
        // Common action verbs (not nouns) - used to identify the "object" of the action
        let action_verbs = ["list", "get", "search", "create", "update", "delete", "find", "retrieve", "fetch", "read", "write", "add", "remove"];
        let stop_words = ["the", "and", "for", "with", "from", "into", "user", "provided", "filters", "pagination"];
        
        let mut score: f64 = 0.0;
        let mut _matched_name_words = 0;
        
        // 1. Action verb alignment
        let intent_action = intent_words.iter().find(|w| action_verbs.contains(&w.to_lowercase().as_str()));
        let cap_action = cap_name_words.iter().find(|w| action_verbs.contains(&w.to_lowercase().as_str()));
        
        if let (Some(ia), Some(ca)) = (intent_action, cap_action) {
            if ia.to_lowercase() == ca.to_lowercase() {
                score += 0.2;
            }
        }
        
        // 2. Find the "object noun" in capability name (the non-verb, non-stop word)
        let cap_object_nouns: Vec<&str> = cap_name_words.iter()
            .filter(|w| !action_verbs.contains(&w.to_lowercase().as_str()))
            .copied()
            .collect();
        
        // 3. Check for object noun matches between intent and capability
        for word in &intent_words {
            let word_lower = word.to_lowercase();
            
            // Skip stop words and action verbs
            if stop_words.contains(&word_lower.as_str()) || action_verbs.contains(&word_lower.as_str()) {
                continue;
            }
            
            // Check for match in capability name (exact or singular/plural)
            let name_match = cap_object_nouns.iter().any(|cw| {
                let cw_lower = cw.to_lowercase();
                cw_lower == word_lower || 
                cw_lower == format!("{}s", word_lower) ||
                word_lower == format!("{}s", cw_lower) ||
                cw_lower == format!("{}es", word_lower) ||
                word_lower == format!("{}es", cw_lower)
            });
            
            if name_match {
                score += 0.4; // Strong bonus for object noun match in name
                _matched_name_words += 1;
            }
            // Partial match in description (lower weight)
            else if cap_desc_lower.contains(&word_lower) {
                score += 0.05;
            }
        }
        
        // 4. Penalize if capability has object nouns NOT mentioned in intent
        // This prevents "list_branches" matching "list issues"
        for cap_noun in &cap_object_nouns {
            let cap_noun_lower = cap_noun.to_lowercase();
            
            let intent_has_noun = intent_words.iter().any(|w| {
                let w_lower = w.to_lowercase();
                w_lower == cap_noun_lower ||
                w_lower == format!("{}s", cap_noun_lower) ||
                cap_noun_lower == format!("{}s", w_lower) ||
                w_lower == format!("{}es", cap_noun_lower) ||
                cap_noun_lower == format!("{}es", w_lower)
            });
            
            if !intent_has_noun {
                score -= 0.5; // Strong penalty for unmentioned object nouns
            }
        }
        
        // 5. Penalize extra specificity (e.g., "types" suffix)
        for cap_word in &cap_name_words {
            let cap_word_lower = cap_word.to_lowercase();
            // If capability has a qualifier word not in intent, penalize
            if !intent_words.iter().any(|w| w.to_lowercase() == cap_word_lower) {
                // Small penalty for each unmatched word in capability name
                if !action_verbs.contains(&cap_word_lower.as_str()) {
                    score -= 0.15;
                }
            }
        }
        
        // 6. Bonus for suggested tool match
        if let Some(suggested) = intent.extracted_params.get("_suggested_tool") {
            if cap.id.contains(suggested) || cap.name.to_lowercase().contains(&suggested.to_lowercase()) {
                score += 0.5;
            }
        }
        
        score.max(0.0_f64).min(1.0_f64)
    }
    
    /// Generate human-friendly prompt text from a parameter name
    /// Generate human-friendly prompt text from a parameter name, considering domain context
    fn humanize_prompt(topic: &str, domain: Option<&DomainHint>) -> String {
        // Clean up topic - remove common filler words and normalize
        let filler_words = ["first", "please", "the", "a", "an", "now", "then"];
        let topic_lower: String = topic.to_lowercase()
            .replace(['_', '-'], " ")
            .split_whitespace()
            .filter(|w| !filler_words.contains(w))
            .collect::<Vec<_>>()
            .join(" ");
        let topic_lower = topic_lower.trim();
        
        // 1. Try domain-specific prompts first
        if let Some(domain) = domain {
            if let Some(prompt) = Self::domain_specific_prompt(topic_lower, domain) {
                return prompt;
            }
        }
        
        // 2. Fall back to generic/universal prompts
        Self::generic_prompt(topic_lower, topic)
    }
    
    /// Domain-specific prompt generation
    fn domain_specific_prompt(topic: &str, domain: &DomainHint) -> Option<String> {
        match domain {
            DomainHint::GitHub => Self::github_prompt(topic),
            DomainHint::Slack => Self::slack_prompt(topic),
            DomainHint::FileSystem => Self::filesystem_prompt(topic),
            DomainHint::Database => Self::database_prompt(topic),
            DomainHint::Web => Self::web_prompt(topic),
            DomainHint::Email => Self::email_prompt(topic),
            DomainHint::Calendar => Self::calendar_prompt(topic),
            DomainHint::Generic | DomainHint::Custom(_) => None,
        }
    }
    
    /// GitHub-specific prompts
    fn github_prompt(topic: &str) -> Option<String> {
        Some(match topic {
            "state" => "Issue/PR state: open, closed, or all?".to_string(),
            "labels" => "Labels to filter by (comma-separated, e.g., bug, enhancement)".to_string(),
            "assignee" => "Assignee username (or 'none' for unassigned, '*' for any)".to_string(),
            "creator" | "author" => "Creator/author GitHub username".to_string(),
            "milestone" => "Milestone number or title".to_string(),
            "sort" => "Sort by: created, updated, or comments?".to_string(),
            "owner" => "Repository owner (username or organization)".to_string(),
            "repo" | "repository" => "Repository name".to_string(),
            "branch" => "Branch name (e.g., main, develop)".to_string(),
            "base" => "Base branch for PR (e.g., main)".to_string(),
            "head" => "Head branch for PR (your feature branch)".to_string(),
            _ => return None,
        })
    }
    
    /// Slack-specific prompts
    fn slack_prompt(topic: &str) -> Option<String> {
        Some(match topic {
            "channel" => "Slack channel name or ID (e.g., #general)".to_string(),
            "user" | "username" => "Slack username or user ID".to_string(),
            "message" | "text" => "Message text to send".to_string(),
            "thread ts" | "thread" => "Thread timestamp (for replies)".to_string(),
            "emoji" => "Emoji name (e.g., thumbsup, rocket)".to_string(),
            _ => return None,
        })
    }
    
    /// File system-specific prompts
    fn filesystem_prompt(topic: &str) -> Option<String> {
        Some(match topic {
            "path" | "file" | "filepath" => "File or directory path".to_string(),
            "directory" | "folder" | "dir" => "Directory path".to_string(),
            "filename" | "name" => "File name".to_string(),
            "extension" | "ext" => "File extension (e.g., .txt, .json)".to_string(),
            "content" | "contents" => "File content to write".to_string(),
            "pattern" | "glob" => "File pattern (e.g., *.txt, **/*.rs)".to_string(),
            _ => return None,
        })
    }
    
    /// Database-specific prompts
    fn database_prompt(topic: &str) -> Option<String> {
        Some(match topic {
            "table" => "Database table name".to_string(),
            "query" | "sql" => "SQL query to execute".to_string(),
            "column" | "field" => "Column/field name".to_string(),
            "where" | "condition" => "WHERE condition (e.g., id = 1)".to_string(),
            "limit" => "Maximum number of rows to return".to_string(),
            "offset" => "Number of rows to skip".to_string(),
            _ => return None,
        })
    }
    
    /// Web/HTTP-specific prompts
    fn web_prompt(topic: &str) -> Option<String> {
        Some(match topic {
            "url" | "endpoint" => "URL or API endpoint".to_string(),
            "method" => "HTTP method: GET, POST, PUT, DELETE".to_string(),
            "headers" => "HTTP headers (JSON format)".to_string(),
            "body" | "payload" => "Request body/payload".to_string(),
            "timeout" => "Request timeout in seconds".to_string(),
            _ => return None,
        })
    }
    
    /// Email-specific prompts
    fn email_prompt(topic: &str) -> Option<String> {
        Some(match topic {
            "to" | "recipient" => "Recipient email address".to_string(),
            "from" | "sender" => "Sender email address".to_string(),
            "subject" => "Email subject line".to_string(),
            "body" | "content" => "Email body content".to_string(),
            "cc" => "CC recipients (comma-separated)".to_string(),
            "bcc" => "BCC recipients (comma-separated)".to_string(),
            _ => return None,
        })
    }
    
    /// Calendar-specific prompts
    fn calendar_prompt(topic: &str) -> Option<String> {
        Some(match topic {
            "title" | "summary" => "Event title/summary".to_string(),
            "start" | "start time" => "Event start time (YYYY-MM-DD HH:MM)".to_string(),
            "end" | "end time" => "Event end time (YYYY-MM-DD HH:MM)".to_string(),
            "date" => "Event date (YYYY-MM-DD)".to_string(),
            "duration" => "Event duration (e.g., 1h, 30m)".to_string(),
            "location" => "Event location".to_string(),
            "attendees" => "Attendees (comma-separated emails)".to_string(),
            _ => return None,
        })
    }
    
    /// Generic/universal prompts (domain-agnostic)
    fn generic_prompt(topic: &str, original: &str) -> String {
        match topic {
            // Universal pagination
            "perpage" | "per page" | "page size" | "limit" => {
                "How many items per page? (e.g., 10, 25, 50)".to_string()
            }
            "page" | "page number" => {
                "Which page number? (starting from 1)".to_string()
            }
            "cursor" | "after" | "before" => {
                format!("Pagination cursor (or leave blank for first page)")
            }
            
            // Universal sorting/ordering
            "sort" | "sort by" | "order by" => {
                "Sort by which field?".to_string()
            }
            "direction" | "order" => {
                "Sort direction: asc (ascending) or desc (descending)?".to_string()
            }
            
            // Universal filtering
            "query" | "q" | "search" => {
                "Search query (keywords to find)".to_string()
            }
            "filter" | "filters" => {
                "Filter criteria".to_string()
            }
            
            // Universal time
            "since" | "from" | "start date" => {
                "Start date (YYYY-MM-DD format)".to_string()
            }
            "until" | "to" | "end date" => {
                "End date (YYYY-MM-DD format)".to_string()
            }
            
            // Default: use the topic with basic formatting
            _ => {
                if original.len() <= 15 {
                    format!("Please provide: {}", original)
                } else {
                    original.to_string()
                }
            }
        }
    }
}

#[async_trait(?Send)]
impl ResolutionStrategy for CatalogResolution {
    fn name(&self) -> &str {
        "catalog"
    }
    
    fn can_handle(&self, _intent: &SubIntent) -> bool {
        true // Can try to handle any intent
    }
    
    async fn resolve(
        &self,
        intent: &SubIntent,
        _context: &ResolutionContext,
    ) -> Result<ResolvedCapability, ResolutionError> {
        // Check for built-in first
        if let Some(builtin) = self.check_builtin(intent) {
            return Ok(builtin);
        }
        
        // Search catalog
        if let Some((cap, score, arguments)) = self.search_catalog(intent).await {
            return Ok(ResolvedCapability::Local {
                capability_id: cap.id,
                arguments,
                confidence: score,
                input_schema: cap.input_schema,
            });
        }
        
        Err(ResolutionError::NotFound(format!(
            "No catalog capability found for: {}",
            intent.description
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    struct MockCatalog;
    
    #[async_trait(?Send)]
    impl CapabilityCatalog for MockCatalog {
        async fn list_capabilities(&self, _domain: Option<&str>) -> Vec<CapabilityInfo> {
            vec![]
        }
        
        async fn get_capability(&self, _id: &str) -> Option<CapabilityInfo> {
            None
        }
        
        async fn search(&self, _query: &str, _limit: usize) -> Vec<CapabilityInfo> {
            vec![]
        }
    }
    
    #[tokio::test]
    async fn test_builtin_user_input() {
        let catalog = Arc::new(MockCatalog);
        let strategy = CatalogResolution::new(catalog);
        let context = ResolutionContext::new();
        
        let intent = SubIntent::new(
            "Ask user for page size",
            IntentType::UserInput { prompt_topic: "page size".to_string() },
        );
        
        let result = strategy.resolve(&intent, &context).await.expect("Should resolve");
        
        match result {
            ResolvedCapability::BuiltIn { capability_id, arguments } => {
                assert_eq!(capability_id, "ccos.user.ask");
                assert!(arguments.get("prompt").unwrap().contains("page size"));
            }
            _ => panic!("Expected BuiltIn capability"),
        }
    }
}
