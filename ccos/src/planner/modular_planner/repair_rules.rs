//! Generic Repair Rules for Type Mismatches
//!
//! This module provides pattern-based repair rules for fixing type mismatches
//! in RTFS expressions. Rules are generic and not specific to any particular
//! capability or data structure.

use rtfs::ast::{Expression, Literal, Symbol};
use std::collections::HashMap;

/// Context for attempting expression repair
#[derive(Debug)]
pub struct RepairContext {
    /// The error message from the failed execution
    pub error_message: String,
    /// The expression that failed
    pub failed_expression: String,
    /// Inferred schemas for variables (var_name → RTFS schema string)
    pub schemas: HashMap<String, String>,
}

/// Result of a repair attempt
#[derive(Debug)]
pub enum RepairResult {
    /// Successfully repaired with a new expression
    Repaired(String),
    /// Could not repair
    NoRepair,
}

/// Attempt to repair a failed expression based on error context.
///
/// This function applies generic repair rules based on the error message pattern.
/// Rules are not specific to any particular key or capability.
///
/// # Repair Rules Applied:
/// 1. **Vector with keyword** → Unwrap `(get vec :key)` to `vec`
/// 2. **Expected string, got map** → Wrap in `(str ...)`
/// 3. **Expected string, got vector** → Wrap in `(str ...)`
///
/// When no rule matches, returns `NoRepair` and logs for debugging.
/// The caller should then invoke LLM-based repair dialog.
pub fn attempt_repair(ctx: &RepairContext) -> RepairResult {
    // Rule 1: "got vector with keyword" → unwrap (get ... :keyword) to inner expression
    if ctx.error_message.contains("got vector with keyword")
        || ctx
            .error_message
            .contains("expected map or vector with appropriate key/index, got vector")
    {
        log::debug!(
            "[repair_rules] Matched rule 1 (vector with keyword): {}",
            &ctx.error_message[..ctx.error_message.len().min(100)]
        );
        if let Some(repaired) = unwrap_get_on_vector(&ctx.failed_expression) {
            log::info!(
                "[repair_rules] Repaired: {} → {}",
                ctx.failed_expression,
                repaired
            );
            return RepairResult::Repaired(repaired);
        }
    }

    // Rule 2: "expected string, got map" → wrap in (str ...)
    if ctx.error_message.contains("expected string") && ctx.error_message.contains("got map") {
        log::debug!("[repair_rules] Matched rule 2 (expected string, got map)");
        if let Some(repaired) = wrap_in_str(&ctx.failed_expression) {
            log::info!("[repair_rules] Repaired with str wrap: {}", repaired);
            return RepairResult::Repaired(repaired);
        }
    }

    // Rule 3: "expected string, got vector" → wrap in (str ...)
    if ctx.error_message.contains("expected string") && ctx.error_message.contains("got vector") {
        log::debug!("[repair_rules] Matched rule 3 (expected string, got vector)");
        if let Some(repaired) = wrap_in_str(&ctx.failed_expression) {
            log::info!("[repair_rules] Repaired with str wrap: {}", repaired);
            return RepairResult::Repaired(repaired);
        }
    }

    // No pattern matched - log for debugging (caller should invoke LLM repair)
    log::debug!(
        "[repair_rules] No pattern matched for error: '{}' (expression: '{}'). Consider LLM repair.",
        &ctx.error_message[..ctx.error_message.len().min(80)],
        &ctx.failed_expression[..ctx.failed_expression.len().min(50)]
    );

    RepairResult::NoRepair
}

/// Unwrap (get X :key) → X when X is a vector
///
/// This traverses the expression and finds (get ... :keyword) patterns,
/// returning the inner expression.
fn unwrap_get_on_vector(expr: &str) -> Option<String> {
    use crate::rtfs_bridge::pretty_printer::expression_to_rtfs_string;

    fn find_and_unwrap_get(expr: Expression) -> (Expression, bool) {
        match expr {
            Expression::FunctionCall { callee, arguments } => {
                if let Expression::Symbol(Symbol(sym)) = callee.as_ref() {
                    if sym == "get" && arguments.len() >= 2 {
                        // Check if second arg is a keyword
                        if matches!(&arguments[1], Expression::Literal(Literal::Keyword(_))) {
                            // Return the first argument (the collection), mark as changed
                            return (arguments[0].clone(), true);
                        }
                    }
                }
                // Recurse into arguments
                let mut changed = false;
                let new_args: Vec<Expression> = arguments
                    .into_iter()
                    .map(|a| {
                        let (new_a, c) = find_and_unwrap_get(a);
                        if c {
                            changed = true;
                        }
                        new_a
                    })
                    .collect();
                let (new_callee, c) = find_and_unwrap_get(*callee);
                if c {
                    changed = true;
                }
                (
                    Expression::FunctionCall {
                        callee: Box::new(new_callee),
                        arguments: new_args,
                    },
                    changed,
                )
            }
            Expression::Let(let_expr) => {
                let mut changed = false;
                let new_bindings: Vec<rtfs::ast::LetBinding> = let_expr
                    .bindings
                    .into_iter()
                    .map(|b| {
                        let (new_val, c) = find_and_unwrap_get(*b.value);
                        if c {
                            changed = true;
                        }
                        rtfs::ast::LetBinding {
                            pattern: b.pattern,
                            type_annotation: b.type_annotation,
                            value: Box::new(new_val),
                        }
                    })
                    .collect();
                let new_body: Vec<Expression> = let_expr
                    .body
                    .into_iter()
                    .map(|e| {
                        let (new_e, c) = find_and_unwrap_get(e);
                        if c {
                            changed = true;
                        }
                        new_e
                    })
                    .collect();
                (
                    Expression::Let(rtfs::ast::LetExpr {
                        bindings: new_bindings,
                        body: new_body,
                    }),
                    changed,
                )
            }
            other => (other, false),
        }
    }

    match rtfs::parser::parse_expression(expr) {
        Ok(parsed) => {
            let (repaired, changed) = find_and_unwrap_get(parsed);
            if changed {
                Some(expression_to_rtfs_string(&repaired))
            } else {
                None
            }
        }
        Err(_) => None,
    }
}

