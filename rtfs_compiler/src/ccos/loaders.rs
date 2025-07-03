//! RTFS to CCOS Object Loaders
//!
//! This module provides conversion from RTFS AST objects to CCOS runtime objects.
//! It bridges the gap between the language syntax and the cognitive computing runtime.

use super::types::{Intent, Plan, PlanBody, PlanLanguage};
use crate::ast::{
    Expression, IntentDefinition, Keyword, Literal, MapKey, PlanDefinition, Property, Symbol,
};
use crate::runtime::error::RuntimeError;
use crate::runtime::values::Value;
use std::collections::HashMap;

/// Convert RTFS IntentDefinition to CCOS Intent
pub fn intent_from_rtfs(rtfs_intent: &IntentDefinition) -> Result<Intent, RuntimeError> {
    let mut intent = Intent::with_name(
        rtfs_intent.name.0.clone(),
        String::new(), // Will be set from properties
        String::new(), // Will be set from properties
    );

    // Parse properties
    for property in &rtfs_intent.properties {
        match property.key.0.as_str() {
            ":goal" => {
                intent.goal = expression_to_string(&property.value)?;
            }
            ":constraints" => {
                intent.constraints = expression_to_map(&property.value)?;
            }
            ":preferences" => {
                intent.preferences = expression_to_map(&property.value)?;
            }
            ":success-criteria" => {
                intent.success_criteria = Some(expression_to_value(&property.value)?);
            }
            ":emotional-tone" => {
                intent.emotional_tone = Some(expression_to_string(&property.value)?);
            }
            ":parent-intent" => {
                intent.parent_intent = Some(expression_to_string(&property.value)?);
            }
            ":id" => {
                intent.intent_id = expression_to_string(&property.value)?;
            }
            _ => {
                // Store unknown properties in metadata
                intent.metadata.insert(
                    property.key.0.clone(),
                    expression_to_value(&property.value)?,
                );
            }
        }
    }

    // Validate required fields
    if intent.goal.is_empty() {
        return Err(RuntimeError::new("Intent must have a :goal property"));
    }

    Ok(intent)
}

/// Convert RTFS PlanDefinition to CCOS Plan
pub fn plan_from_rtfs(rtfs_plan: &PlanDefinition) -> Result<Plan, RuntimeError> {
    let mut plan = Plan::new_named(
        rtfs_plan.name.0.clone(),
        PlanLanguage::Rtfs20,          // Default to RTFS 2.0
        PlanBody::Text(String::new()), // Will be set from properties
        Vec::new(),                    // Will be set from properties
    );

    // Parse properties
    for property in &rtfs_plan.properties {
        match property.key.0.as_str() {
            ":body" | ":rtfs-body" => {
                let body_text = expression_to_string(&property.value)?;
                plan.body = PlanBody::Text(body_text);
            }
            ":language" => {
                let language_str = expression_to_string(&property.value)?;
                plan.language = parse_plan_language(&language_str)?;
            }
            ":intents" => {
                plan.intent_ids = expression_to_string_list(&property.value)?;
            }
            ":id" => {
                plan.plan_id = expression_to_string(&property.value)?;
            }
            _ => {
                // Store unknown properties in metadata
                plan.metadata.insert(
                    property.key.0.clone(),
                    expression_to_value(&property.value)?,
                );
            }
        }
    }

    // Validate required fields
    match &plan.body {
        PlanBody::Text(text) if text.is_empty() => {
            return Err(RuntimeError::new(
                "Plan must have a :body or :rtfs-body property",
            ));
        }
        PlanBody::Bytes(bytes) if bytes.is_empty() => {
            return Err(RuntimeError::new("Plan must have a non-empty body"));
        }
        _ => {}
    }

    Ok(plan)
}

/// Helper: Convert Expression to String
fn expression_to_string(expr: &Expression) -> Result<String, RuntimeError> {
    match expr {
        Expression::Literal(Literal::String(s)) => Ok(s.clone()),
        Expression::Literal(Literal::Keyword(k)) => Ok(k.0.clone()),
        Expression::Symbol(s) => Ok(s.0.clone()),
        _ => Err(RuntimeError::new(&format!(
            "Expected string, keyword, or symbol, got: {:?}",
            expr
        ))),
    }
}

