/// Learning Capabilities
///
/// Exposes CausalChain query and analysis functions as RTFS-callable capabilities.
use crate::capability_marketplace::CapabilityMarketplace;
use crate::causal_chain::CausalQuery;
use crate::types::ActionType;
use futures::future::BoxFuture;
use futures::FutureExt;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Failure summary for learning analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureSummary {
    pub capability_id: String,
    pub error_message: String,
    pub error_category: String,
    pub timestamp: u64,
    pub plan_id: Option<String>,
    pub intent_id: Option<String>,
}

/// Input for learning.get_failures
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GetFailuresInput {
    #[serde(default)]
    pub capability_id: Option<String>,
    #[serde(default)]
    pub error_category: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Output for learning.get_failures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetFailuresOutput {
    pub failures: Vec<FailureSummary>,
    pub total_count: usize,
}

/// Input for learning.analyze_failure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnalyzeFailureInput {
    #[serde(default)]
    pub error_message: String,
    #[serde(default)]
    pub capability_id: Option<String>,
}

/// Output for learning.analyze_failure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzeFailureOutput {
    pub error_category: String,
    pub suggested_fix: Option<String>,
    pub similar_failures_count: usize,
}

/// Input for learning.get_failure_stats
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GetFailureStatsInput {
    #[serde(default)]
    pub capability_id: Option<String>,
}

/// Output for learning.get_failure_stats
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetFailureStatsOutput {
    pub total_failures: usize,
    pub by_category: HashMap<String, usize>,
    pub top_failing_capabilities: Vec<(String, usize)>,
}

/// Input for learning.suggest_improvement (LLM-assisted)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SuggestImprovementInput {
    /// Specific capability to analyze, or None to analyze top failures
    #[serde(default)]
    pub capability_id: Option<String>,
    /// Maximum number of failures to analyze
    #[serde(default)]
    pub max_failures: Option<usize>,
    /// Include detailed analysis in response
    #[serde(default)]
    pub detailed: Option<bool>,
}

/// Output for learning.suggest_improvement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestImprovementOutput {
    pub analyzed_failures: usize,
    pub suggestions: Vec<ImprovementSuggestion>,
    pub llm_used: bool,
}

/// A single improvement suggestion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImprovementSuggestion {
    pub capability_id: String,
    pub issue_summary: String,
    pub suggested_action: String,
    pub priority: String, // "high", "medium", "low"
    pub rationale: Option<String>,
}

/// Input for learning.extract_patterns
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExtractPatternsInput {
    /// Look back period in hours (default: 24)
    #[serde(default)]
    pub time_window_hours: Option<u64>,
    /// Minimum occurrences to form a cluster (default: 2)
    #[serde(default)]
    pub min_occurrences: Option<usize>,
    /// Auto-store patterns in WorkingMemory (default: true)
    #[serde(default)]
    pub store_in_memory: Option<bool>,
}

/// Output for learning.extract_patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractPatternsOutput {
    pub patterns_extracted: usize,
    pub patterns: Vec<ExtractedPattern>,
}

/// A pattern extracted from failure clusters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedPattern {
    pub pattern_id: String,
    pub description: String,
    pub confidence: f64,
    pub affected_capabilities: Vec<String>,
    pub error_category: String,
    pub occurrence_count: usize,
    pub suggested_action: Option<String>,
}

/// Input for learning.recall_for_capability
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RecallForCapabilityInput {
    pub capability_id: String,
}

/// Output for learning.recall_for_capability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallForCapabilityOutput {
    pub patterns: Vec<ExtractedPattern>,
    pub suggested_plan_modifications: Vec<String>,
}

/// Input for learning.apply_fix - Automatic remediation based on error patterns
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApplyFixInput {
    /// The capability ID that failed (required)
    pub capability_id: String,
    /// The error category (optional - will be looked up from patterns)
    pub error_category: Option<String>,
    /// Specific action to take: "auto", "retry", "synthesize", "alternative", "adjust_timeout"
    pub action: Option<String>,
    /// Optional: maximum retries for retry action
    pub max_retries: Option<u32>,
    /// Optional: timeout adjustment multiplier (e.g., 2.0 = double timeout)
    pub timeout_multiplier: Option<f64>,
}

/// Output for learning.apply_fix
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyFixOutput {
    /// Whether the fix was applied successfully
    pub success: bool,
    /// The remediation action that was taken
    pub remediation_action: String,
    /// Description of what was done
    pub description: String,
    /// If this triggered another capability call, its ID
    pub triggered_capability: Option<String>,
    /// Result of the triggered capability (if any)
    pub triggered_result: Option<serde_json::Value>,
    /// Suggestions for the Arbiter to modify plans
    pub plan_modifications: Vec<PlanModification>,
}

