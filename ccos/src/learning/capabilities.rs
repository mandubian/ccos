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
    pub plan_id: String,
    pub intent_id: String,
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
    arbiter: Arc<crate::arbiter::DelegatingArbiter>,
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
