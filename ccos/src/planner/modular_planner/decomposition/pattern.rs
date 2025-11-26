//! Pattern-based decomposition strategy
//!
//! Uses regex patterns to recognize common goal structures and decompose them
//! without needing an LLM. This is fast and deterministic for known patterns.

use std::collections::HashMap;
use async_trait::async_trait;
use regex::Regex;
use once_cell::sync::Lazy;

use super::{DecompositionContext, DecompositionError, DecompositionResult, DecompositionStrategy};
use crate::planner::modular_planner::types::{
    ApiAction, DomainHint, IntentType, SubIntent, ToolSummary, TransformType,
};

// ============================================================================
// Static Regexes - Defined at module level to avoid interior mutability issues
// ============================================================================

static PATTERN_ACTION_WITH_USER_INPUT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^(.+?)\s+but\s+(?:ask|prompt)\s+(?:me|user|the user)\s+(?:for|about)\s+(?:the\s+)?(.+)$").unwrap()
});

static PATTERN_USER_INPUT_THEN_ACTION: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^(?:ask|prompt)\s+(?:me|user)\s+(?:for|about)\s+(?:the\s+)?(.+?)\s+(?:then|and then|and)\s+(.+)$").unwrap()
});

static PATTERN_SEQUENTIAL_ACTIONS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^(.+?)\s+(?:and\s+)?then\s+(.+)$").unwrap()
});

static PATTERN_ACTION_WITH_TRANSFORM: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^(.+?)\s+and\s+(filter|sort|group|count|aggregate)\s+(?:by|for|on)?\s*(.*)$").unwrap()
});

static OWNER_REPO_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?:(?:owner\s+)?(\w+)[/\\](\w+))|(?:(?:user|owner)\s+(\w+).*?(?:repo(?:sitory)?)\s+(\w+))|(?:(?:repo(?:sitory)?)\s+(\w+).*?(?:user|owner)\s+(\w+))").unwrap()
});

/// A pattern with name and handler
struct PatternDef {
    name: &'static str,
    handler: fn(&regex::Captures, &str, &DecompositionContext) -> Option<Vec<SubIntent>>,
}

/// Pattern-based decomposition strategy.
/// 
/// Recognizes common goal patterns using regex and produces sub-intents
/// without LLM calls. This is the fastest strategy but only works for
/// known patterns.
pub struct PatternDecomposition {
    /// Whether to allow partial matches (lower confidence)
    allow_partial: bool,
}

impl PatternDecomposition {
    pub fn new() -> Self {
        Self { allow_partial: true }
    }
    
    pub fn strict() -> Self {
        Self { allow_partial: false }
    }
    
    /// Try all patterns and return the first match
    fn try_patterns(&self, goal: &str, context: &DecompositionContext) -> Option<(Vec<SubIntent>, &'static str)> {
        // Pattern 1: "X but ask me for Y"
        if let Some(captures) = PATTERN_ACTION_WITH_USER_INPUT.captures(goal) {
            if let Some(intents) = handle_action_with_user_input(&captures, goal, context) {
                return Some((intents, "action_with_user_input"));
            }
        }
        
        // Pattern 2: "ask me for X then Y"
        if let Some(captures) = PATTERN_USER_INPUT_THEN_ACTION.captures(goal) {
            if let Some(intents) = handle_user_input_then_action(&captures, goal, context) {
                return Some((intents, "user_input_then_action"));
            }
        }
        
        // Pattern 3: "X then Y"
        if let Some(captures) = PATTERN_SEQUENTIAL_ACTIONS.captures(goal) {
            if let Some(intents) = handle_sequential_actions(&captures, goal, context) {
                return Some((intents, "sequential_actions"));
            }
        }
        
        // Pattern 4: "X and filter/sort by Y"
        if let Some(captures) = PATTERN_ACTION_WITH_TRANSFORM.captures(goal) {
            if let Some(intents) = handle_action_with_transform(&captures, goal, context) {
                return Some((intents, "action_with_transform"));
            }
        }
        
        None
    }
    
