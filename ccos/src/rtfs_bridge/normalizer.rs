//! Normalization of function-call syntax to canonical map format
//!
//! This module provides functions to convert RTFS function-call expressions
//! (e.g., `(plan "name" :body ...)`) into canonical map format
//! (e.g., `{:type "plan" :name "name" :body ...}`) with deprecation warnings.

use super::canonical_schemas::{CanonicalCapabilitySchema, CanonicalPlanSchema};
use super::errors::RtfsBridgeError;
use rtfs::ast::{Expression, Literal, MapKey};
use rtfs::runtime::values::Value;
use std::collections::HashMap;

/// Configuration for normalization behavior
#[derive(Debug, Clone, Copy)]
pub struct NormalizationConfig {
    /// Emit deprecation warnings when function-call syntax is detected
    pub warn_on_function_call: bool,
    /// Validate normalized maps against canonical schemas
    pub validate_after_normalization: bool,
}

impl Default for NormalizationConfig {
    fn default() -> Self {
        Self {
            warn_on_function_call: true,
            validate_after_normalization: true,
        }
    }
}

/// Normalize a Plan expression to canonical map format
///
/// Converts function calls like `(plan "name" :body ...)` or `(ccos/plan "name" :body ...)`
/// to canonical maps like `{:type "plan" :name "name" :body ...}`.
///
/// If the input is already a map, validates it and returns it as-is (or normalizes field names).
pub fn normalize_plan_to_map(
    expr: &Expression,
    config: NormalizationConfig,
) -> Result<Expression, RtfsBridgeError> {
    match expr {
        Expression::FunctionCall { callee, arguments } => {
            // Emit deprecation warning if enabled
            if config.warn_on_function_call {
                eprintln!(
                    "⚠️  DEPRECATED: Plan function-call syntax detected. Use canonical map format: {:?}",
                    expr
                );
                eprintln!("   Migration: (plan \"name\" :body ...) → {{:type \"plan\" :name \"name\" :body ...}}");
            }

            // Extract callee name
            let callee_name = match callee.as_ref() {
                Expression::Symbol(s) => &s.0,
                _ => {
                    return Err(RtfsBridgeError::InvalidCcosFunctionCall {
                        message: "Plan callee must be a symbol".to_string(),
                    });
                }
            };

            // Accept both "plan" and "ccos/plan"
            if callee_name != "plan" && callee_name != "ccos/plan" {
                return Err(RtfsBridgeError::InvalidCcosFunctionCall {
                    message: format!("Expected 'plan' or 'ccos/plan', got: {}", callee_name),
                });
            }

            // Extract name from first argument
            let name = if let Some(Expression::Literal(Literal::String(s))) = arguments.first() {
                s.clone()
            } else {
                return Err(RtfsBridgeError::MissingRequiredField {
                    field: "name".to_string(),
                });
            };

            // Build canonical map
            let mut plan_map: HashMap<MapKey, Expression> = HashMap::new();

            // Required fields
            plan_map.insert(
                MapKey::String(":type".to_string()),
                Expression::Literal(Literal::String("plan".to_string())),
            );
            plan_map.insert(
                MapKey::String(":name".to_string()),
                Expression::Literal(Literal::String(name)),
            );

            // Parse remaining arguments as keyword-value pairs or a single map
            if arguments.len() > 1 {
                // Check if second argument is a map (new style)
                if let Some(Expression::Map(kwargs)) = arguments.get(1) {
                    // Merge keyword arguments into canonical map
                    for (key, value) in kwargs {
                        let key_str = map_key_to_string(key);
                        // Normalize key (ensure it starts with :)
                        let normalized_key = if key_str.starts_with(':') {
                            key_str.clone()
                        } else {
                            format!(":{}", key_str)
                        };
                        plan_map.insert(MapKey::String(normalized_key), value.clone());
                    }
                } else {
                    // Old style: keyword-value pairs as separate arguments
                    let mut i = 1;
                    while i < arguments.len() {
                        if let Expression::Literal(Literal::Keyword(k)) = &arguments[i] {
                            let key = format!(":{}", k.0);
                            if i + 1 >= arguments.len() {
                                return Err(RtfsBridgeError::InvalidObjectFormat {
                                    message: format!("Plan property '{}' requires a value", key),
                                });
                            }
                            plan_map.insert(MapKey::String(key), arguments[i + 1].clone());
                            i += 2;
                        } else {
                            return Err(RtfsBridgeError::InvalidObjectFormat {
                                message: format!(
                                    "Plan properties must be keyword-value pairs, got: {:?}",
                                    arguments[i]
                                ),
                            });
                        }
                    }
                }
            }

            // Validate if enabled
            if config.validate_after_normalization {
                // Convert Expression map to Value map for validation
                let value_map: HashMap<MapKey, Value> = plan_map
                    .iter()
                    .filter_map(|(k, v)| Some((k.clone(), expression_to_value_simple(v)?)))
                    .collect();

                if let Err(e) = CanonicalPlanSchema::validate(&value_map) {
                    return Err(RtfsBridgeError::InvalidObjectFormat {
                        message: format!("Normalized plan failed validation: {}", e),
                    });
                }
            }

            Ok(Expression::Map(plan_map))
        }
        Expression::Map(map) => {
            // Already a map - validate and normalize field names if needed
            let mut normalized_map: HashMap<MapKey, Expression> = HashMap::new();

            // Ensure :type is set
            if !map.contains_key(&MapKey::String(":type".to_string())) {
                normalized_map.insert(
                    MapKey::String(":type".to_string()),
                    Expression::Literal(Literal::String("plan".to_string())),
                );
            }

            // Copy and normalize all fields
            for (key, value) in map {
                let key_str = map_key_to_string(key);
                let normalized_key = if key_str.starts_with(':') {
                    key_str.clone()
                } else {
                    format!(":{}", key_str)
                };
                normalized_map.insert(MapKey::String(normalized_key), value.clone());
            }

            // Validate if enabled
            if config.validate_after_normalization {
                let value_map: HashMap<MapKey, Value> = normalized_map
                    .iter()
                    .filter_map(|(k, v)| Some((k.clone(), expression_to_value_simple(v)?)))
                    .collect();

                if let Err(e) = CanonicalPlanSchema::validate(&value_map) {
                    return Err(RtfsBridgeError::InvalidObjectFormat {
                        message: format!("Plan map failed validation: {}", e),
                    });
                }
            }

            Ok(Expression::Map(normalized_map))
        }
        _ => Err(RtfsBridgeError::InvalidObjectFormat {
            message: format!(
                "Expected FunctionCall or Map for Plan normalization, got: {:?}",
                expr
            ),
        }),
    }
}

