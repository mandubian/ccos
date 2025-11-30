//! Plan Verification Module
//!
//! Implements the "Ask, Don't Guess" principle by verifying generated plans
//! before execution. Uses an Arbiter/Judge LLM to check for consistency issues.

use std::sync::Arc;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::types::SubIntent;
use super::resolution::ResolvedCapability;

// ============================================================================
// Verification Result Types
// ============================================================================

/// Result of plan verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Overall verdict
    pub verdict: VerificationVerdict,
    /// List of issues found
    pub issues: Vec<VerificationIssue>,
    /// Suggestions for improvement
    pub suggestions: Vec<String>,
    /// Confidence in the verification (0.0-1.0)
    pub confidence: f64,
}

/// Verification verdict
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationVerdict {
    /// Plan is valid and ready for execution
    Valid,
    /// Plan has warnings but can proceed (user should be informed)
    ValidWithWarnings,
    /// Plan has issues that should be addressed before execution
    NeedsReview,
    /// Plan is invalid and should be re-generated
    Invalid,
}

/// A specific issue found during verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationIssue {
    /// Severity of the issue
    pub severity: IssueSeverity,
    /// Category of the issue
    pub category: IssueCategory,
    /// Human-readable description
    pub description: String,
    /// Which step(s) are affected
    pub affected_steps: Vec<usize>,
    /// Suggested fix (if any)
    pub suggested_fix: Option<String>,
}

/// Issue severity levels
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IssueSeverity {
    /// Informational - plan is fine but could be improved
    Info,
    /// Warning - plan works but may not match user intent
    Warning,
    /// Error - plan has a bug or inconsistency
    Error,
    /// Critical - plan should not be executed
    Critical,
}

/// Categories of verification issues
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IssueCategory {
    /// Data flow issue (dependencies not wired correctly)
    DataFlow,
    /// Goal coverage issue (plan doesn't address all parts of goal)
    GoalCoverage,
    /// Semantic clarity issue (prompts or steps are unclear)
    SemanticClarity,
    /// Type mismatch (e.g., string passed where number expected)
    TypeMismatch,
    /// Missing capability (required tool not found)
    MissingCapability,
    /// Security concern
    Security,
    /// Other issue
    Other,
}

// ============================================================================
// Verification Trait
// ============================================================================

/// Trait for plan verifiers
#[async_trait(?Send)]
pub trait PlanVerifier: Send + Sync {
    /// Verify a generated plan
    async fn verify(
        &self,
        goal: &str,
        sub_intents: &[SubIntent],
        resolutions: &std::collections::HashMap<String, ResolvedCapability>,
        rtfs_plan: &str,
    ) -> Result<VerificationResult, VerificationError>;
    
    /// Name of this verifier
    fn name(&self) -> &str;
}

/// Verification errors
#[derive(Debug, Clone)]
pub enum VerificationError {
    /// LLM provider error
    LlmError(String),
    /// Parse error in LLM response
    ParseError(String),
    /// Other error
    Other(String),
}

impl std::fmt::Display for VerificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerificationError::LlmError(s) => write!(f, "LLM error: {}", s),
            VerificationError::ParseError(s) => write!(f, "Parse error: {}", s),
            VerificationError::Other(s) => write!(f, "Error: {}", s),
        }
    }
}

impl std::error::Error for VerificationError {}

// ============================================================================
// Rule-Based Verifier (Fast, No LLM)
// ============================================================================

/// A fast, rule-based verifier that checks for common issues
/// without requiring an LLM. Good for catching obvious problems.
pub struct RuleBasedVerifier {
    /// Whether to check data flow
    check_data_flow: bool,
    /// Whether to check goal coverage
    check_goal_coverage: bool,
    /// Whether to check semantic clarity
    check_semantic_clarity: bool,
}

impl RuleBasedVerifier {
    pub fn new() -> Self {
        Self {
            check_data_flow: true,
            check_goal_coverage: true,
            check_semantic_clarity: true,
        }
    }
    
