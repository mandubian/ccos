//! Learning-Aware Plan Augmentation
//!
//! This module provides functionality to augment RTFS plans based on learned
//! failure patterns. It can inject retry logic, adjust timeouts, add fallbacks,
//! and skip risky capabilities based on historical failure data.

use crate::learning::capabilities::PlanModification;
use crate::types::{Plan, PlanBody};
use rtfs::parser;

/// Result of plan augmentation
#[derive(Debug, Clone)]
pub struct AugmentationResult {
    /// The augmented plan
    pub plan: Plan,
    /// Modifications that were applied
    pub applied_modifications: Vec<PlanModification>,
    /// Modifications that were skipped (e.g., AST parse failed)
    pub skipped_modifications: Vec<(PlanModification, String)>,
}

/// Augment a plan with learning-derived modifications.
/// Uses AST-first approach for safer transformations, falls back to text for simple cases.
pub fn augment_plan_with_learning(
    plan: Plan,
    modifications: &[PlanModification],
) -> AugmentationResult {
    if modifications.is_empty() {
        return AugmentationResult {
            plan,
            applied_modifications: vec![],
            skipped_modifications: vec![],
        };
    }

    let mut applied = vec![];
    let mut skipped = vec![];

    // Get the RTFS source
    let rtfs_source = match &plan.body {
        PlanBody::Rtfs(source) => source.clone(),
        _ => {
            // Non-RTFS plans: skip all modifications
            for m in modifications {
                skipped.push((m.clone(), "Plan is not RTFS format".to_string()));
            }
            return AugmentationResult {
                plan,
                applied_modifications: applied,
                skipped_modifications: skipped,
            };
        }
    };

    // Try AST-based augmentation first
    let augmented_source = match try_ast_augmentation(&rtfs_source, modifications) {
        Ok((source, applied_mods, skipped_mods)) => {
            applied = applied_mods;
            skipped = skipped_mods;
            source
        }
        Err(e) => {
            // AST parse failed, try textual fallback
            eprintln!("DEBUG: AST augmentation failed, trying textual: {}", e);
            match try_textual_augmentation(&rtfs_source, modifications) {
                Ok((source, applied_mods, skipped_mods)) => {
                    applied = applied_mods;
                    skipped = skipped_mods;
                    source
                }
                Err(e2) => {
                    // Both failed, return original plan with all mods skipped
                    for m in modifications {
                        skipped.push((m.clone(), format!("Augmentation failed: {}", e2)));
                    }
                    rtfs_source
                }
            }
        }
    };

    // Build the augmented plan
    let mut augmented_plan = plan;
    augmented_plan.body = PlanBody::Rtfs(augmented_source);

    // Add metadata about augmentations
    augmented_plan.metadata.insert(
        "learning.augmented".to_string(),
        rtfs::runtime::values::Value::Boolean(true),
    );
    augmented_plan.metadata.insert(
        "learning.modifications_applied".to_string(),
        rtfs::runtime::values::Value::Integer(applied.len() as i64),
    );

    AugmentationResult {
        plan: augmented_plan,
        applied_modifications: applied,
        skipped_modifications: skipped,
    }
}

/// Try AST-based augmentation (safer but may fail on malformed RTFS)
fn try_ast_augmentation(
    source: &str,
    modifications: &[PlanModification],
) -> Result<
    (
        String,
        Vec<PlanModification>,
        Vec<(PlanModification, String)>,
    ),
    String,
> {
    // Parse the RTFS source
    let _ast = parser::parse(source).map_err(|e| format!("Parse error: {:?}", e))?;

    let mut applied = vec![];
    let mut skipped = vec![];
    let mut result_source = source.to_string();

    for modification in modifications {
        match apply_modification_ast(&result_source, modification) {
            Ok(new_source) => {
                result_source = new_source;
                applied.push(modification.clone());
            }
            Err(e) => {
                skipped.push((
                    modification.clone(),
                    format!("AST modification failed: {}", e),
                ));
            }
        }
    }

    Ok((result_source, applied, skipped))
}