/// Normalize a Capability expression to canonical map format
///
/// Converts function calls like `(capability "id" :name "..." :implementation ...)`
/// to canonical maps like `{:type "capability" :id "id" :name "..." :implementation ...}`.
pub fn normalize_capability_to_map(
    expr: &Expression,
    config: NormalizationConfig,
) -> Result<Expression, RtfsBridgeError> {
    match expr {
        Expression::FunctionCall { callee, arguments } => {
            // Emit deprecation warning if enabled
            if config.warn_on_function_call {
                eprintln!(
                    "⚠️  DEPRECATED: Capability function-call syntax detected. Use canonical map format: {:?}",
                    expr
                );
                eprintln!("   Migration: (capability \"id\" :name \"...\" ...) → {{:type \"capability\" :id \"id\" :name \"...\" ...}}");
            }

            // Extract callee name
            let callee_name = match callee.as_ref() {
                Expression::Symbol(s) => &s.0,
                _ => {
                    return Err(RtfsBridgeError::InvalidCcosFunctionCall {
                        message: "Capability callee must be a symbol".to_string(),
                    });
                }
            };

            if callee_name != "capability" {
                return Err(RtfsBridgeError::InvalidCcosFunctionCall {
                    message: format!("Expected 'capability', got: {}", callee_name),
                });
            }

            // Extract id from first argument
            let id = if let Some(Expression::Literal(Literal::String(s))) = arguments.first() {
                s.clone()
            } else {
                return Err(RtfsBridgeError::MissingRequiredField {
                    field: "id".to_string(),
                });
            };

            // Build canonical map
            let mut cap_map: HashMap<MapKey, Expression> = HashMap::new();

            // Required fields
            cap_map.insert(
                MapKey::String(":type".to_string()),
                Expression::Literal(Literal::String("capability".to_string())),
            );
            cap_map.insert(
                MapKey::String(":id".to_string()),
                Expression::Literal(Literal::String(id)),
            );

            // Parse remaining arguments as keyword-value pairs or a single map
            if arguments.len() > 1 {
                // Check if second argument is a map (new style)
                if let Some(Expression::Map(kwargs)) = arguments.get(1) {
                    // Merge keyword arguments into canonical map
                    for (key, value) in kwargs {
                        let key_str = map_key_to_string(key);
                        let normalized_key = if key_str.starts_with(':') {
                            key_str.clone()
                        } else {
                            format!(":{}", key_str)
                        };
                        cap_map.insert(MapKey::String(normalized_key), value.clone());
                    }
                } else {
                    // Old style: keyword-value pairs as separate arguments
                    let mut i = 1;
                    while i < arguments.len() {
                        if let Expression::Literal(Literal::Keyword(k)) = &arguments[i] {
                            let key = format!(":{}", k.0);
                            if i + 1 >= arguments.len() {
                                return Err(RtfsBridgeError::InvalidObjectFormat {
                                    message: format!(
                                        "Capability property '{}' requires a value",
                                        key
                                    ),
                                });
                            }
                            cap_map.insert(MapKey::String(key), arguments[i + 1].clone());
                            i += 2;
                        } else {
                            return Err(RtfsBridgeError::InvalidObjectFormat {
                                message: format!(
                                    "Capability properties must be keyword-value pairs, got: {:?}",
                                    arguments[i]
                                ),
                            });
                        }
                    }
                }
            }

            // Validate if enabled
            if config.validate_after_normalization {
                let value_map: HashMap<MapKey, Value> = cap_map
                    .iter()
                    .filter_map(|(k, v)| Some((k.clone(), expression_to_value_simple(v)?)))
                    .collect();

                if let Err(e) = CanonicalCapabilitySchema::validate(&value_map) {
                    return Err(RtfsBridgeError::InvalidObjectFormat {
                        message: format!("Normalized capability failed validation: {}", e),
                    });
                }
            }

            Ok(Expression::Map(cap_map))
        }
        Expression::Map(map) => {
            // Already a map - validate and normalize field names if needed
            let mut normalized_map: HashMap<MapKey, Expression> = HashMap::new();

            // Ensure :type is set
            if !map.contains_key(&MapKey::String(":type".to_string())) {
                normalized_map.insert(
                    MapKey::String(":type".to_string()),
                    Expression::Literal(Literal::String("capability".to_string())),
                );
            }

            // Copy and normalize all fields
            for (key, value) in map {
                let key_str = map_key_to_string(key);
                let normalized_key = if key_str.starts_with(':') {
                    key_str.clone()
                } else {
                    format!(":{}", key_str)
                };
                normalized_map.insert(MapKey::String(normalized_key), value.clone());
            }

            // Validate if enabled
            if config.validate_after_normalization {
                let value_map: HashMap<MapKey, Value> = normalized_map
                    .iter()
                    .filter_map(|(k, v)| Some((k.clone(), expression_to_value_simple(v)?)))
                    .collect();

                if let Err(e) = CanonicalCapabilitySchema::validate(&value_map) {
                    return Err(RtfsBridgeError::InvalidObjectFormat {
                        message: format!("Capability map failed validation: {}", e),
                    });
                }
            }

            Ok(Expression::Map(normalized_map))
        }
        _ => Err(RtfsBridgeError::InvalidObjectFormat {
            message: format!(
                "Expected FunctionCall or Map for Capability normalization, got: {:?}",
                expr
            ),
        }),
    }
}