    /// Check data flow issues
    fn check_data_flow_issues(
        &self,
        sub_intents: &[SubIntent],
        rtfs_plan: &str,
    ) -> Vec<VerificationIssue> {
        let mut issues = Vec::new();
        
        for (idx, intent) in sub_intents.iter().enumerate() {
            // Check if this intent has dependencies
            if !intent.dependencies.is_empty() {
                // Check if dependencies are referenced in the plan
                for &dep_idx in &intent.dependencies {
                    let dep_var = format!("step_{}", dep_idx + 1);
                    
                    // Simple check: is the dependency variable mentioned after its definition?
                    // This is a heuristic - the plan might use it in a different form
                    if !rtfs_plan.contains(&dep_var) {
                        issues.push(VerificationIssue {
                            severity: IssueSeverity::Warning,
                            category: IssueCategory::DataFlow,
                            description: format!(
                                "Step {} depends on step {} but {} may not be used in the plan",
                                idx + 1, dep_idx + 1, dep_var
                            ),
                            affected_steps: vec![idx, dep_idx],
                            suggested_fix: Some(format!(
                                "Ensure step {} output is passed to step {}",
                                dep_idx + 1, idx + 1
                            )),
                        });
                    }
                }
            }
        }
        
        issues
    }
    
    /// Check goal coverage issues
    fn check_goal_coverage_issues(
        &self,
        goal: &str,
        sub_intents: &[SubIntent],
    ) -> Vec<VerificationIssue> {
        let mut issues = Vec::new();
        let goal_lower = goal.to_lowercase();
        
        // Check for common goal keywords that should be reflected in intents
        let goal_keywords = extract_goal_keywords(&goal_lower);
        let intent_descriptions: String = sub_intents.iter()
            .map(|i| i.description.to_lowercase())
            .collect::<Vec<_>>()
            .join(" ");
        
        for keyword in goal_keywords {
            if !intent_descriptions.contains(&keyword) {
                issues.push(VerificationIssue {
                    severity: IssueSeverity::Info,
                    category: IssueCategory::GoalCoverage,
                    description: format!(
                        "Goal mentions '{}' but no step explicitly addresses it",
                        keyword
                    ),
                    affected_steps: vec![],
                    suggested_fix: Some(format!(
                        "Consider adding a step that handles '{}'",
                        keyword
                    )),
                });
            }
        }
        
        issues
    }
    
    /// Check semantic clarity issues
    fn check_semantic_clarity_issues(
        &self,
        sub_intents: &[SubIntent],
    ) -> Vec<VerificationIssue> {
        use super::types::IntentType;
        
        let mut issues = Vec::new();
        
        for (idx, intent) in sub_intents.iter().enumerate() {
            // Check UserInput prompts for clarity
            if let IntentType::UserInput { prompt_topic } = &intent.intent_type {
                // Check for cryptic single-word prompts
                if prompt_topic.split_whitespace().count() == 1 {
                    let topic_lower = prompt_topic.to_lowercase();
                    
                    // Common cryptic prompts that need explanation
                    let needs_explanation = matches!(
                        topic_lower.as_str(),
                        "state" | "labels" | "assignee" | "milestone" | 
                        "since" | "direction" | "sort" | "perpage" | "per_page"
                    );
                    
                    if needs_explanation {
                        issues.push(VerificationIssue {
                            severity: IssueSeverity::Warning,
                            category: IssueCategory::SemanticClarity,
                            description: format!(
                                "Step {} asks user for '{}' without explaining valid values",
                                idx + 1, prompt_topic
                            ),
                            affected_steps: vec![idx],
                            suggested_fix: Some(suggest_prompt_improvement(prompt_topic)),
                        });
                    }
                }
            }
        }
        
        issues
    }
}

impl Default for RuleBasedVerifier {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PlanVerifier for RuleBasedVerifier {
    fn name(&self) -> &str {
        "rule_based"
    }
    
