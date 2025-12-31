//! Effects and Permissions Propagation for Plan-as-Capability
//!
//! This module analyzes a Plan's body to extract capability calls and
//! conservatively propagates their effects and permissions to the wrapper capability.

use super::errors::RtfsBridgeError;
use crate::capability_marketplace::CapabilityManifest;
use crate::types::{Plan, PlanBody};
use rtfs::ast::{Expression, Literal, Symbol};
use std::collections::HashSet;

/// Configuration for effects propagation analysis
#[derive(Debug, Clone)]
pub struct EffectsPropagationConfig {
    /// Whether to include effects from capabilities
    pub include_effects: bool,
    /// Whether to include permissions from capabilities
    pub include_permissions: bool,
    /// Whether to be conservative (include all) or optimistic (only include common subset)
    pub conservative: bool,
}

impl Default for EffectsPropagationConfig {
    fn default() -> Self {
        Self {
            include_effects: true,
            include_permissions: true,
            conservative: true, // Default to conservative (safer)
        }
    }
}

/// Result of effects propagation analysis
#[derive(Debug, Clone)]
pub struct PropagatedEffects {
    /// All effects from used capabilities (union)
    pub effects: Vec<String>,
    /// All permissions from used capabilities (union)
    pub permissions: Vec<String>,
    /// Capability IDs that were analyzed
    pub analyzed_capabilities: Vec<String>,
}

impl PropagatedEffects {
    pub fn new() -> Self {
        Self {
            effects: Vec::new(),
            permissions: Vec::new(),
            analyzed_capabilities: Vec::new(),
        }
    }

    pub fn empty() -> Self {
        Self::new()
    }
}

/// Analyze a plan and propagate effects/permissions from used capabilities
///
/// This function:
/// 1. Extracts all capability calls from the plan's body
/// 2. Looks up each capability using the provided lookup function
/// 3. Collects all effects and permissions (union)
/// 4. Returns the propagated effects and permissions
///
/// The `capability_lookup` function should return `Some(CapabilityManifest)` if the
/// capability is found, or `None` if not found. This allows flexibility in how
/// capabilities are looked up (from marketplace, cache, etc.).
pub fn propagate_effects_from_plan<F>(
    plan: &Plan,
    capability_lookup: F,
    config: EffectsPropagationConfig,
) -> Result<PropagatedEffects, RtfsBridgeError>
where
    F: Fn(&str) -> Option<CapabilityManifest>,
{
    let mut propagated = PropagatedEffects::new();

    // Extract capability IDs from plan body
    let capability_ids = extract_capability_ids_from_plan(plan)?;

    if capability_ids.is_empty() {
        return Ok(propagated);
    }

    // Collect effects and permissions from all used capabilities
    let mut all_effects = HashSet::new();
    let mut all_permissions = HashSet::new();

    for cap_id in &capability_ids {
        propagated.analyzed_capabilities.push(cap_id.clone());

        // Look up capability using provided function
        if let Some(manifest) = capability_lookup(cap_id) {
            // Add effects
            if config.include_effects {
                for effect in &manifest.effects {
                    all_effects.insert(effect.clone());
                }
            }

            // Add permissions
            if config.include_permissions {
                for permission in &manifest.permissions {
                    all_permissions.insert(permission.clone());
                }
            }
        } else {
            // Capability not found - conservative approach:
            // If conservative mode, we could add a generic effect/permission
            // For now, we just skip it (capability might be synthesized later)
        }
    }

    // Convert to sorted vectors
    let mut effects: Vec<String> = all_effects.into_iter().collect();
    effects.sort();
    propagated.effects = effects;

    let mut permissions: Vec<String> = all_permissions.into_iter().collect();
    permissions.sort();
    propagated.permissions = permissions;

    Ok(propagated)
}

/// Extract all capability IDs from a plan's body
///
/// This analyzes the RTFS code to find all `(call :capability.id ...)` patterns.
fn extract_capability_ids_from_plan(plan: &Plan) -> Result<Vec<String>, RtfsBridgeError> {
    let body_str = match &plan.body {
        PlanBody::Source(rtfs_code) | PlanBody::Rtfs(rtfs_code) => rtfs_code.clone(),
        PlanBody::Binary(_) | PlanBody::Wasm(_) => {
            // For binary/WASM plans, we can't statically analyze the body
            // Return empty list (conservative: no effects propagated)
            return Ok(Vec::new());
        }
    };

    // Parse RTFS code to extract capability calls
    // We use a simple regex-based approach for now, but could use the full RTFS parser
    let capability_ids = extract_capability_ids_from_rtfs_string(&body_str)?;

    Ok(capability_ids)
}