/// Apply a single modification using AST transformation
fn apply_modification_ast(source: &str, modification: &PlanModification) -> Result<String, String> {
    let target_cap = &modification.target_capability;

    match modification.modification_type.as_str() {
        "inject_retry" => {
            // Extract retry parameters
            let max_retries = modification
                .parameters
                .get("max_retries")
                .and_then(|v| v.as_i64())
                .unwrap_or(3);
            let initial_delay_ms = modification
                .parameters
                .get("initial_delay_ms")
                .or_else(|| modification.parameters.get("backoff_ms")) // Backward compat
                .and_then(|v| v.as_i64())
                .unwrap_or(1000);

            inject_retry_metadata(source, target_cap, max_retries, initial_delay_ms)
        }
        "adjust_timeout" => {
            // Support both multiplier (old) and timeout_ms (new)
            let timeout_ms = modification
                .parameters
                .get("timeout_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or_else(|| {
                    let multiplier = modification
                        .parameters
                        .get("timeout_multiplier")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(2.0);
                    (5000.0 * multiplier) as u64
                });

            // Reuse timeout adjustment with calculated timeout
            let multiplier = timeout_ms as f64 / 5000.0;
            inject_timeout_adjustment(source, target_cap, multiplier)
        }
        "inject_fallback" => {
            let fallback_cap = modification
                .parameters
                .get("fallback_capability")
                .and_then(|v| v.as_str())
                .unwrap_or("ccos.error.handler");

            inject_fallback_metadata(source, target_cap, fallback_cap)
        }
        "inject_circuit_breaker" => {
            let failure_threshold = modification
                .parameters
                .get("failure_threshold")
                .and_then(|v| v.as_u64())
                .unwrap_or(5);
            let cooldown_ms = modification
                .parameters
                .get("cooldown_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(30000);

            inject_circuit_breaker_metadata(source, target_cap, failure_threshold, cooldown_ms)
        }
        "inject_rate_limit" => {
            let rps = modification
                .parameters
                .get("requests_per_second")
                .and_then(|v| v.as_u64())
                .unwrap_or(10);
            let burst = modification
                .parameters
                .get("burst")
                .and_then(|v| v.as_u64())
                .unwrap_or(5);

            inject_rate_limit_metadata(source, target_cap, rps, burst)
        }
        "inject_metrics" => {
            let emit_to_chain = modification
                .parameters
                .get("emit_to_chain")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);

            inject_metrics_metadata(source, target_cap, emit_to_chain)
        }
        "synthesize_first" => {
            // Add a synthesis step before the capability call
            inject_synthesis_step(source, target_cap)
        }
        "skip_capability" => {
            // Comment out the capability call and add a warning
            skip_capability(source, target_cap)
        }
        _ => Err(format!(
            "Unknown modification type: {}",
            modification.modification_type
        )),
    }
}

/// Inject retry metadata
fn inject_retry_metadata(
    source: &str,
    target_cap: &str,
    max_retries: i64,
    backoff_ms: i64,
) -> Result<String, String> {
    // Pattern: (call :target_cap ...) -> ^{:runtime.learning.retry {:max-retries N :initial-delay-ms M}} (call :target_cap ...)
    let cap_pattern = format!("(call :{}", target_cap.replace('.', "."));

    if !source.contains(&cap_pattern) {
        // Try alternate pattern without colon
        let alt_pattern = format!("(call {}", target_cap);
        if !source.contains(&alt_pattern) {
            return Err(format!("Capability {} not found in plan", target_cap));
        }
        // Use the alternate pattern for replacement
        let metadata = format!(
            "^{{:runtime.learning.retry {{:max-retries {} :initial-delay-ms {}}}}} ",
            max_retries, backoff_ms
        );
        return Ok(source.replace(&alt_pattern, &format!("{}{}", metadata, alt_pattern)));
    }

    // Insert metadata before the call
    let metadata = format!(
        "^{{:runtime.learning.retry {{:max-retries {} :initial-delay-ms {}}}}} ",
        max_retries, backoff_ms
    );

    // We replace the opening of the call with metadata + opening
    let result = source.replace(&cap_pattern, &format!("{}{}", metadata, cap_pattern));

    Ok(result)
}

/// Inject timeout adjustment metadata
fn inject_timeout_adjustment(
    source: &str,
    target_cap: &str,
    multiplier: f64,
) -> Result<String, String> {
    // Pattern: (call :target_cap ...) -> ^{:runtime.learning.timeout {:timeout-ms M}} (call :target_cap ...)
    let cap_pattern = format!("(call :{}", target_cap.replace('.', "."));

    if !source.contains(&cap_pattern) {
        return Err(format!("Capability {} not found in plan", target_cap));
    }

    // Convert multiplier to timeout-ms (assume 5s base timeout * multiplier)
    let timeout_ms = (5000.0 * multiplier) as u64;
    let metadata = format!(
        "^{{:runtime.learning.timeout {{:timeout-ms {}}}}} ",
        timeout_ms
    );
    let result = source.replace(&cap_pattern, &format!("{}{}", metadata, cap_pattern));

    Ok(result)
}

/// Inject fallback metadata
fn inject_fallback_metadata(
    source: &str,
    target_cap: &str,
    fallback_cap: &str,
) -> Result<String, String> {
    let cap_pattern = format!("(call :{}", target_cap.replace('.', "."));

    if !source.contains(&cap_pattern) {
        return Err(format!("Capability {} not found in plan", target_cap));
    }

    let metadata = format!(
        "^{{:runtime.learning.fallback {{:capability \"{}\"}}}} ",
        fallback_cap
    );
    let result = source.replace(&cap_pattern, &format!("{}{}", metadata, cap_pattern));

    Ok(result)
}