    async fn verify(
        &self,
        goal: &str,
        sub_intents: &[SubIntent],
        _resolutions: &std::collections::HashMap<String, ResolvedCapability>,
        rtfs_plan: &str,
    ) -> Result<VerificationResult, VerificationError> {
        let mut issues = Vec::new();
        
        if self.check_data_flow {
            issues.extend(self.check_data_flow_issues(sub_intents, rtfs_plan));
        }
        
        if self.check_goal_coverage {
            issues.extend(self.check_goal_coverage_issues(goal, sub_intents));
        }
        
        if self.check_semantic_clarity {
            issues.extend(self.check_semantic_clarity_issues(sub_intents));
        }
        
        // Determine verdict based on issues
        let verdict = if issues.iter().any(|i| i.severity == IssueSeverity::Critical) {
            VerificationVerdict::Invalid
        } else if issues.iter().any(|i| i.severity == IssueSeverity::Error) {
            VerificationVerdict::NeedsReview
        } else if issues.iter().any(|i| i.severity == IssueSeverity::Warning) {
            VerificationVerdict::ValidWithWarnings
        } else {
            VerificationVerdict::Valid
        };
        
        // Generate suggestions from issues
        let suggestions: Vec<String> = issues.iter()
            .filter_map(|i| i.suggested_fix.clone())
            .collect();
        
        Ok(VerificationResult {
            verdict,
            issues,
            suggestions,
            confidence: 0.7, // Rule-based verification has moderate confidence
        })
    }
}

// ============================================================================
// LLM-Based Verifier (Thorough, Requires LLM)
// ============================================================================

/// LLM provider trait for verification (reuse from decomposition if available)
#[async_trait(?Send)]
pub trait VerificationLlmProvider: Send + Sync {
    async fn generate(&self, prompt: &str) -> Result<String, String>;
}

/// An LLM-based verifier that uses an Arbiter/Judge model
/// to thoroughly analyze the plan for issues.
pub struct LlmVerifier {
    llm_provider: Arc<dyn VerificationLlmProvider>,
}

impl LlmVerifier {
    pub fn new(llm_provider: Arc<dyn VerificationLlmProvider>) -> Self {
        Self { llm_provider }
    }
    
    fn build_verification_prompt(
        &self,
        goal: &str,
        sub_intents: &[SubIntent],
        rtfs_plan: &str,
    ) -> String {
        let steps_description = sub_intents.iter()
            .enumerate()
            .map(|(idx, intent)| format!(
                "Step {}: {} (type: {:?}, depends on: {:?})",
                idx + 1,
                intent.description,
                intent.intent_type,
                intent.dependencies.iter().map(|d| d + 1).collect::<Vec<_>>()
            ))
            .collect::<Vec<_>>()
            .join("\n");
        
        format!(r#"You are a plan verification expert. Analyze the following plan and identify any issues.

ORIGINAL GOAL:
"{goal}"

DECOMPOSED STEPS:
{steps_description}

GENERATED RTFS PLAN:
```
{rtfs_plan}
```

Check for these issues:
1. DATA FLOW: Are all step dependencies correctly wired? Is each input properly passed?
2. GOAL COVERAGE: Does the plan fully address the original goal?
3. SEMANTIC CLARITY: Are user prompts clear? Will users understand what's being asked?
4. TYPE CORRECTNESS: Are types correctly handled (e.g., numbers vs strings)?

Respond with a JSON object:
{{
  "verdict": "valid" | "valid_with_warnings" | "needs_review" | "invalid",
  "issues": [
    {{
      "severity": "info" | "warning" | "error" | "critical",
      "category": "data_flow" | "goal_coverage" | "semantic_clarity" | "type_mismatch" | "other",
      "description": "Description of the issue",
      "affected_steps": [1, 2],
      "suggested_fix": "How to fix it"
    }}
  ],
  "summary": "Brief summary of verification result"
}}"#, goal = goal, steps_description = steps_description, rtfs_plan = rtfs_plan)
    }
}

#[async_trait(?Send)]
impl PlanVerifier for LlmVerifier {
    fn name(&self) -> &str {
        "llm_arbiter"
    }
    
    async fn verify(
        &self,
        goal: &str,
        sub_intents: &[SubIntent],
        _resolutions: &std::collections::HashMap<String, ResolvedCapability>,
        rtfs_plan: &str,
    ) -> Result<VerificationResult, VerificationError> {
        let prompt = self.build_verification_prompt(goal, sub_intents, rtfs_plan);
        
        let response = self.llm_provider.generate(&prompt).await
            .map_err(|e| VerificationError::LlmError(e))?;
        
        // Parse the JSON response
        parse_verification_response(&response)
    }
}

// ============================================================================
// Composite Verifier
// ============================================================================

/// Combines multiple verifiers, running rule-based first (fast)
/// then optionally LLM-based for deeper analysis.
pub struct CompositeVerifier {
    rule_based: RuleBasedVerifier,
    llm_verifier: Option<LlmVerifier>,
    /// Only run LLM verification if rule-based finds issues
    llm_on_issues_only: bool,
}

impl CompositeVerifier {
    pub fn new() -> Self {
        Self {
            rule_based: RuleBasedVerifier::new(),
            llm_verifier: None,
            llm_on_issues_only: true,
        }
    }
    