/// A suggested modification to an execution plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanModification {
    /// Type of modification: "inject_retry", "inject_fallback", "adjust_timeout", "skip_capability", "synthesize_first"
    pub modification_type: String,
    /// The capability this applies to
    pub target_capability: String,
    /// Additional parameters (e.g., retry count, fallback capability ID)
    pub parameters: serde_json::Value,
    /// Confidence that this modification will help (0.0 - 1.0)
    pub confidence: f64,
}

/// Register learning capabilities with the marketplace
pub async fn register_learning_capabilities(
    marketplace: Arc<CapabilityMarketplace>,
    causal_chain: Arc<Mutex<crate::causal_chain::CausalChain>>,
) -> Result<(), RuntimeError> {
    let chain_clone = causal_chain.clone();

    // learning.get_failures - Query failures from causal chain
    marketplace
        .register_native_capability(
            "learning.get_failures".to_string(),
            "Get Failures".to_string(),
            "Query execution failures from the causal chain, optionally filtered by capability or error category".to_string(),
            Arc::new({
                let chain = chain_clone.clone();
                move |args: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
                    let args_clone = args.clone();
                    let chain = chain.clone();
                    async move {
                        let input: GetFailuresInput = parse_input(&args_clone)?;
                        let guard = chain.lock().map_err(|_| {
                            RuntimeError::Generic("Failed to lock causal chain".to_string())
                        })?;

                        // Query for CapabilityResult actions
                        let query = CausalQuery {
                            action_type: Some(ActionType::CapabilityResult),
                            ..Default::default()
                        };
                        let actions = guard.query_actions(&query);

                        // Filter for failures and extract data
                        let mut failures: Vec<FailureSummary> = actions
                            .iter()
                            .filter_map(|action| {
                                let result = action.result.as_ref()?;
                                if result.success {
                                    return None;
                                }

                                let capability_id =
                                    action.function_name.clone().unwrap_or_default();
                                let error_message = result
                                    .metadata
                                    .get("error")
                                    .and_then(|v| {
                                        if let Value::String(s) = v {
                                            Some(s.clone())
                                        } else {
                                            None
                                        }
                                    })
                                    .unwrap_or_default();
                                let error_category = result
                                    .metadata
                                    .get("error_category")
                                    .and_then(|v| {
                                        if let Value::String(s) = v {
                                            Some(s.clone())
                                        } else {
                                            None
                                        }
                                    })
                                    .unwrap_or_else(|| "Unknown".to_string());

                                // Apply filters
                                if let Some(ref filter_cap) = input.capability_id {
                                    if !capability_id.contains(filter_cap) {
                                        return None;
                                    }
                                }
                                if let Some(ref filter_cat) = input.error_category {
                                    if &error_category != filter_cat {
                                        return None;
                                    }
                                }

                                Some(FailureSummary {
                                    capability_id,
                                    error_message,
                                    error_category,
                                    timestamp: action.timestamp,
                                    plan_id: action.plan_id.clone(),
                                    intent_id: action.intent_id.clone(),
                                })
                            })
                            .collect();

                        // Sort by timestamp descending (most recent first)
                        failures.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

                        // Apply limit
                        let limit = input.limit.unwrap_or(100);
                        let total_count = failures.len();
                        failures.truncate(limit);

                        let output = GetFailuresOutput {
                            failures,
                            total_count,
                        };

                        to_value(&output)
                    }
                    .boxed()
                }
            }),
            "low".to_string(),
        )
        .await?;

    // learning.get_failure_stats - Get aggregated failure statistics
    marketplace
        .register_native_capability(
            "learning.get_failure_stats".to_string(),
            "Get Failure Stats".to_string(),
            "Get aggregated statistics about failures by category and capability".to_string(),
            Arc::new({
                let chain = chain_clone.clone();
                move |args: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
                    let args_clone = args.clone();
                    let chain = chain.clone();
                    async move {
                        let input: GetFailureStatsInput = parse_input(&args_clone)?;
                        let guard = chain.lock().map_err(|_| {
                            RuntimeError::Generic("Failed to lock causal chain".to_string())
                        })?;

                        // Query for CapabilityResult actions
                        let query = CausalQuery {
                            action_type: Some(ActionType::CapabilityResult),
                            ..Default::default()
                        };
                        let actions = guard.query_actions(&query);

                        let mut by_category: HashMap<String, usize> = HashMap::new();
                        let mut by_capability: HashMap<String, usize> = HashMap::new();
                        let mut total_failures = 0;

                        for action in actions {
                            let result = match action.result.as_ref() {
                                Some(r) if !r.success => r,
                                _ => continue,
                            };

                            let capability_id = action.function_name.clone().unwrap_or_default();

                            // Apply capability filter if specified
                            if let Some(ref filter_cap) = input.capability_id {
                                if !capability_id.contains(filter_cap) {
                                    continue;
                                }
                            }

                            let error_category = result
                                .metadata
                                .get("error_category")
                                .and_then(|v| {
                                    if let Value::String(s) = v {
                                        Some(s.clone())
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or_else(|| "Unknown".to_string());

                            total_failures += 1;
                            *by_category.entry(error_category).or_insert(0) += 1;
                            *by_capability.entry(capability_id).or_insert(0) += 1;
                        }

                        // Sort capabilities by failure count
                        let mut top_failing: Vec<(String, usize)> =
                            by_capability.into_iter().collect();
                        top_failing.sort_by(|a, b| b.1.cmp(&a.1));
                        top_failing.truncate(10);

                        let output = GetFailureStatsOutput {
                            total_failures,
                            by_category,
                            top_failing_capabilities: top_failing,
                        };

                        to_value(&output)
                    }
                    .boxed()
                }
            }),
            "low".to_string(),
        )
        .await?;

    // learning.analyze_failure - Analyze a specific failure
    marketplace
        .register_native_capability(
            "learning.analyze_failure".to_string(),
            "Analyze Failure".to_string(),
            "Analyze an error message and suggest potential fixes".to_string(),
            Arc::new({
                let chain = chain_clone.clone();
                move |args: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
                    let args_clone = args.clone();
                    let chain = chain.clone();
                    async move {
                        let input: AnalyzeFailureInput = parse_input(&args_clone)?;
                        let msg = input.error_message.to_lowercase();

                        // Classify the error
                        let error_category = classify_error_message(&msg);

                        // Suggest fix based on category
                        let suggested_fix = suggest_fix(&error_category, &msg);

                        // Count similar failures
                        let similar_count = if let Ok(guard) = chain.lock() {
                            let query = CausalQuery {
                                action_type: Some(ActionType::CapabilityResult),
                                ..Default::default()
                            };
                            guard
                                .query_actions(&query)
                                .iter()
                                .filter(|a| {
                                    a.result
                                        .as_ref()
                                        .map(|r| {
                                            !r.success
                                                && r.metadata
                                                    .get("error_category")
                                                    .map(|v| {
                                                        if let Value::String(s) = v {
                                                            s == &error_category
                                                        } else {
                                                            false
                                                        }
                                                    })
                                                    .unwrap_or(false)
                                        })
                                        .unwrap_or(false)
                                })
                                .count()
                        } else {
                            0
                        };

                        let output = AnalyzeFailureOutput {
                            error_category,
                            suggested_fix,
                            similar_failures_count: similar_count,
                        };

                        to_value(&output)
                    }
                    .boxed()
                }
            }),
            "low".to_string(),
        )
        .await?;

    eprintln!("📚 Registered 3 learning capabilities");
    Ok(())
}