/// Extract capability IDs from RTFS code string using regex
///
/// This finds all patterns like `(call :capability.id ...)` and extracts the capability ID.
fn extract_capability_ids_from_rtfs_string(
    rtfs_code: &str,
) -> Result<Vec<String>, RtfsBridgeError> {
    use regex::Regex;

    let mut capability_ids = HashSet::new();

    // Regex to match (call :capability.id ...) patterns
    // Handles both keyword and symbol forms: (call :cap.id ...) or (call cap.id ...)
    let call_regex = Regex::new(r#"(?m)\(call\s+[:]?([a-zA-Z0-9._-]+)\s+"#).map_err(|e| {
        RtfsBridgeError::ValidationFailed {
            message: format!("Failed to compile regex for capability extraction: {}", e),
        }
    })?;

    for captures in call_regex.captures_iter(rtfs_code) {
        if let Some(cap_id_match) = captures.get(1) {
            let cap_id = cap_id_match.as_str().trim().to_string();
            if !cap_id.is_empty() {
                capability_ids.insert(cap_id);
            }
        }
    }

    // Also try to parse the RTFS code as an expression tree and extract from AST
    // This is more robust than regex but requires parsing
    if let Ok(expr) = rtfs::parser::parse_expression(rtfs_code) {
        extract_capability_ids_from_expression(&expr, &mut capability_ids)?;
    }

    let mut result: Vec<String> = capability_ids.into_iter().collect();
    result.sort();
    Ok(result)
}

/// Extract capability IDs from an RTFS Expression AST
///
/// Recursively walks the expression tree to find all `(call :capability.id ...)` forms.
fn extract_capability_ids_from_expression(
    expr: &Expression,
    capability_ids: &mut HashSet<String>,
) -> Result<(), RtfsBridgeError> {
    match expr {
        Expression::List(items) => {
            // Check if this is a (call ...) form
            if let Some(Expression::Symbol(Symbol(symbol_name))) = items.first() {
                if symbol_name == "call" && items.len() > 1 {
                    // Extract capability ID from second argument
                    let cap_id_expr = &items[1];
                    let mut cap_id = String::new();
                    match cap_id_expr {
                        Expression::Literal(Literal::Keyword(keyword)) => {
                            cap_id = keyword.0.clone();
                        }
                        Expression::Symbol(Symbol(symbol)) => {
                            cap_id = symbol.clone();
                        }
                        _ => {
                            // Not a recognizable capability ID format
                            // Continue recursion to find nested calls
                        }
                    }

                    if !cap_id.is_empty() {
                        capability_ids.insert(cap_id);
                    }
                }
            }

            // Recursively process all items
            for item in items {
                extract_capability_ids_from_expression(item, capability_ids)?;
            }
        }
        Expression::Map(map) => {
            // Recursively process map values
            for (_key, value) in map {
                extract_capability_ids_from_expression(value, capability_ids)?;
            }
        }
        Expression::Vector(vec) => {
            // Recursively process vector elements
            for item in vec {
                extract_capability_ids_from_expression(item, capability_ids)?;
            }
        }
        Expression::Fn(fn_expr) => {
            // Recursively process function body
            for expr in &fn_expr.body {
                extract_capability_ids_from_expression(expr, capability_ids)?;
            }
        }
        Expression::Let(let_expr) => {
            // Process bindings
            for binding in &let_expr.bindings {
                extract_capability_ids_from_expression(&binding.value, capability_ids)?;
            }
            // Process body
            for expr in &let_expr.body {
                extract_capability_ids_from_expression(expr, capability_ids)?;
            }
        }
        Expression::If(if_expr) => {
            // Process condition, then, and else branches
            extract_capability_ids_from_expression(&if_expr.condition, capability_ids)?;
            extract_capability_ids_from_expression(&if_expr.then_branch, capability_ids)?;
            if let Some(else_branch) = &if_expr.else_branch {
                extract_capability_ids_from_expression(else_branch, capability_ids)?;
            }
        }
        Expression::Do(do_expr) => {
            // Process all expressions in do block
            for expr in &do_expr.expressions {
                extract_capability_ids_from_expression(expr, capability_ids)?;
            }
        }
        Expression::Match(match_expr) => {
            // Process match expression and all clauses
            extract_capability_ids_from_expression(&match_expr.expression, capability_ids)?;
            for clause in &match_expr.clauses {
                extract_capability_ids_from_expression(&clause.body, capability_ids)?;
                if let Some(guard) = &clause.guard {
                    extract_capability_ids_from_expression(guard, capability_ids)?;
                }
            }
        }
        Expression::For(for_expr) => {
            // Process for comprehension bindings and body
            for binding in &for_expr.bindings {
                extract_capability_ids_from_expression(binding, capability_ids)?;
            }
            extract_capability_ids_from_expression(&for_expr.body, capability_ids)?;
        }
        _ => {
            // Leaf expressions (symbols, literals, etc.) don't contain capability calls
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Plan, PlanBody, PlanLanguage};

    #[test]
    fn test_extract_capability_ids_from_rtfs_string() {
        let rtfs_code = r#"
            (do
              (step "step1" (call :github.list-issues {}))
              (step "step2" (call :data.filter {:filter "open"}))
              (step "step3" (let [result (call :data.format.structure {:input result})]
                              result))
            )
        "#;

        let capability_ids = extract_capability_ids_from_rtfs_string(rtfs_code).unwrap();

        assert!(capability_ids.contains(&"github.list-issues".to_string()));
        assert!(capability_ids.contains(&"data.filter".to_string()));
        assert!(capability_ids.contains(&"data.format.structure".to_string()));
        assert_eq!(capability_ids.len(), 3);
    }

    #[test]
    fn test_extract_capability_ids_without_call() {
        let rtfs_code = r#"
            (do
              (step "step1" {:result "test"})
              (let [x 1] (+ x 2))
            )
        "#;

        let capability_ids = extract_capability_ids_from_rtfs_string(rtfs_code).unwrap();
        assert_eq!(capability_ids.len(), 0);
    }
}