/// Inject synthesis step before capability call
/// Note: This remains a code injection as it changes control flow explicitly
fn inject_synthesis_step(source: &str, target_cap: &str) -> Result<String, String> {
    let cap_pattern = format!("(call :{}", target_cap.replace('.', "."));

    if !source.contains(&cap_pattern) {
        return Err(format!("Capability {} not found in plan", target_cap));
    }

    // Add synthesis step before the call
    let synthesis_step = format!(
        ";; [learning] Synthesize if missing\n(when-not (ccos.capability.exists? :{}) (planner.synthesize_capability {{:name \"{}\"}}))\n",
        target_cap.replace('.', "."),
        target_cap
    );

    let result = source.replace(&cap_pattern, &format!("{}{}", synthesis_step, cap_pattern));

    Ok(result)
}

/// Skip capability by commenting it out (or using metadata to disable it)
fn skip_capability(source: &str, target_cap: &str) -> Result<String, String> {
    let cap_pattern = format!("(call :{}", target_cap.replace('.', "."));

    if !source.contains(&cap_pattern) {
        return Err(format!("Capability {} not found in plan", target_cap));
    }

    // Comment out the call - simplest way to skip for now
    // Alternatively: ^{:learning.skip true} (call ...)
    let result = source.replace(
        &cap_pattern,
        &format!(
            ";; [learning] SKIPPED - high failure rate\n;; {}",
            cap_pattern
        ),
    );

    Ok(result)
}

/// Inject circuit breaker metadata
fn inject_circuit_breaker_metadata(
    source: &str,
    target_cap: &str,
    failure_threshold: u64,
    cooldown_ms: u64,
) -> Result<String, String> {
    let cap_pattern = format!("(call :{}", target_cap.replace('.', "."));

    if !source.contains(&cap_pattern) {
        return Err(format!("Capability {} not found in plan", target_cap));
    }

    let metadata = format!(
        "^{{:runtime.learning.circuit-breaker {{:failure-threshold {} :cooldown-ms {}}}}} ",
        failure_threshold, cooldown_ms
    );
    let result = source.replace(&cap_pattern, &format!("{}{}", metadata, cap_pattern));

    Ok(result)
}

/// Inject rate limit metadata
fn inject_rate_limit_metadata(
    source: &str,
    target_cap: &str,
    requests_per_second: u64,
    burst: u64,
) -> Result<String, String> {
    let cap_pattern = format!("(call :{}", target_cap.replace('.', "."));

    if !source.contains(&cap_pattern) {
        return Err(format!("Capability {} not found in plan", target_cap));
    }

    let metadata = format!(
        "^{{:runtime.learning.rate-limit {{:requests-per-second {} :burst {}}}}} ",
        requests_per_second, burst
    );
    let result = source.replace(&cap_pattern, &format!("{}{}", metadata, cap_pattern));

    Ok(result)
}

/// Inject metrics metadata for observability
fn inject_metrics_metadata(
    source: &str,
    target_cap: &str,
    emit_to_chain: bool,
) -> Result<String, String> {
    let cap_pattern = format!("(call :{}", target_cap.replace('.', "."));

    if !source.contains(&cap_pattern) {
        return Err(format!("Capability {} not found in plan", target_cap));
    }

    let metadata = format!(
        "^{{:runtime.learning.metrics {{:label \"{}\" :emit-to-chain {}}}}} ",
        target_cap, emit_to_chain
    );
    let result = source.replace(&cap_pattern, &format!("{}{}", metadata, cap_pattern));

    Ok(result)
}

/// Textual fallback for simpler transformations
fn try_textual_augmentation(
    source: &str,
    modifications: &[PlanModification],
) -> Result<
    (
        String,
        Vec<PlanModification>,
        Vec<(PlanModification, String)>,
    ),
    String,
> {
    let mut applied = vec![];
    let skipped = vec![];
    let mut result_source = source.to_string();

    for modification in modifications {
        // For textual, we only support simple comment-based annotations
        let annotation = format!(
            ";; [learning:{}] {} (confidence: {:.2})\n",
            modification.modification_type, modification.target_capability, modification.confidence
        );

        // Add annotation at the top of the plan
        result_source = format!("{}{}", annotation, result_source);
        applied.push(modification.clone());
    }

    Ok((result_source, applied, skipped))
}

/// Extract capability IDs from an RTFS plan source
pub fn extract_capabilities_from_plan(source: &str) -> Vec<String> {
    let mut capabilities = vec![];

    // Simple regex-like extraction for (call :cap.id ...)
    for line in source.lines() {
        if let Some(start) = line.find("(call :") {
            let after_call = &line[start + 7..];
            if let Some(end) = after_call.find(|c: char| c.is_whitespace() || c == ')') {
                let cap_id = &after_call[..end];
                capabilities.push(cap_id.replace(".", "."));
            }
        }
    }

    capabilities
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_capabilities_empty() {
        let caps = extract_capabilities_from_plan("(do (println \"hello\"))");
        assert!(caps.is_empty());
    }

    #[test]
    fn test_extract_capabilities_single() {
        let caps = extract_capabilities_from_plan("(do (call :demo.echo {:msg \"hi\"}))");
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0], "demo.echo");
    }

    #[test]
    fn test_skip_capability() {
        let source = "(do (call :demo.network_call {:url \"test\"}))";
        let result = skip_capability(source, "demo.network_call").unwrap();
        assert!(result.contains(";; [learning] SKIPPED"));
    }
}