/// Register LLM-assisted learning capabilities (requires arbiter)
pub async fn register_llm_learning_capabilities(
    marketplace: Arc<CapabilityMarketplace>,
    causal_chain: Arc<Mutex<crate::causal_chain::CausalChain>>,
    arbiter: Arc<crate::cognitive_engine::DelegatingCognitiveEngine>,
) -> Result<(), RuntimeError> {
    let chain_clone = causal_chain.clone();
    let arbiter_clone = arbiter.clone();

    // learning.suggest_improvement - LLM-assisted failure analysis
    marketplace
        .register_native_capability(
            "learning.suggest_improvement".to_string(),
            "Suggest Improvement".to_string(),
            "Analyze failures using LLM and suggest improvements to capabilities".to_string(),
            Arc::new({
                let chain = chain_clone.clone();
                let arbiter = arbiter_clone.clone();
                move |args: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
                    let args_clone = args.clone();
                    let chain = chain.clone();
                    let arbiter = arbiter.clone();
                    async move {
                        let input: SuggestImprovementInput = parse_input(&args_clone)?;
                        let max_failures = input.max_failures.unwrap_or(5);
                        let detailed = input.detailed.unwrap_or(false);

                        // Get failures from causal chain
                        let failures = {
                            let guard = chain.lock().map_err(|_| {
                                RuntimeError::Generic("Failed to lock causal chain".to_string())
                            })?;

                            let query = CausalQuery {
                                action_type: Some(ActionType::CapabilityResult),
                                ..Default::default()
                            };
                            let actions = guard.query_actions(&query);

                            let mut failures: Vec<FailureSummary> = actions
                                .iter()
                                .filter_map(|action| {
                                    let result = action.result.as_ref()?;
                                    if result.success {
                                        return None;
                                    }

                                    let capability_id =
                                        action.function_name.clone().unwrap_or_default();

                                    // Apply capability filter
                                    if let Some(ref filter_cap) = input.capability_id {
                                        if !capability_id.contains(filter_cap) {
                                            return None;
                                        }
                                    }

                                    let error_message = result
                                        .metadata
                                        .get("error")
                                        .and_then(|v| {
                                            if let Value::String(s) = v {
                                                Some(s.clone())
                                            } else {
                                                None
                                            }
                                        })
                                        .unwrap_or_default();
                                    let error_category = result
                                        .metadata
                                        .get("error_category")
                                        .and_then(|v| {
                                            if let Value::String(s) = v {
                                                Some(s.clone())
                                            } else {
                                                None
                                            }
                                        })
                                        .unwrap_or_else(|| "Unknown".to_string());

                                    Some(FailureSummary {
                                        capability_id,
                                        error_message,
                                        error_category,
                                        timestamp: action.timestamp,
                                        plan_id: action.plan_id.clone(),
                                        intent_id: action.intent_id.clone(),
                                    })
                                })
                                .collect();

                            failures.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
                            failures.truncate(max_failures);
                            failures
                        };

                        if failures.is_empty() {
                            return to_value(&SuggestImprovementOutput {
                                analyzed_failures: 0,
                                suggestions: vec![],
                                llm_used: false,
                            });
                        }

                        // Build prompt for LLM analysis
                        let failures_json = serde_json::to_string_pretty(&failures)
                            .unwrap_or_else(|_| "[]".to_string());

                        let prompt = format!(
                            r#"Analyze these capability execution failures and suggest improvements.

Failures:
{}

For each unique failing capability, provide:
1. A brief summary of what's failing
2. A concrete suggested action to fix it
3. Priority level (high/medium/low)
{}

Output JSON array:
[
  {{
    "capability_id": "...",
    "issue_summary": "Brief description of the problem",
    "suggested_action": "Concrete fix suggestion",
    "priority": "high|medium|low"{}
  }}
]

Return ONLY the JSON array, no other text."#,
                            failures_json,
                            if detailed { "4. Detailed rationale for the fix" } else { "" },
                            if detailed { ",\n    \"rationale\": \"Detailed explanation\"" } else { "" }
                        );

                        // Call LLM
                        eprintln!("[learning.suggest_improvement] Calling LLM with {} failures", failures.len());
                        let llm_response = arbiter.generate_raw_text(&prompt).await;
                        let llm_used = llm_response.is_ok();

                        let suggestions = match llm_response {
                            Ok(response) => {
                                eprintln!("[learning.suggest_improvement] LLM response len: {}", response.len());
                                // Try to parse JSON from response
                                if let Some(json_str) = extract_json_array(&response) {
                                    serde_json::from_str::<Vec<ImprovementSuggestion>>(json_str)
                                        .unwrap_or_else(|e| {
                                            eprintln!("[learning.suggest_improvement] JSON parse error: {}", e);
                                            generate_fallback_suggestions(&failures)
                                        })
                                } else {
                                    eprintln!("[learning.suggest_improvement] No JSON array found, using fallback");
                                    generate_fallback_suggestions(&failures)
                                }
                            }
                            Err(e) => {
                                eprintln!("[learning.suggest_improvement] LLM error: {}, using fallback", e);
                                generate_fallback_suggestions(&failures)
                            }
                        };

                        let output = SuggestImprovementOutput {
                            analyzed_failures: failures.len(),
                            suggestions,
                            llm_used,
                        };

                        to_value(&output)
                    }
                    .boxed()
                }
            }),
            "medium".to_string(),
        )
        .await?;

    eprintln!("🧠 Registered 1 LLM-assisted learning capability");
    Ok(())
}

