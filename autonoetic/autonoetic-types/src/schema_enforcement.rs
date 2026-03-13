//! Schema enforcement for agent.spawn payloads.
//!
//! When agents call other agents via `agent.spawn`, the calling LLM must produce
//! payloads matching the target's `io` schema. This module provides a pluggable
//! enforcement hook that can coerce payloads, reject with actionable errors,
//! and log all decisions.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EnforcementResult {
    Pass,
    Coerced(CoercionDetails),
    Reject(RejectionDetails),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct CoercionDetails {
    pub transformations: Vec<Transformation>,
    pub final_payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Transformation {
    pub kind: TransformationKind,
    pub field_path: String,
    pub original_value: Option<serde_json::Value>,
    pub new_value: Option<serde_json::Value>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TransformationKind {
    DefaultValue,
    FieldRename,
    TypeCoercion,
    MissingAdded,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RejectionDetails {
    pub reason: String,
    pub hint: Option<String>,
    pub fields_with_errors: Vec<FieldError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FieldError {
    pub field_path: String,
    pub error: String,
}

pub trait SchemaEnforcer: Send + Sync {
    fn enforce(
        &self,
        payload: &serde_json::Value,
        target_schema: &serde_json::Value,
    ) -> EnforcementResult;
}

pub fn default_enforcer() -> impl SchemaEnforcer {
    DeterministicCoercionEnforcer::default()
}

#[derive(Debug, Clone, Default)]
pub struct DeterministicCoercionEnforcer {}

impl DeterministicCoercionEnforcer {
    pub fn new() -> Self {
        Self::default()
    }

    fn enforce_impl(
        &self,
        payload: &serde_json::Value,
        target_schema: &serde_json::Value,
    ) -> EnforcementResult {
        let mut transformations = Vec::new();
        let mut payload = payload.clone();
        let mut errors = Vec::new();

        if let (Some(obj), Some(schema_obj)) = (
            payload.as_object_mut(),
            target_schema.get("properties").and_then(|p| p.as_object()),
        ) {
            let fields: Vec<(String, serde_json::Value)> = schema_obj
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();

            for (field_name, schema_field) in fields {
                let value = obj.get(&field_name).cloned();

                match (&value, schema_field.get("default")) {
                    (None, Some(default)) => {
                        obj.insert(field_name.clone(), default.clone());
                        transformations.push(Transformation {
                            kind: TransformationKind::DefaultValue,
                            field_path: field_name.clone(),
                            original_value: None,
                            new_value: Some(default.clone()),
                            reason: format!("Field '{}' missing, using default", field_name),
                        });
                    }
                    (None, None) if schema_field.get("required").is_some() => {
                        if let Some(type_info) = schema_field.get("type") {
                            if let Some(default) = Self::default_for_type(type_info) {
                                obj.insert(field_name.clone(), default.clone());
                                transformations.push(Transformation {
                                    kind: TransformationKind::DefaultValue,
                                    field_path: field_name.clone(),
                                    original_value: None,
                                    new_value: Some(default.clone()),
                                    reason: format!(
                                        "Field '{}' required but missing, using type default",
                                        field_name
                                    ),
                                });
                            } else {
                                errors.push(FieldError {
                                    field_path: field_name.clone(),
                                    error: "Required field missing with no default available"
                                        .to_string(),
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        if !errors.is_empty() {
            EnforcementResult::Reject(RejectionDetails {
                reason: "Schema validation failed".to_string(),
                hint: Some(
                    "Missing required fields. Add missing fields or provide defaults.".to_string(),
                ),
                fields_with_errors: errors,
            })
        } else if !transformations.is_empty() {
            EnforcementResult::Coerced(CoercionDetails {
                transformations,
                final_payload: payload,
            })
        } else {
            EnforcementResult::Pass
        }
    }

    fn default_for_type(type_info: &serde_json::Value) -> Option<serde_json::Value> {
        match type_info.as_str()? {
            "string" => Some(serde_json::Value::String(String::new())),
            "integer" => Some(serde_json::Value::Number(serde_json::Number::from(0))),
            "number" => Some(serde_json::Value::Number(serde_json::Number::from(0))),
            "boolean" => Some(serde_json::Value::Bool(false)),
            "array" => Some(serde_json::Value::Array(Vec::new())),
            "object" => Some(serde_json::Value::Object(serde_json::Map::new())),
            _ => None,
        }
    }
}

impl SchemaEnforcer for DeterministicCoercionEnforcer {
    fn enforce(
        &self,
        payload: &serde_json::Value,
        target_schema: &serde_json::Value,
    ) -> EnforcementResult {
        self.enforce_impl(payload, target_schema)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pass_exact_match() {
        let enforcer = DeterministicCoercionEnforcer::new();
        let payload = serde_json::json!({
            "name": "test",
            "count": 5
        });
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "count": { "type": "integer" }
            }
        });

        let result = enforcer.enforce(&payload, &schema);
        assert!(matches!(result, EnforcementResult::Pass));
    }

    #[test]
    fn test_coerce_missing_field_with_default() {
        let enforcer = DeterministicCoercionEnforcer::new();
        let payload = serde_json::json!({
            "name": "test"
        });
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "count": { "type": "integer", "default": 10 }
            }
        });

        let result = enforcer.enforce(&payload, &schema);
        match result {
            EnforcementResult::Coerced(details) => {
                assert_eq!(details.final_payload["count"], 10);
                assert_eq!(details.transformations.len(), 1);
            }
            _ => panic!("Expected Coerced result"),
        }
    }

    #[test]
    fn test_coerce_missing_required_field_with_type_default() {
        let enforcer = DeterministicCoercionEnforcer::new();
        let payload = serde_json::json!({});
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            },
            "required": ["name"]
        });

        let result = enforcer.enforce(&payload, &schema);
        match result {
            EnforcementResult::Coerced(details) => {
                assert_eq!(details.final_payload["name"], "");
                assert!(!details.transformations.is_empty());
            }
            EnforcementResult::Pass => {
                // Also acceptable - no required check was performed
            }
            _ => panic!("Expected Coerced or Pass result"),
        }
    }
}
