use super::errors::RtfsBridgeError;
use crate::types::{Intent, Plan};
use rtfs::ast::TypeExpr;
use rtfs::runtime::values::Value;
use std::collections::HashMap;

/// Validates a CCOS Intent extracted from RTFS
pub fn validate_intent(intent: &Intent) -> Result<(), RtfsBridgeError> {
    // Check required fields
    if intent.goal.is_empty() {
        return Err(RtfsBridgeError::ValidationFailed {
            message: "Intent goal cannot be empty".to_string(),
        });
    }

    if intent.intent_id.is_empty() {
        return Err(RtfsBridgeError::ValidationFailed {
            message: "Intent ID cannot be empty".to_string(),
        });
    }

    // Validate constraints format
    for (key, _value) in &intent.constraints {
        if key.trim().is_empty() {
            return Err(RtfsBridgeError::ValidationFailed {
                message: "Constraint key cannot be empty".to_string(),
            });
        }
    }

    // Validate preferences format
    for (key, _value) in &intent.preferences {
        if key.trim().is_empty() {
            return Err(RtfsBridgeError::ValidationFailed {
                message: "Preference key cannot be empty".to_string(),
            });
        }
    }

    Ok(())
}

/// Validates a CCOS Plan extracted from RTFS
pub fn validate_plan(plan: &Plan) -> Result<(), RtfsBridgeError> {
    // Check required fields
    if plan.plan_id.is_empty() {
        return Err(RtfsBridgeError::ValidationFailed {
            message: "Plan ID cannot be empty".to_string(),
        });
    }

    // Validate plan body
    match &plan.body {
        crate::types::PlanBody::Rtfs(rtfs_code) => {
            if rtfs_code.trim().is_empty() {
                return Err(RtfsBridgeError::ValidationFailed {
                    message: "Plan RTFS body cannot be empty".to_string(),
                });
            }
        }
        crate::types::PlanBody::Wasm(_) => {
            // WASM bodies are binary, so we can't check for emptiness in the same way
            // This is handled by the WASM runtime
        }
    }

    // Validate intent IDs format
    for (i, intent_id) in plan.intent_ids.iter().enumerate() {
        if intent_id.trim().is_empty() {
            return Err(RtfsBridgeError::ValidationFailed {
                message: format!("Intent ID {} cannot be empty", i),
            });
        }
    }

    // Validate capabilities required format
    for (i, capability) in plan.capabilities_required.iter().enumerate() {
        if capability.trim().is_empty() {
            return Err(RtfsBridgeError::ValidationFailed {
                message: format!("Required capability {} cannot be empty", i),
            });
        }
    }

    // Validate policies format
    for (key, _value) in &plan.policies {
        if key.trim().is_empty() {
            return Err(RtfsBridgeError::ValidationFailed {
                message: "Policy key cannot be empty".to_string(),
            });
        }
    }

    // Validate annotations format
    for (key, _value) in &plan.annotations {
        if key.trim().is_empty() {
            return Err(RtfsBridgeError::ValidationFailed {
                message: "Annotation key cannot be empty".to_string(),
            });
        }
    }

    // Validate input/output schemas if present (TypeExpr-aware validation)
    if let Some(input_schema) = &plan.input_schema {
        validate_type_expr_schema(input_schema, "input-schema")?;
    }

    if let Some(output_schema) = &plan.output_schema {
        validate_type_expr_schema(output_schema, "output-schema")?;
    }

    Ok(())
}