/// Wrap an expression in (str ...) for string coercion
fn wrap_in_str(expr: &str) -> Option<String> {
    // Simple approach: wrap the whole expression
    Some(format!("(str {})", expr))
}

/// Proactive validation: check if a (get X :key) would fail on a vector schema
///
/// Returns simplified expression if the original would fail.
pub fn validate_get_expression(
    expr: &str,
    var_schemas: &HashMap<String, String>,
) -> Option<String> {
    use crate::rtfs_bridge::pretty_printer::expression_to_rtfs_string;

    fn check_and_simplify(
        expr: Expression,
        var_schemas: &HashMap<String, String>,
    ) -> (Expression, bool) {
        match expr {
            Expression::FunctionCall { callee, arguments } => {
                if let Expression::Symbol(Symbol(sym)) = callee.as_ref() {
                    if sym == "get" && arguments.len() >= 2 {
                        // Check if first arg is a symbol with a vector schema
                        if let Expression::Symbol(Symbol(var_name)) = &arguments[0] {
                            if let Some(schema) = var_schemas.get(var_name) {
                                if schema.starts_with("[:vector")
                                    || schema.starts_with("[\"vector\"")
                                {
                                    // Second arg is keyword → would fail, return just the symbol
                                    if matches!(
                                        &arguments[1],
                                        Expression::Literal(Literal::Keyword(_))
                                    ) {
                                        return (arguments[0].clone(), true);
                                    }
                                }
                            }
                        }
                    }
                }
                // Recurse
                let mut changed = false;
                let new_args: Vec<Expression> = arguments
                    .into_iter()
                    .map(|a| {
                        let (new_a, c) = check_and_simplify(a, var_schemas);
                        if c {
                            changed = true;
                        }
                        new_a
                    })
                    .collect();
                let (new_callee, c) = check_and_simplify(*callee, var_schemas);
                if c {
                    changed = true;
                }
                (
                    Expression::FunctionCall {
                        callee: Box::new(new_callee),
                        arguments: new_args,
                    },
                    changed,
                )
            }
            Expression::Let(let_expr) => {
                let mut changed = false;
                let new_bindings: Vec<rtfs::ast::LetBinding> = let_expr
                    .bindings
                    .into_iter()
                    .map(|b| {
                        let (new_val, c) = check_and_simplify(*b.value, var_schemas);
                        if c {
                            changed = true;
                        }
                        rtfs::ast::LetBinding {
                            pattern: b.pattern,
                            type_annotation: b.type_annotation,
                            value: Box::new(new_val),
                        }
                    })
                    .collect();
                let new_body: Vec<Expression> = let_expr
                    .body
                    .into_iter()
                    .map(|e| {
                        let (new_e, c) = check_and_simplify(e, var_schemas);
                        if c {
                            changed = true;
                        }
                        new_e
                    })
                    .collect();
                (
                    Expression::Let(rtfs::ast::LetExpr {
                        bindings: new_bindings,
                        body: new_body,
                    }),
                    changed,
                )
            }
            other => (other, false),
        }
    }

    match rtfs::parser::parse_expression(expr) {
        Ok(parsed) => {
            let (repaired, changed) = check_and_simplify(parsed, var_schemas);
            if changed {
                Some(expression_to_rtfs_string(&repaired))
            } else {
                None
            }
        }
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unwrap_get_on_vector() {
        let expr = r#"(get step_3 :result)"#;
        let result = unwrap_get_on_vector(expr);
        assert!(result.is_some());
        assert!(result.unwrap().contains("step_3"));
    }

    #[test]
    fn test_attempt_repair_vector_with_keyword() {
        let ctx = RepairContext {
            error_message: "Type error in get: expected map or vector with appropriate key/index, got vector with keyword".to_string(),
            failed_expression: "(get step_3 :result)".to_string(),
            schemas: HashMap::new(),
        };

        match attempt_repair(&ctx) {
            RepairResult::Repaired(repaired) => {
                assert!(repaired.contains("step_3"));
                assert!(!repaired.contains(":result"));
            }
            RepairResult::NoRepair => panic!("Expected repair"),
        }
    }

    #[test]
    fn test_validate_get_expression_proactive() {
        let expr = r#"(get my_var :title)"#;
        let mut schemas = HashMap::new();
        schemas.insert(
            "my_var".to_string(),
            "[:vector [:map {:title :string}]]".to_string(),
        );

        let result = validate_get_expression(expr, &schemas);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "my_var");
    }
}