// Helper functions

fn map_key_to_string(key: &MapKey) -> String {
    match key {
        MapKey::String(s) => s.clone(),
        MapKey::Keyword(k) => format!(":{}", k.0),
        MapKey::Integer(i) => i.to_string(),
    }
}

/// Simple expression-to-value conversion for validation purposes
/// This is a simplified version that only handles basic types
pub fn expression_to_value_simple(expr: &Expression) -> Option<Value> {
    match expr {
        Expression::Literal(lit) => match lit {
            Literal::String(s) => Some(Value::String(s.clone())),
            Literal::Integer(i) => Some(Value::Integer(*i)),
            Literal::Float(f) => Some(Value::Float(*f)),
            Literal::Boolean(b) => Some(Value::Boolean(*b)),
            Literal::Nil => Some(Value::Nil),
            Literal::Keyword(k) => Some(Value::Keyword(k.clone())),
            _ => None,
        },
        Expression::Symbol(s) => Some(Value::Symbol(s.clone())),
        Expression::Vector(vec) => {
            let values: Option<Vec<Value>> = vec.iter().map(expression_to_value_simple).collect();
            values.map(Value::Vector)
        }
        Expression::Map(map) => {
            let value_map: Option<HashMap<MapKey, Value>> = map
                .iter()
                .map(|(k, v)| expression_to_value_simple(v).map(|val| (k.clone(), val)))
                .collect();
            value_map.map(Value::Map)
        }
        // For complex expressions (do, let, fn, calls, etc.), return a placeholder string.
        // Validation only checks presence/type for some fields; presence is enough for :body.
        _ => Some(Value::String("<expr>".to_string())),
    }
}