    /// Check if any pattern matches AND can be fully handled
    /// Returns false if pattern matches but contains complexity we can't handle
    fn has_pattern_match(&self, goal: &str) -> bool {
        // First check: does any regex match?
        let has_regex_match = PATTERN_ACTION_WITH_USER_INPUT.is_match(goal) ||
            PATTERN_USER_INPUT_THEN_ACTION.is_match(goal) ||
            PATTERN_SEQUENTIAL_ACTIONS.is_match(goal) ||
            PATTERN_ACTION_WITH_TRANSFORM.is_match(goal);
        
        if !has_regex_match {
            return false;
        }
        
        // Second check: does the goal contain complexity signals that patterns can't handle?
        // If so, we should defer to LLM even though regex matched.
        // "Ask, don't guess" principle.
        if Self::has_unhandled_complexity(goal) {
            log::debug!("[pattern] Goal has complexity signals that patterns can't fully handle");
            return false;
        }
        
        true
    }
    
    /// Check if goal contains complexity signals that patterns can't handle properly
    fn has_unhandled_complexity(goal: &str) -> bool {
        let lower = goal.to_lowercase();
        
        // User interaction signals that require proper decomposition
        let user_signals = [
            "asked to the user", "asked to user", "ask the user", "ask user",
            "user provides", "user input", "user chooses", "user selects",
            "prompt the user", "prompt user",
        ];
        
        // Multiple "and" clauses often indicate complex multi-step goals
        let and_count = lower.matches(" and ").count();
        
        // Check for user signals
        if user_signals.iter().any(|s| lower.contains(s)) {
            return true;
        }
        
        // Multiple "and" clauses suggest complexity beyond simple patterns
        if and_count > 1 {
            return true;
        }
        
        false
    }
    
    /// Extract parameters from goal (owner/repo, etc.)
    fn extract_common_params(goal: &str) -> HashMap<String, String> {
        let mut params = HashMap::new();
        
        if let Some(caps) = OWNER_REPO_REGEX.captures(goal) {
            if let (Some(owner), Some(repo)) = (caps.get(1), caps.get(2)) {
                params.insert("owner".to_string(), owner.as_str().to_string());
                params.insert("repo".to_string(), repo.as_str().to_string());
            } else if let (Some(owner), Some(repo)) = (caps.get(3), caps.get(4)) {
                params.insert("owner".to_string(), owner.as_str().to_string());
                params.insert("repo".to_string(), repo.as_str().to_string());
            } else if let (Some(repo), Some(owner)) = (caps.get(5), caps.get(6)) {
                params.insert("owner".to_string(), owner.as_str().to_string());
                params.insert("repo".to_string(), repo.as_str().to_string());
            }
        }
        
        params
    }
}

impl Default for PatternDecomposition {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl DecompositionStrategy for PatternDecomposition {
    fn name(&self) -> &str {
        "pattern"
    }
    
    fn can_handle(&self, goal: &str) -> f64 {
        if self.has_pattern_match(goal) {
            return 0.9; // High confidence for pattern matches
        }
        
        // Check for simple single-action goals
        if is_simple_api_goal(goal) {
            return 0.7;
        }
        
        0.0
    }
    