/// Helper: Convert Expression to Value
fn expression_to_value(expr: &Expression) -> Result<Value, RuntimeError> {
    match expr {
        Expression::Literal(lit) => literal_to_value(lit),
        Expression::Symbol(s) => Ok(Value::Symbol(Symbol(s.0.clone()))),
        Expression::List(items) => {
            let mut values = Vec::new();
            for item in items {
                values.push(expression_to_value(item)?);
            }
            Ok(Value::List(values))
        }
        Expression::Vector(items) => {
            let mut values = Vec::new();
            for item in items {
                values.push(expression_to_value(item)?);
            }
            Ok(Value::Vector(values))
        }
        Expression::Map(map) => {
            let mut value_map = HashMap::new();
            for (key, value) in map {
                value_map.insert(key.clone(), expression_to_value(value)?);
            }
            Ok(Value::Map(value_map))
        }
        _ => Err(RuntimeError::new(&format!(
            "Unsupported expression type for conversion: {:?}",
            expr
        ))),
    }
}

/// Helper: Convert Literal to Value
fn literal_to_value(lit: &Literal) -> Result<Value, RuntimeError> {
    match lit {
        Literal::Integer(i) => Ok(Value::Integer(*i)),
        Literal::Float(f) => Ok(Value::Float(*f)),
        Literal::String(s) => Ok(Value::String(s.clone())),
        Literal::Boolean(b) => Ok(Value::Boolean(*b)),
        Literal::Keyword(k) => Ok(Value::Keyword(Keyword(k.0.clone()))),
        Literal::Nil => Ok(Value::Nil),
        Literal::Timestamp(s) => Ok(Value::String(s.clone())),
        Literal::Uuid(s) => Ok(Value::String(s.clone())),
        Literal::ResourceHandle(s) => Ok(Value::String(s.clone())),
    }
}

/// Helper: Convert Expression to HashMap
fn expression_to_map(expr: &Expression) -> Result<HashMap<String, Value>, RuntimeError> {
    match expr {
        Expression::Map(map) => {
            let mut value_map = HashMap::new();
            for (key, value) in map {
                let key_str = match key {
                    MapKey::Keyword(k) => k.0.clone(),
                    MapKey::String(s) => s.clone(),
                    MapKey::Integer(i) => i.to_string(),
                };
                value_map.insert(key_str, expression_to_value(value)?);
            }
            Ok(value_map)
        }
        _ => Err(RuntimeError::new(&format!(
            "Expected map expression, got: {:?}",
            expr
        ))),
    }
}

/// Helper: Convert Expression to Vec<String>
fn expression_to_string_list(expr: &Expression) -> Result<Vec<String>, RuntimeError> {
    match expr {
        Expression::List(items) | Expression::Vector(items) => {
            let mut strings = Vec::new();
            for item in items {
                strings.push(expression_to_string(item)?);
            }
            Ok(strings)
        }
        _ => Err(RuntimeError::new(&format!(
            "Expected list or vector of strings, got: {:?}",
            expr
        ))),
    }
}

/// Helper: Parse PlanLanguage from string
fn parse_plan_language(lang: &str) -> Result<PlanLanguage, RuntimeError> {
    match lang.to_lowercase().as_str() {
        "rtfs" | "rtfs20" | "rtfs2.0" => Ok(PlanLanguage::Rtfs20),
        "wasm" => Ok(PlanLanguage::Wasm),
        "python" => Ok(PlanLanguage::Python),
        "graph" | "graphjson" | "json" => Ok(PlanLanguage::GraphJson),
        _ => Ok(PlanLanguage::Other(lang.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{IntentDefinition, Keyword, Literal, PlanDefinition, Property, Symbol};

    #[test]
    fn test_intent_from_rtfs() {
        let rtfs_intent = IntentDefinition {
            name: Symbol("my-intent".to_string()),
            properties: vec![
                Property {
                    key: Keyword(":goal".to_string()),
                    value: Expression::Literal(Literal::String("Analyze data".to_string())),
                },
                Property {
                    key: Keyword(":constraints".to_string()),
                    value: Expression::Map(HashMap::new()),
                },
            ],
        };

        let intent = intent_from_rtfs(&rtfs_intent).unwrap();
        assert_eq!(intent.name, "my-intent");
        assert_eq!(intent.goal, "Analyze data");
    }

    #[test]
    fn test_plan_from_rtfs() {
        let rtfs_plan = PlanDefinition {
            name: Symbol("my-plan".to_string()),
            properties: vec![
                Property {
                    key: Keyword(":body".to_string()),
                    value: Expression::Literal(Literal::String("(+ 1 2)".to_string())),
                },
                Property {
                    key: Keyword(":intents".to_string()),
                    value: Expression::List(vec![Expression::Literal(Literal::String(
                        "intent-1".to_string(),
                    ))]),
                },
            ],
        };

        let plan = plan_from_rtfs(&rtfs_plan).unwrap();
        assert_eq!(plan.name, "my-plan");
        assert_eq!(plan.language, PlanLanguage::Rtfs20);
        assert_eq!(plan.intent_ids, vec!["intent-1"]);
    }
}