    pub fn with_llm(mut self, llm_provider: Arc<dyn VerificationLlmProvider>) -> Self {
        self.llm_verifier = Some(LlmVerifier::new(llm_provider));
        self
    }
    
    pub fn always_use_llm(mut self) -> Self {
        self.llm_on_issues_only = false;
        self
    }
}

impl Default for CompositeVerifier {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PlanVerifier for CompositeVerifier {
    fn name(&self) -> &str {
        "composite"
    }
    
    async fn verify(
        &self,
        goal: &str,
        sub_intents: &[SubIntent],
        resolutions: &std::collections::HashMap<String, ResolvedCapability>,
        rtfs_plan: &str,
    ) -> Result<VerificationResult, VerificationError> {
        // First, run rule-based verification (fast)
        let rule_result = self.rule_based.verify(goal, sub_intents, resolutions, rtfs_plan).await?;
        
        // If we have an LLM verifier and should use it
        let should_use_llm = self.llm_verifier.is_some() && 
            (!self.llm_on_issues_only || !rule_result.issues.is_empty());
        
        if should_use_llm {
            if let Some(ref llm_verifier) = self.llm_verifier {
                let llm_result = llm_verifier.verify(goal, sub_intents, resolutions, rtfs_plan).await?;
                
                // Merge results (LLM result takes precedence for verdict)
                return Ok(VerificationResult {
                    verdict: llm_result.verdict,
                    issues: merge_issues(rule_result.issues, llm_result.issues),
                    suggestions: merge_suggestions(rule_result.suggestions, llm_result.suggestions),
                    confidence: llm_result.confidence,
                });
            }
        }
        
        Ok(rule_result)
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Extract important keywords from a goal
fn extract_goal_keywords(goal: &str) -> Vec<String> {
    let stop_words = [
        "the", "a", "an", "in", "on", "at", "to", "for", "and", "or", "but",
        "with", "by", "from", "as", "is", "are", "was", "were", "be", "been",
        "it", "its", "my", "me", "i", "you", "your", "them", "them", "their",
    ];
    
    goal.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 3 && !stop_words.contains(&w.to_lowercase().as_str()))
        .map(|w| w.to_lowercase())
        .collect()
}

/// Suggest improvement for a cryptic prompt
fn suggest_prompt_improvement(topic: &str) -> String {
    match topic.to_lowercase().as_str() {
        "state" => "Ask for 'issue state (open, closed, or all)'".to_string(),
        "labels" => "Ask for 'labels to filter by (comma-separated, e.g., bug, enhancement)'".to_string(),
        "assignee" => "Ask for 'assignee username (or leave blank for all)'".to_string(),
        "perpage" | "per_page" => "Ask for 'number of items per page (e.g., 10, 25, 50)'".to_string(),
        "since" => "Ask for 'show issues since date (YYYY-MM-DD format)'".to_string(),
        "sort" => "Ask for 'sort by (created, updated, comments)'".to_string(),
        "direction" => "Ask for 'sort direction (asc or desc)'".to_string(),
        _ => format!("Provide more context for '{}'", topic),
    }
}

/// Parse LLM verification response
fn parse_verification_response(response: &str) -> Result<VerificationResult, VerificationError> {
    // Extract JSON from response
    let json_str = if let Some(start) = response.find('{') {
        if let Some(end) = response.rfind('}') {
            &response[start..=end]
        } else {
            response
        }
    } else {
        response
    };
    
    // Parse JSON
    let parsed: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| VerificationError::ParseError(format!("JSON parse error: {}", e)))?;
    
    let verdict = match parsed.get("verdict").and_then(|v| v.as_str()) {
        Some("valid") => VerificationVerdict::Valid,
        Some("valid_with_warnings") => VerificationVerdict::ValidWithWarnings,
        Some("needs_review") => VerificationVerdict::NeedsReview,
        Some("invalid") => VerificationVerdict::Invalid,
        _ => VerificationVerdict::ValidWithWarnings, // Default to warnings if unclear
    };
    