/// Extract JSON array from LLM response
fn extract_json_array(text: &str) -> Option<&str> {
    let start = text.find('[')?;
    let mut depth = 0;
    let bytes = text.as_bytes();

    for (i, &byte) in bytes[start..].iter().enumerate() {
        match byte {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..start + i + 1]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Generate fallback suggestions without LLM
fn generate_fallback_suggestions(failures: &[FailureSummary]) -> Vec<ImprovementSuggestion> {
    // Group by capability and create basic suggestions
    let mut seen_caps: std::collections::HashSet<String> = std::collections::HashSet::new();

    failures
        .iter()
        .filter_map(|f| {
            if seen_caps.contains(&f.capability_id) {
                return None;
            }
            seen_caps.insert(f.capability_id.clone());

            let (issue_summary, suggested_action, priority) = match f.error_category.as_str() {
                "SchemaError" => (
                    format!("Schema validation failed for {}", f.capability_id),
                    "Review and update the capability's input/output schema".to_string(),
                    "high".to_string(),
                ),
                "MissingCapability" => (
                    format!("Missing dependency for {}", f.capability_id),
                    "Synthesize or import the missing capability".to_string(),
                    "high".to_string(),
                ),
                "TimeoutError" => (
                    format!("Timeout executing {}", f.capability_id),
                    "Increase timeout or break into smaller operations".to_string(),
                    "medium".to_string(),
                ),
                "NetworkError" => (
                    format!("Network error in {}", f.capability_id),
                    "Add retry logic and check endpoint availability".to_string(),
                    "medium".to_string(),
                ),
                "LLMError" => (
                    format!("LLM generation failed in {}", f.capability_id),
                    "Verify API keys and add fallback model".to_string(),
                    "high".to_string(),
                ),
                _ => (
                    format!("Runtime error in {}", f.capability_id),
                    "Review error logs for detailed diagnostics".to_string(),
                    "low".to_string(),
                ),
            };

            Some(ImprovementSuggestion {
                capability_id: f.capability_id.clone(),
                issue_summary,
                suggested_action,
                priority,
                rationale: None,
            })
        })
        .collect()
}

/// Parse input from Value to typed struct
fn parse_input<T: for<'de> Deserialize<'de>>(args: &Value) -> RuntimeResult<T> {
    let json = crate::utils::value_conversion::rtfs_value_to_json(args)?;
    serde_json::from_value(json)
        .map_err(|e| RuntimeError::Generic(format!("Failed to parse input: {}", e)))
}

/// Convert output struct to Value
fn to_value<T: Serialize>(output: &T) -> RuntimeResult<Value> {
    let json = serde_json::to_value(output)
        .map_err(|e| RuntimeError::Generic(format!("Failed to serialize output: {}", e)))?;
    crate::utils::value_conversion::json_to_rtfs_value(&json)
}

/// Classify error message into a category (mirrors host.rs classify_error)
fn classify_error_message(msg: &str) -> String {
    if msg.contains("schema")
        || msg.contains("validation failed")
        || msg.contains("missing field")
        || msg.contains("type mismatch")
    {
        return "SchemaError".to_string();
    }
    if msg.contains("unknown capability")
        || msg.contains("not found")
        || msg.contains("missing capability")
    {
        return "MissingCapability".to_string();
    }
    if msg.contains("timeout") || msg.contains("timed out") {
        return "TimeoutError".to_string();
    }
    if msg.contains("network") || msg.contains("connection") || msg.contains("http") {
        return "NetworkError".to_string();
    }
    if msg.contains("llm") || msg.contains("generation failed") || msg.contains("synthesis") {
        return "LLMError".to_string();
    }
    "RuntimeError".to_string()
}

/// Suggest a fix based on error category
fn suggest_fix(category: &str, msg: &str) -> Option<String> {
    match category {
        "SchemaError" => {
            if msg.contains("missing field") {
                Some("Check that all required input fields are provided".to_string())
            } else if msg.contains("type mismatch") {
                Some("Verify input types match the expected schema".to_string())
            } else {
                Some("Review the capability's input/output schema".to_string())
            }
        }
        "MissingCapability" => {
            Some("The capability may need to be synthesized or imported. Try running planner.synthesize_capability".to_string())
        }
        "TimeoutError" => {
            Some("Consider increasing timeout limits or breaking the operation into smaller steps".to_string())
        }
        "NetworkError" => {
            Some("Check network connectivity and API endpoint availability".to_string())
        }
        "LLMError" => {
            Some("Verify LLM API keys and model availability. Consider fallback to alternative provider".to_string())
        }
        _ => None,
    }
}

/// Register pattern extraction capabilities (Phase 6: Autonomous Learning Loop)
pub async fn register_pattern_extraction_capabilities(
    marketplace: Arc<CapabilityMarketplace>,
    causal_chain: Arc<Mutex<crate::causal_chain::CausalChain>>,
    working_memory: Arc<Mutex<crate::working_memory::facade::WorkingMemory>>,
) -> Result<(), RuntimeError> {
    let chain_clone = causal_chain.clone();
    let wm_clone = working_memory.clone();

    // learning.extract_patterns - Scan CausalChain for failure clusters and store in WorkingMemory
    marketplace
        .register_native_capability(
            "learning.extract_patterns".to_string(),
            "Extract Patterns".to_string(),
            "Scan CausalChain for failure clusters, create LearnedPatterns, and store in WorkingMemory".to_string(),
            Arc::new({
                let chain = chain_clone.clone();
                let wm = wm_clone.clone();
                move |args: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
                    let args_clone = args.clone();
                    let chain = chain.clone();
                    let wm = wm.clone();
                    async move {
                        let input: ExtractPatternsInput = parse_input(&args_clone)?;
                        
                        let time_window = input.time_window_hours.unwrap_or(24);
                        let min_occurrences = input.min_occurrences.unwrap_or(2);
                        let store_in_memory = input.store_in_memory.unwrap_or(true);
                        
                        // Get current time and calculate cutoff
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64;
                        let cutoff = now.saturating_sub(time_window * 3600 * 1000);
                        
                        // Query failures from CausalChain
                        let guard = chain.lock().map_err(|_| {
                            RuntimeError::Generic("Failed to lock causal chain".to_string())
                        })?;
                        
                        let query = CausalQuery {
                            action_type: Some(ActionType::CapabilityResult),
                            ..Default::default()
                        };
                        let actions = guard.query_actions(&query);
                        
                        // Filter failures within time window and cluster by (capability_id, error_category)
                        let mut clusters: HashMap<(String, String), Vec<String>> = HashMap::new();
                        
                        for action in actions.iter() {
                            if let Some(result) = action.result.as_ref() {
                                if !result.success && action.timestamp >= cutoff {
                                    let capability_id = action.function_name.clone().unwrap_or_default();
                                    let error_category = result
                                        .metadata
                                        .get("error_category")
                                        .and_then(|v| match v {
                                            Value::String(s) => Some(s.clone()),
                                            _ => None,
                                        })
                                        .unwrap_or_else(|| "RuntimeError".to_string());
                                    
                                    let key = (capability_id, error_category);
                                    clusters.entry(key).or_default().push(action.action_id.clone());
                                }
                            }
                        }
                        drop(guard);
                        
                        // Extract patterns from clusters with >= min_occurrences
                        let mut patterns: Vec<ExtractedPattern> = Vec::new();
                        
                        for ((capability_id, error_category), failure_ids) in clusters {
                            if failure_ids.len() >= min_occurrences {
                                let occurrence_count = failure_ids.len();
                                let confidence = (occurrence_count as f64 / 10.0).min(1.0).max(0.3);
                                
                                let description = format!(
                                    "{} failures for {} (category: {})",
                                    occurrence_count,
                                    capability_id,
                                    error_category
                                );
                                
                                let suggested_action = suggest_fix(&error_category, "");
                                
                                let pattern = ExtractedPattern {
                                    pattern_id: format!("pattern:{}:{}", capability_id, error_category),
                                    description: description.clone(),
                                    confidence,
                                    affected_capabilities: vec![capability_id.clone()],
                                    error_category: error_category.clone(),
                                    occurrence_count,
                                    suggested_action,
                                };
                                
                                patterns.push(pattern);
                            }
                        }
                        
                        // Store patterns in WorkingMemory if requested
                        if store_in_memory && !patterns.is_empty() {
                            if let Ok(mut wm_guard) = wm.lock() {
                                for pattern in &patterns {
                                    let entry = crate::working_memory::types::WorkingMemoryEntry::new_with_estimate(
                                        pattern.pattern_id.clone(),
                                        format!("Learned Pattern: {}", pattern.pattern_id),
                                        serde_json::to_string(pattern).unwrap_or_default(),
                                        ["learned-pattern".to_string(), 
                                         format!("error:{}", pattern.error_category),
                                         format!("capability:{}", pattern.affected_capabilities.join(","))].into_iter(),
                                        std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_secs(),
                                        crate::working_memory::types::WorkingMemoryMeta {
                                            provider: Some("learning.extract_patterns".to_string()),
                                            ..Default::default()
                                        },
                                    );
                                    let _ = wm_guard.append(entry);
                                }
                            }
                        }
                        
                        let output = ExtractPatternsOutput {
                            patterns_extracted: patterns.len(),
                            patterns,
                        };
                        
                        to_value(&output)
                    }
                    .boxed()
                }
            }),
            "low".to_string(),
        )
        .await?;

    // learning.recall_for_capability - Pre-execution hook to get relevant patterns
    let wm_clone2 = working_memory.clone();
    marketplace
        .register_native_capability(
            "learning.recall_for_capability".to_string(),
            "Recall for Capability".to_string(),
            "Pre-execution hook: retrieve relevant learned patterns before running a capability"
                .to_string(),
            Arc::new({
                let wm = wm_clone2;
                move |args: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
                    let args_clone = args.clone();
                    let wm = wm.clone();
                    async move {
                        let input: RecallForCapabilityInput = parse_input(&args_clone)?;
                        let capability_id = input.capability_id;

                        let mut patterns: Vec<ExtractedPattern> = Vec::new();
                        let mut suggested_modifications: Vec<String> = Vec::new();

                        // Query WorkingMemory for patterns related to this capability
                        if let Ok(wm_guard) = wm.lock() {
                            let tag_set: std::collections::HashSet<String> =
                                ["learned-pattern".to_string()].into_iter().collect();

                            let params =
                                crate::working_memory::backend::QueryParams::with_tags(tag_set)
                                    .with_limit(Some(20));

                            if let Ok(result) = wm_guard.query(&params) {
                                for entry in result.entries {
                                    // Check if pattern relates to our capability
                                    if entry.tags.iter().any(|t| t.contains(&capability_id)) {
                                        if let Ok(pattern) =
                                            serde_json::from_str::<ExtractedPattern>(&entry.content)
                                        {
                                            // Generate suggested modifications based on pattern
                                            if pattern.occurrence_count >= 3 {
                                                match pattern.error_category.as_str() {
                                                    "TimeoutError" => {
                                                        suggested_modifications
                                                            .push("add_timeout_buffer".to_string());
                                                    }
                                                    "NetworkError" => {
                                                        suggested_modifications
                                                            .push("add_retry_step".to_string());
                                                    }
                                                    "SchemaError" => {
                                                        suggested_modifications.push(
                                                            "add_input_validation".to_string(),
                                                        );
                                                    }
                                                    "MissingCapability" => {
                                                        suggested_modifications
                                                            .push("trigger_synthesis".to_string());
                                                    }
                                                    _ => {}
                                                }
                                            }
                                            patterns.push(pattern);
                                        }
                                    }
                                }
                            }
                        }

                        let output = RecallForCapabilityOutput {
                            patterns,
                            suggested_plan_modifications: suggested_modifications,
                        };

                        to_value(&output)
                    }
                    .boxed()
                }
            }),
            "low".to_string(),
        )
        .await?;

    eprintln!("📚 Registered 2 pattern extraction capabilities");
    Ok(())
}