    async fn decompose(
        &self,
        goal: &str,
        _available_tools: Option<&[ToolSummary]>,
        context: &DecompositionContext,
    ) -> Result<DecompositionResult, DecompositionError> {
        // Merge pre-extracted params with newly extracted ones
        let mut all_params = Self::extract_common_params(goal);
        for (k, v) in &context.pre_extracted_params {
            all_params.insert(k.clone(), v.clone());
        }
        
        // Create context with merged params
        let enriched_context = DecompositionContext {
            pre_extracted_params: all_params.clone(),
            ..context.clone()
        };
        
        // Try pattern matching first
        if let Some((mut intents, pattern_name)) = self.try_patterns(goal, &enriched_context) {
            // Inject extracted params into intents
            for intent in &mut intents {
                for (k, v) in &all_params {
                    if !intent.extracted_params.contains_key(k) {
                        intent.extracted_params.insert(k.clone(), v.clone());
                    }
                }
            }
            
            return Ok(DecompositionResult::atomic(intents, format!("pattern:{}", pattern_name))
                .with_confidence(0.9)
                .with_reasoning(format!("Matched pattern: {}", pattern_name)));
        }
        
        // Try simple API goal extraction
        if let Some(intent) = try_simple_api_goal(goal, &all_params) {
            return Ok(DecompositionResult::atomic(vec![intent], "pattern:simple_api")
                .with_confidence(0.7)
                .with_reasoning("Extracted simple API call from goal"));
        }
        
        Err(DecompositionError::PatternError(
            "No pattern matched the goal".to_string()
        ))
    }
}

// ============================================================================
// Pattern Handlers
// ============================================================================

/// Handle "X but ask me for Y" pattern
fn handle_action_with_user_input(
    captures: &regex::Captures,
    _goal: &str,
    _context: &DecompositionContext,
) -> Option<Vec<SubIntent>> {
    let main_action = captures.get(1)?.as_str().trim();
    let user_input_topic = captures.get(2)?.as_str().trim();
    
    // Infer domain and action from main action text
    let domain = DomainHint::infer_from_text(main_action);
    let action = infer_api_action(main_action);
    let domain_hint = domain.unwrap_or(DomainHint::Generic);
    
    Some(vec![
        // Step 1: Ask user for input (also gets domain for context-aware prompts)
        SubIntent::new(
            format!("Ask user for {}", user_input_topic),
            IntentType::UserInput { prompt_topic: user_input_topic.to_string() },
        )
        .with_domain(domain_hint.clone()),
        
        // Step 2: Execute main action with user input
        SubIntent::new(
            main_action.to_string(),
            IntentType::ApiCall { action },
        )
        .with_dependencies(vec![0])
        .with_domain(domain_hint),
    ])
}

/// Handle "ask me for X then Y" pattern
fn handle_user_input_then_action(
    captures: &regex::Captures,
    _goal: &str,
    _context: &DecompositionContext,
) -> Option<Vec<SubIntent>> {
    let user_input_topic = captures.get(1)?.as_str().trim();
    let action_text = captures.get(2)?.as_str().trim();
    
    let domain = DomainHint::infer_from_text(action_text);
    let action = infer_api_action(action_text);
    let domain_hint = domain.unwrap_or(DomainHint::Generic);
    
    Some(vec![
        SubIntent::new(
            format!("Ask user for {}", user_input_topic),
            IntentType::UserInput { prompt_topic: user_input_topic.to_string() },
        )
        .with_domain(domain_hint.clone()),
        SubIntent::new(
            action_text.to_string(),
            IntentType::ApiCall { action },
        )
        .with_dependencies(vec![0])
        .with_domain(domain_hint),
    ])
}

/// Handle "X then Y" sequential pattern
fn handle_sequential_actions(
    captures: &regex::Captures,
    _goal: &str,
    _context: &DecompositionContext,
) -> Option<Vec<SubIntent>> {
    let first_action = captures.get(1)?.as_str().trim();
    let second_action = captures.get(2)?.as_str().trim();
    
    // Skip if this is better handled by action_with_transform
    let transform_keywords = ["filter", "sort", "group", "count", "aggregate"];
    if transform_keywords.iter().any(|kw| second_action.to_lowercase().starts_with(kw)) {
        return None;
    }
    
    let domain1 = DomainHint::infer_from_text(first_action);
    let domain2 = DomainHint::infer_from_text(second_action);
    
    Some(vec![
        SubIntent::new(
            first_action.to_string(),
            IntentType::ApiCall { action: infer_api_action(first_action) },
        )
        .with_domain(domain1.unwrap_or(DomainHint::Generic)),
        
        SubIntent::new(
            second_action.to_string(),
            IntentType::ApiCall { action: infer_api_action(second_action) },
        )
        .with_dependencies(vec![0])
        .with_domain(domain2.unwrap_or(DomainHint::Generic)),
    ])
}

/// Handle "X and filter/sort/etc by Y" pattern
fn handle_action_with_transform(
    captures: &regex::Captures,
    _goal: &str,
    _context: &DecompositionContext,
) -> Option<Vec<SubIntent>> {
    let main_action = captures.get(1)?.as_str().trim();
    let transform_type = captures.get(2)?.as_str().trim().to_lowercase();
    let transform_target = captures.get(3).map(|m| m.as_str().trim()).unwrap_or("");
    
    // HUMBLE CHECK: If the transform target contains complexity we can't handle,
    // return None to let a smarter strategy (LLM) decompose this properly.
    // "Ask, don't guess" principle.
    let complexity_signals = [
        "ask me", "asked to", "ask user", "ask the user", "prompt me", "prompt user",
        "user provides", "user input", "user chooses", "user selects",
        " and then ", " then ", " after ", " before ",
    ];
    let target_lower = transform_target.to_lowercase();
    if complexity_signals.iter().any(|s| target_lower.contains(s)) {
        log::debug!(
            "[pattern] Transform target '{}' contains complexity signals, deferring to LLM",
            transform_target
        );
        return None; // Let LLM handle this
    }
    
    let domain = DomainHint::infer_from_text(main_action);
    
    let transform = match transform_type.as_str() {
        "filter" => TransformType::Filter,
        "sort" => TransformType::Sort,
        "group" => TransformType::GroupBy,
        "count" => TransformType::Count,
        "aggregate" => TransformType::Aggregate,
        _ => TransformType::Other(transform_type.clone()),
    };
    
    let mut intents = vec![
        SubIntent::new(
            main_action.to_string(),
            IntentType::ApiCall { action: infer_api_action(main_action) },
        )
        .with_domain(domain.unwrap_or(DomainHint::Generic)),
        
        SubIntent::new(
            format!("{} by {}", transform_type, transform_target),
            IntentType::DataTransform { transform },
        )
        .with_dependencies(vec![0]),
    ];
    
    // Add transform target as param if present
    if !transform_target.is_empty() {
        intents[1].extracted_params.insert("transform_target".to_string(), transform_target.to_string());
    }
    
    Some(intents)
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Infer API action from text
fn infer_api_action(text: &str) -> ApiAction {
    let lower = text.to_lowercase();
    
    if lower.starts_with("list ") || lower.contains(" list ") {
        ApiAction::List
    } else if lower.starts_with("get ") || lower.starts_with("fetch ") || lower.starts_with("show ") {
        ApiAction::Get
    } else if lower.starts_with("create ") || lower.starts_with("add ") || lower.starts_with("new ") {
        ApiAction::Create
    } else if lower.starts_with("update ") || lower.starts_with("edit ") || lower.starts_with("modify ") {
        ApiAction::Update
    } else if lower.starts_with("delete ") || lower.starts_with("remove ") {
        ApiAction::Delete
    } else if lower.starts_with("search ") || lower.starts_with("find ") {
        ApiAction::Search
    } else {
        ApiAction::Other(text.split_whitespace().next().unwrap_or("unknown").to_string())
    }
}

/// Check if goal is a simple single API action
fn is_simple_api_goal(goal: &str) -> bool {
    let lower = goal.to_lowercase();
    
    // Check for action verbs without complex conjunctions
    let action_starters = ["list ", "get ", "fetch ", "show ", "create ", "add ", 
                           "update ", "edit ", "delete ", "remove ", "search ", "find "];
    let complexity_markers = [" and ", " then ", " but ", " after ", " before ", 
                              "filter", "sort", "group", "ask me", "prompt"];
    
    action_starters.iter().any(|s| lower.starts_with(s)) &&
    !complexity_markers.iter().any(|m| lower.contains(m))
}

/// Try to extract a simple API goal
fn try_simple_api_goal(goal: &str, params: &HashMap<String, String>) -> Option<SubIntent> {
    if !is_simple_api_goal(goal) {
        return None;
    }
    
    let domain = DomainHint::infer_from_text(goal);
    let action = infer_api_action(goal);
    
    let mut intent = SubIntent::new(goal, IntentType::ApiCall { action })
        .with_domain(domain.unwrap_or(DomainHint::Generic));
    
    for (k, v) in params {
        intent.extracted_params.insert(k.clone(), v.clone());
    }
    
    Some(intent)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_action_with_user_input_pattern() {
        let strategy = PatternDecomposition::new();
        let context = DecompositionContext::new();
        
        let result = strategy
            .decompose(
                "list issues in mandubian/ccos but ask me for the page size",
                None,
                &context,
            )
            .await
            .expect("Should decompose");
        
        assert_eq!(result.sub_intents.len(), 2);
        
        // First intent should be user input
        assert!(matches!(result.sub_intents[0].intent_type, IntentType::UserInput { .. }));
        assert!(result.sub_intents[0].description.contains("page size"));
        
        // Second intent should be API call with dependency on first
        assert!(matches!(result.sub_intents[1].intent_type, IntentType::ApiCall { .. }));
        assert_eq!(result.sub_intents[1].dependencies, vec![0]);
        
        // Should extract owner/repo
        assert_eq!(result.sub_intents[1].extracted_params.get("owner"), Some(&"mandubian".to_string()));
        assert_eq!(result.sub_intents[1].extracted_params.get("repo"), Some(&"ccos".to_string()));
    }
    
    #[tokio::test]
    async fn test_simple_api_goal() {
        let strategy = PatternDecomposition::new();
        let context = DecompositionContext::new();
        
        let result = strategy
            .decompose("list issues in mandubian/ccos", None, &context)
            .await
            .expect("Should decompose");
        
        assert_eq!(result.sub_intents.len(), 1);
        assert!(matches!(
            result.sub_intents[0].intent_type,
            IntentType::ApiCall { action: ApiAction::List }
        ));
        assert_eq!(result.sub_intents[0].domain_hint, Some(DomainHint::GitHub));
    }
    
    #[tokio::test]
    async fn test_action_with_transform_pattern() {
        let strategy = PatternDecomposition::new();
        let context = DecompositionContext::new();
        
        let result = strategy
            .decompose("list issues and filter by label bug", None, &context)
            .await
            .expect("Should decompose");
        
        assert_eq!(result.sub_intents.len(), 2);
        
        // First is API call
        assert!(matches!(result.sub_intents[0].intent_type, IntentType::ApiCall { .. }));
        
        // Second is transform
        assert!(matches!(
            result.sub_intents[1].intent_type,
            IntentType::DataTransform { transform: TransformType::Filter }
        ));
        assert_eq!(result.sub_intents[1].dependencies, vec![0]);
    }
    
    #[test]
    fn test_can_handle() {
        let strategy = PatternDecomposition::new();
        
        // Should handle these
        assert!(strategy.can_handle("list issues but ask me for page size") > 0.5);
        assert!(strategy.can_handle("list issues in mandubian/ccos") > 0.5);
        
        // May not handle complex goals
        assert!(strategy.can_handle("do something very complicated with many steps") < 0.5);
    }
    
    #[test]
    fn test_extract_common_params() {
        let params = PatternDecomposition::extract_common_params("list issues in mandubian/ccos");
        assert_eq!(params.get("owner"), Some(&"mandubian".to_string()));
        assert_eq!(params.get("repo"), Some(&"ccos".to_string()));
        
        let params2 = PatternDecomposition::extract_common_params("user mandubian repository ccos");
        assert_eq!(params2.get("owner"), Some(&"mandubian".to_string()));
        assert_eq!(params2.get("repo"), Some(&"ccos".to_string()));
    }
}