/// Validates that a Value represents a valid TypeExpr schema structure
///
/// This checks that input/output schemas in Plans and Capabilities are
/// properly structured as TypeExpr maps (e.g., {:key1 :type1 :key2 :type2}).
fn validate_type_expr_schema(schema: &Value, schema_name: &str) -> Result<(), RtfsBridgeError> {
    match schema {
        Value::Map(map) => {
            // Validate that map values can be parsed as TypeExpr
            for (key, value) in map {
                let key_str = match key {
                    rtfs::ast::MapKey::String(s) => s.clone(),
                    rtfs::ast::MapKey::Keyword(k) => format!(":{}", k.0),
                    rtfs::ast::MapKey::Integer(i) => i.to_string(),
                };

                // Try to parse the value as a TypeExpr
                match value {
                    Value::Keyword(k) => {
                        // Keywords like :int, :string, :any are valid primitive types
                        let type_str = format!(":{}", k.0);
                        if TypeExpr::from_str(&type_str).is_err() {
                            // Try as alias
                            let _ = TypeExpr::from_str(&k.0);
                        }
                    }
                    Value::String(s) => {
                        // String might be a type expression in string form
                        if TypeExpr::from_str(s).is_err() {
                            return Err(RtfsBridgeError::ValidationFailed {
                                message: format!(
                                    "Invalid type expression in {} for key '{}': {}",
                                    schema_name, key_str, s
                                ),
                            });
                        }
                    }
                    Value::Map(nested_map) => {
                        // Nested map might be a complex type (Map, Vector, etc.)
                        // Recursively validate
                        let nested_value = Value::Map(nested_map.clone());
                        if let Err(e) = validate_type_expr_schema(
                            &nested_value,
                            &format!("{}.{}", schema_name, key_str),
                        ) {
                            return Err(e);
                        }
                    }
                    Value::Vector(vec) => {
                        // Vector might represent a Vector type or Union type
                        // Basic validation: check if elements are valid type expressions
                        for (i, item) in vec.iter().enumerate() {
                            match item {
                                Value::Keyword(k) => {
                                    let type_str = format!(":{}", k.0);
                                    if TypeExpr::from_str(&type_str).is_err()
                                        && TypeExpr::from_str(&k.0).is_err()
                                    {
                                        return Err(RtfsBridgeError::ValidationFailed {
                                            message: format!(
                                                "Invalid type expression in {} for key '{}[{}]': {}",
                                                schema_name, key_str, i, k.0
                                            ),
                                        });
                                    }
                                }
                                Value::String(s) => {
                                    if TypeExpr::from_str(s).is_err() {
                                        return Err(RtfsBridgeError::ValidationFailed {
                                            message: format!(
                                                "Invalid type expression in {} for key '{}[{}]': {}",
                                                schema_name, key_str, i, s
                                            ),
                                        });
                                    }
                                }
                                _ => {
                                    // Allow other types for now (could be complex nested structures)
                                }
                            }
                        }
                    }
                    _ => {
                        // Allow other value types (they might represent complex type expressions)
                        // In a stricter implementation, we'd parse them as RTFS expressions first
                    }
                }
            }
            Ok(())
        }
        Value::Nil => {
            // Nil schema means no schema (accepts anything) - this is valid
            Ok(())
        }
        _ => {
            // For non-map schemas, try to parse as a type expression string
            // This handles cases where schema is stored as a string representation
            if let Value::String(s) = schema {
                TypeExpr::from_str(s).map_err(|e| RtfsBridgeError::ValidationFailed {
                    message: format!("Invalid type expression in {}: {}", schema_name, e),
                })?;
                Ok(())
            } else {
                Err(RtfsBridgeError::ValidationFailed {
                    message: format!(
                        "Schema {} must be a map or string, got: {:?}",
                        schema_name, schema
                    ),
                })
            }
        }
    }
}

/// Validates a Capability's input/output schemas
///
/// This is a helper function that can be used when validating capabilities
/// extracted from RTFS expressions.
pub fn validate_capability_schemas(
    input_schema: Option<&Value>,
    output_schema: Option<&Value>,
) -> Result<(), RtfsBridgeError> {
    if let Some(schema) = input_schema {
        validate_type_expr_schema(schema, "input-schema")?;
    }

    if let Some(schema) = output_schema {
        validate_type_expr_schema(schema, "output-schema")?;
    }

    Ok(())
}

/// Validates that a Plan's input schema is compatible with its Intent's constraints
pub fn validate_plan_intent_compatibility(
    plan: &Plan,
    intent: &Intent,
) -> Result<(), RtfsBridgeError> {
    // Check that the plan references the intent
    if !plan.intent_ids.contains(&intent.intent_id) {
        return Err(RtfsBridgeError::ValidationFailed {
            message: format!("Plan does not reference intent '{}'", intent.intent_id),
        });
    }

    // TODO: Add more sophisticated validation of input schema vs intent constraints
    // This would involve analyzing the RTFS body and checking that it can handle
    // the constraints defined in the intent

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtfs::ast::{Keyword, MapKey};
    use rtfs::runtime::values::Value;

    #[test]
    fn test_validate_type_expr_schema_with_keywords() {
        let mut schema = HashMap::new();
        schema.insert(
            MapKey::Keyword(Keyword("input".to_string())),
            Value::Keyword(Keyword("string".to_string())),
        );
        schema.insert(
            MapKey::Keyword(Keyword("count".to_string())),
            Value::Keyword(Keyword("int".to_string())),
        );

        let schema_value = Value::Map(schema);
        assert!(validate_type_expr_schema(&schema_value, "input-schema").is_ok());
    }

    #[test]
    fn test_validate_type_expr_schema_with_strings() {
        let mut schema = HashMap::new();
        schema.insert(
            MapKey::Keyword(Keyword("input".to_string())),
            Value::String(":string".to_string()),
        );

        let schema_value = Value::Map(schema);
        assert!(validate_type_expr_schema(&schema_value, "input-schema").is_ok());
    }

    #[test]
    fn test_validate_type_expr_schema_nil() {
        // Nil schema is valid (accepts anything)
        assert!(validate_type_expr_schema(&Value::Nil, "input-schema").is_ok());
    }

    #[test]
    fn test_validate_plan_with_schemas() {
        use crate::types::{Plan, PlanBody, PlanLanguage};

        let mut plan = Plan::new_named(
            "test".to_string(),
            PlanLanguage::Rtfs20,
            PlanBody::Rtfs("(do (step \"test\" {}))".to_string()),
            vec![],
        );

        // Add valid input schema
        let mut input_schema = HashMap::new();
        input_schema.insert(
            MapKey::Keyword(Keyword("input".to_string())),
            Value::Keyword(Keyword("string".to_string())),
        );
        plan.input_schema = Some(Value::Map(input_schema));

        assert!(validate_plan(&plan).is_ok());
    }
}