/// Register the learning.apply_fix capability for automatic remediation
pub async fn register_apply_fix_capability(
    marketplace: Arc<CapabilityMarketplace>,
    causal_chain: Arc<Mutex<crate::causal_chain::CausalChain>>,
    working_memory: Arc<Mutex<crate::working_memory::facade::WorkingMemory>>,
) -> Result<(), RuntimeError> {
    let chain_clone = causal_chain.clone();
    let wm_clone = working_memory.clone();
    let mp_clone = marketplace.clone();

    // learning.apply_fix - Automatic remediation based on error patterns
    marketplace
        .register_native_capability(
            "learning.apply_fix".to_string(),
            "Apply Fix".to_string(),
            "Attempt automatic remediation based on error patterns: retry, synthesize, alternative, or timeout adjustment".to_string(),
            Arc::new({
                let chain = chain_clone.clone();
                let wm = wm_clone.clone();
                let _mp = mp_clone.clone(); // Reserved for future: triggering synthesis
                move |args: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
                    let args_clone = args.clone();
                    let _chain = chain.clone();
                    let wm = wm.clone();
                    let _mp = _mp.clone();
                    async move {
                        let input: ApplyFixInput = parse_input(&args_clone)?;
                        
                        let capability_id = input.capability_id.clone();
                        let action = input.action.clone().unwrap_or_else(|| "auto".to_string());
                        
                        // Determine error category from input or by looking up patterns
                        let error_category = if let Some(cat) = input.error_category.clone() {
                            cat
                        } else {
                            // Look up from WorkingMemory
                            let mut found_category = "Unknown".to_string();
                            if let Ok(wm_guard) = wm.lock() {
                                let tag_set: std::collections::HashSet<String> = [
                                    "learned-pattern".to_string(),
                                    format!("capability:{}", capability_id),
                                ].into_iter().collect();
                                
                                let params = crate::working_memory::backend::QueryParams::with_tags(tag_set)
                                    .with_limit(Some(1));
                                
                                if let Ok(result) = wm_guard.query(&params) {
                                    if let Some(entry) = result.entries.first() {
                                        if let Ok(pattern) = serde_json::from_str::<ExtractedPattern>(&entry.content) {
                                            found_category = pattern.error_category.clone();
                                        }
                                    }
                                }
                            }
                            found_category
                        };
                        
                        // Determine appropriate remediation based on action and error category
                        let (remediation_action, description, plan_mods) = match action.as_str() {
                            "auto" => {
                                // Auto-select based on error category
                                match error_category.as_str() {
                                    "TimeoutError" => {
                                        let multiplier = input.timeout_multiplier.unwrap_or(2.0);
                                        (
                                            "adjust_timeout".to_string(),
                                            format!("Adjusting timeout by {}x for {}", multiplier, capability_id),
                                            vec![PlanModification {
                                                modification_type: "adjust_timeout".to_string(),
                                                target_capability: capability_id.clone(),
                                                parameters: serde_json::json!({
                                                    "timeout_multiplier": multiplier
                                                }),
                                                confidence: 0.7,
                                            }]
                                        )
                                    }
                                    "NetworkError" => {
                                        let max_retries = input.max_retries.unwrap_or(3);
                                        (
                                            "inject_retry".to_string(),
                                            format!("Adding retry logic ({} attempts) for {}", max_retries, capability_id),
                                            vec![PlanModification {
                                                modification_type: "inject_retry".to_string(),
                                                target_capability: capability_id.clone(),
                                                parameters: serde_json::json!({
                                                    "max_retries": max_retries,
                                                    "backoff_ms": 1000
                                                }),
                                                confidence: 0.8,
                                            }]
                                        )
                                    }
                                    "MissingCapability" => {
                                        (
                                            "synthesize_first".to_string(),
                                            format!("Triggering synthesis for missing capability: {}", capability_id),
                                            vec![PlanModification {
                                                modification_type: "synthesize_first".to_string(),
                                                target_capability: capability_id.clone(),
                                                parameters: serde_json::json!({
                                                    "trigger_synthesis": true,
                                                    "synthesis_capability": "planner.synthesize_capability"
                                                }),
                                                confidence: 0.6,
                                            }]
                                        )
                                    }
                                    "SchemaError" => {
                                        (
                                            "add_validation".to_string(),
                                            format!("Adding input validation step before {}", capability_id),
                                            vec![PlanModification {
                                                modification_type: "inject_validation".to_string(),
                                                target_capability: capability_id.clone(),
                                                parameters: serde_json::json!({
                                                    "validate_schema": true
                                                }),
                                                confidence: 0.75,
                                            }]
                                        )
                                    }
                                    "LLMError" => {
                                        (
                                            "inject_fallback".to_string(),
                                            format!("Adding fallback LLM provider for {}", capability_id),
                                            vec![PlanModification {
                                                modification_type: "inject_fallback".to_string(),
                                                target_capability: capability_id.clone(),
                                                parameters: serde_json::json!({
                                                    "fallback_provider": "alternative_llm"
                                                }),
                                                confidence: 0.65,
                                            }]
                                        )
                                    }
                                    _ => {
                                        (
                                            "skip_capability".to_string(),
                                            format!("Suggesting to skip problematic capability: {}", capability_id),
                                            vec![PlanModification {
                                                modification_type: "skip_capability".to_string(),
                                                target_capability: capability_id.clone(),
                                                parameters: serde_json::json!({
                                                    "reason": format!("Frequent {} failures", error_category)
                                                }),
                                                confidence: 0.5,
                                            }]
                                        )
                                    }
                                }
                            }
                            "retry" => {
                                let max_retries = input.max_retries.unwrap_or(3);
                                (
                                    "inject_retry".to_string(),
                                    format!("Adding retry logic ({} attempts) for {}", max_retries, capability_id),
                                    vec![PlanModification {
                                        modification_type: "inject_retry".to_string(),
                                        target_capability: capability_id.clone(),
                                        parameters: serde_json::json!({
                                            "max_retries": max_retries,
                                            "backoff_ms": 1000
                                        }),
                                        confidence: 0.8,
                                    }]
                                )
                            }
                            "synthesize" => {
                                (
                                    "synthesize_first".to_string(),
                                    format!("Triggering synthesis for: {}", capability_id),
                                    vec![PlanModification {
                                        modification_type: "synthesize_first".to_string(),
                                        target_capability: capability_id.clone(),
                                        parameters: serde_json::json!({
                                            "trigger_synthesis": true
                                        }),
                                        confidence: 0.6,
                                    }]
                                )
                            }
                            "adjust_timeout" => {
                                let multiplier = input.timeout_multiplier.unwrap_or(2.0);
                                (
                                    "adjust_timeout".to_string(),
                                    format!("Adjusting timeout by {}x for {}", multiplier, capability_id),
                                    vec![PlanModification {
                                        modification_type: "adjust_timeout".to_string(),
                                        target_capability: capability_id.clone(),
                                        parameters: serde_json::json!({
                                            "timeout_multiplier": multiplier
                                        }),
                                        confidence: 0.7,
                                    }]
                                )
                            }
                            "alternative" => {
                                (
                                    "inject_fallback".to_string(),
                                    format!("Adding fallback alternative for {}", capability_id),
                                    vec![PlanModification {
                                        modification_type: "inject_fallback".to_string(),
                                        target_capability: capability_id.clone(),
                                        parameters: serde_json::json!({
                                            "find_alternative": true
                                        }),
                                        confidence: 0.55,
                                    }]
                                )
                            }
                            _ => {
                                return Err(RuntimeError::Generic(format!(
                                    "Unknown action: {}. Valid actions: auto, retry, synthesize, adjust_timeout, alternative",
                                    action
                                )));
                            }
                        };
                        
                        // Note: CausalChain recording skipped for simplicity.
                        // The primary value is returning plan_modifications for Arbiter use.
                        
                        let output = ApplyFixOutput {
                            success: true,
                            remediation_action,
                            description,
                            triggered_capability: None,
                            triggered_result: None,
                            plan_modifications: plan_mods,
                        };
                        
                        to_value(&output)
                    }
                    .boxed()
                }
            }),
            "medium".to_string(),
        )
        .await?;

    eprintln!("🔧 Registered learning.apply_fix capability");
    Ok(())
}