    let issues = parsed.get("issues")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(parse_issue).collect())
        .unwrap_or_default();
    
    let suggestions = parsed.get("summary")
        .and_then(|v| v.as_str())
        .map(|s| vec![s.to_string()])
        .unwrap_or_default();
    
    Ok(VerificationResult {
        verdict,
        issues,
        suggestions,
        confidence: 0.85,
    })
}

/// Parse a single issue from JSON
fn parse_issue(value: &serde_json::Value) -> Option<VerificationIssue> {
    let severity = match value.get("severity")?.as_str()? {
        "info" => IssueSeverity::Info,
        "warning" => IssueSeverity::Warning,
        "error" => IssueSeverity::Error,
        "critical" => IssueSeverity::Critical,
        _ => IssueSeverity::Info,
    };
    
    let category = match value.get("category")?.as_str()? {
        "data_flow" => IssueCategory::DataFlow,
        "goal_coverage" => IssueCategory::GoalCoverage,
        "semantic_clarity" => IssueCategory::SemanticClarity,
        "type_mismatch" => IssueCategory::TypeMismatch,
        _ => IssueCategory::Other,
    };
    
    Some(VerificationIssue {
        severity,
        category,
        description: value.get("description")?.as_str()?.to_string(),
        affected_steps: value.get("affected_steps")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_u64().map(|n| n as usize)).collect())
            .unwrap_or_default(),
        suggested_fix: value.get("suggested_fix").and_then(|v| v.as_str()).map(String::from),
    })
}

/// Merge issues from multiple verifiers
fn merge_issues(mut rule_issues: Vec<VerificationIssue>, llm_issues: Vec<VerificationIssue>) -> Vec<VerificationIssue> {
    // Add LLM issues that aren't duplicates
    for llm_issue in llm_issues {
        let is_duplicate = rule_issues.iter().any(|ri| 
            ri.category == llm_issue.category && 
            ri.affected_steps == llm_issue.affected_steps
        );
        if !is_duplicate {
            rule_issues.push(llm_issue);
        }
    }
    rule_issues
}

/// Merge suggestions from multiple verifiers
fn merge_suggestions(mut rule_suggestions: Vec<String>, llm_suggestions: Vec<String>) -> Vec<String> {
    for suggestion in llm_suggestions {
        if !rule_suggestions.contains(&suggestion) {
            rule_suggestions.push(suggestion);
        }
    }
    rule_suggestions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::modular_planner::types::IntentType;
    
    #[tokio::test]
    async fn test_rule_based_verifier_data_flow() {
        let verifier = RuleBasedVerifier::new();
        
        let mut intent1 = SubIntent::new("Ask for page size", IntentType::UserInput { 
            prompt_topic: "perPage".to_string() 
        });
        
        let mut intent2 = SubIntent::new("List issues", IntentType::ApiCall { 
            action: crate::planner::modular_planner::types::ApiAction::List 
        });
        intent2.dependencies = vec![0]; // Depends on intent1
        
        let sub_intents = vec![intent1, intent2];
        let resolutions = std::collections::HashMap::new();
        
        // Plan that doesn't use step_1
        let bad_plan = r#"(let [step_1 ...] (let [step_2 (call "list_issues" {})] step_2))"#;
        
        let result = verifier.verify("list issues", &sub_intents, &resolutions, bad_plan).await.unwrap();
        
        assert!(!result.issues.is_empty());
        assert!(result.issues.iter().any(|i| i.category == IssueCategory::DataFlow));
    }
    
    #[tokio::test]
    async fn test_rule_based_verifier_semantic_clarity() {
        let verifier = RuleBasedVerifier::new();
        
        let intent = SubIntent::new("Ask for state", IntentType::UserInput { 
            prompt_topic: "state".to_string() 
        });
        
        let sub_intents = vec![intent];
        let resolutions = std::collections::HashMap::new();
        let plan = r#"(call "ccos.user.ask" {:prompt "Please provide: state"})"#;
        
        let result = verifier.verify("filter by state", &sub_intents, &resolutions, plan).await.unwrap();
        
        assert!(result.issues.iter().any(|i| i.category == IssueCategory::SemanticClarity));
    }
}

