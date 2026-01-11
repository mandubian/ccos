//! Canonical schemas for Plan and Capability RTFS maps
//!
//! This module defines the canonical field names, types, and validation rules
//! for Plan and Capability objects in RTFS. All function-call syntax should
//! normalize to these canonical map structures.

use rtfs::ast::{Expression, MapKey};
use rtfs::runtime::values::Value;
use std::collections::HashMap;

/// Canonical Plan map schema
///
/// A Plan is represented as a map with these canonical fields:
/// - `:type` (String): Always "plan"
/// - `:name` (String): Plan name/identifier
/// - `:language` (String|Symbol): Language identifier (e.g., "rtfs20", :rtfs20)
/// - `:body` (Expression): The executable plan body (RTFS code or WASM)
/// - `:input-schema` (Map<Keyword, TypeExpr>): Input parameter schema
/// - `:output-schema` (Map<Keyword, TypeExpr>): Output value schema
/// - `:capabilities-required` (Vector<String>): List of capability IDs this plan depends on
/// - `:annotations` (Map<String, Value>): Provenance and metadata
/// - `:intent-ids` (Vector<String>): Associated intent identifiers (optional)
/// - `:policies` (Map<String, Value>): Execution policies (optional)
pub struct CanonicalPlanSchema;

impl CanonicalPlanSchema {
    /// Required fields for a canonical Plan map
    pub const REQUIRED_FIELDS: &'static [&'static str] = &[":type", ":name", ":body"];

    /// Optional fields for a canonical Plan map
    pub const OPTIONAL_FIELDS: &'static [&'static str] = &[
        ":language",
        ":input-schema",
        ":output-schema",
        ":capabilities-required",
        ":annotations",
        ":intent-ids",
        ":policies",
    ];

    /// Validate that a map represents a canonical Plan structure
    pub fn validate(plan_map: &HashMap<MapKey, Value>) -> Result<(), String> {
        // Check required fields
        for field in Self::REQUIRED_FIELDS {
            let key = MapKey::String(field.to_string());
            if !plan_map.contains_key(&key) {
                return Err(format!("Plan missing required field: {}", field));
            }
        }

        // Validate :type field
        if let Some(Value::String(type_val)) = plan_map.get(&MapKey::String(":type".to_string())) {
            if type_val != "plan" {
                return Err(format!("Plan :type must be 'plan', got: {}", type_val));
            }
        } else {
            return Err("Plan :type must be a string".to_string());
        }

        // Validate :name field
        if let Some(Value::String(_)) = plan_map.get(&MapKey::String(":name".to_string())) {
            // OK
        } else {
            return Err("Plan :name must be a string".to_string());
        }

        // Validate :body field exists (type checked later)
        if !plan_map.contains_key(&MapKey::String(":body".to_string())) {
            return Err("Plan :body is required".to_string());
        }

        // Validate :language if present
        if let Some(lang_val) = plan_map.get(&MapKey::String(":language".to_string())) {
            match lang_val {
                Value::String(_) | Value::Symbol(_) | Value::Keyword(_) => {
                    // OK
                }
                _ => {
                    return Err("Plan :language must be string, symbol, or keyword".to_string());
                }
            }
        }

        // Validate :input-schema and :output-schema if present (should be maps)
        // Use TypeExpr-aware validation
        for schema_field in &[":input-schema", ":output-schema"] {
            if let Some(schema_val) = plan_map.get(&MapKey::String(schema_field.to_string())) {
                if !matches!(schema_val, Value::Map(_)) {
                    return Err(format!("Plan {} must be a map", schema_field));
                }
                // Additional TypeExpr validation would be done by validators module
            }
        }

        // Validate :capabilities-required if present (should be vector)
        if let Some(caps_val) = plan_map.get(&MapKey::String(":capabilities-required".to_string()))
        {
            if !matches!(caps_val, Value::Vector(_) | Value::List(_)) {
                return Err("Plan :capabilities-required must be a vector or list".to_string());
            }
        }

        Ok(())
    }

    /// Extract canonical Plan map from a function call expression
    ///
    /// Normalizes `(plan "name" :body ...)` or `(ccos/plan "name" :body ...)`
    /// to canonical map format.
    pub fn from_function_call(
        callee: &Expression,
        arguments: &[Expression],
    ) -> Result<HashMap<MapKey, Value>, String> {
        // Check callee is "plan" or "ccos/plan"
        let callee_name = match callee {
            Expression::Symbol(s) => &s.0,
            _ => return Err("Plan callee must be a symbol".to_string()),
        };

        if callee_name != "plan" && callee_name != "ccos/plan" {
            return Err(format!(
                "Expected 'plan' or 'ccos/plan', got: {}",
                callee_name
            ));
        }

        // First argument should be name (string)
        let name =
            if let Some(Expression::Literal(rtfs::ast::Literal::String(s))) = arguments.first() {
                s.clone()
            } else {
                return Err("Plan name must be a string literal".to_string());
            };

        // Build canonical map
        let mut plan_map = HashMap::new();
        plan_map.insert(
            MapKey::String(":type".to_string()),
            Value::String("plan".to_string()),
        );
        plan_map.insert(MapKey::String(":name".to_string()), Value::String(name));

        // Parse remaining keyword arguments
        let mut i = 1;
        while i < arguments.len() {
            if let Expression::Literal(rtfs::ast::Literal::Keyword(k)) = &arguments[i] {
                let key = format!(":{}", k.0);
                if i + 1 >= arguments.len() {
                    return Err(format!("Plan property '{}' requires a value", key));
                }
                // Convert expression to value (simplified - would need proper evaluation)
                // For now, we'll handle this in the extractor
                i += 2;
            } else {
                return Err(format!(
                    "Plan properties must be keyword-value pairs, got: {:?}",
                    arguments[i]
                ));
            }
        }

        Ok(plan_map)
    }
}

/// Canonical Capability map schema
///
/// A Capability is represented as a map with these canonical fields:
/// - `:type` (String): Always "capability" (optional if top-level form)
/// - `:id` (String): Capability identifier (preferred for immutability)
/// - `:name` (String): Human-readable name
/// - `:version` (String): Semantic version (e.g., "1.0.0")
/// - `:description` (String): Purpose description
/// - `:input-schema` (Map<Keyword, TypeExpr>): Input parameter schema
/// - `:output-schema` (Map<Keyword, TypeExpr>): Output value schema
/// - `:implementation` (Expression): For local RTFS capabilities: `(fn [input] ...)`
/// - `:language` (String|Symbol): Language for local implementation (e.g., "rtfs20")
/// - `:provider` (String): Provider type ("Local", "MCP", "Http", etc.)
/// - `:provider-meta` (Map): Provider-specific metadata
/// - `:permissions` (Vector<String>): Required permissions
/// - `:effects` (Vector<String>): Side effects this capability may have
/// - `:metadata` (Map<String, Value>): Additional metadata
/// - `:attestation` (String): Cryptographic signature (optional)
/// - `:provenance` (Map): Source/provenance information (optional)
pub struct CanonicalCapabilitySchema;

impl CanonicalCapabilitySchema {
    /// Required fields for a canonical Capability map
    pub const REQUIRED_FIELDS: &'static [&'static str] =
        &[":id", ":name", ":version", ":description"];

    /// Optional fields for a canonical Capability map
    pub const OPTIONAL_FIELDS: &'static [&'static str] = &[
        ":type",
        ":input-schema",
        ":output-schema",
        ":implementation",
        ":language",
        ":provider",
        ":provider-meta",
        ":permissions",
        ":effects",
        ":metadata",
        ":attestation",
        ":provenance",
    ];

    /// Validate that a map represents a canonical Capability structure
    pub fn validate(cap_map: &HashMap<MapKey, Value>) -> Result<(), String> {
        // Check required fields
        for field in Self::REQUIRED_FIELDS {
            let key = MapKey::String(field.to_string());
            if !cap_map.contains_key(&key) {
                return Err(format!("Capability missing required field: {}", field));
            }
        }

        // Validate :type field if present
        if let Some(Value::String(type_val)) = cap_map.get(&MapKey::String(":type".to_string())) {
            if type_val != "capability" {
                return Err(format!(
                    "Capability :type must be 'capability', got: {}",
                    type_val
                ));
            }
        }

        // Validate required string fields
        for field in Self::REQUIRED_FIELDS {
            if let Some(Value::String(_)) = cap_map.get(&MapKey::String(field.to_string())) {
                // OK
            } else {
                return Err(format!("Capability {} must be a string", field));
            }
        }

        // Validate :input-schema and :output-schema if present (should be maps, vectors, lists, strings, or keywords)
        for schema_field in &[":input-schema", ":output-schema"] {
            if let Some(schema_val) = cap_map.get(&MapKey::String(schema_field.to_string())) {
                if !matches!(
                    schema_val,
                    Value::Map(_)
                        | Value::Vector(_)
                        | Value::List(_)
                        | Value::String(_)
                        | Value::Keyword(_)
                ) {
                    return Err(format!(
                        "Capability {} must be a map, vector, list, string, or keyword type expression",
                        schema_field
                    ));
                }
            }
        }

        // Validate :implementation if present (should be a function expression)
        // Type checking happens at evaluation time

        // Validate :language if present
        if let Some(lang_val) = cap_map.get(&MapKey::String(":language".to_string())) {
            let lang_str = match lang_val {
                Value::String(s) => Some(s.clone()),
                Value::Keyword(k) => Some(format!(":{}", k.0)),
                Value::Symbol(rtfs::ast::Symbol(s)) => Some(s.clone()),
                _ => {
                    return Err(
                        "Capability :language must be string, symbol, or keyword".to_string()
                    );
                }
            };

            // Validate language string format
            if let Some(lang) = lang_str {
                use super::language_utils::validate_language_string;
                if let Err(e) = validate_language_string(&lang) {
                    return Err(format!("Invalid capability :language: {}", e));
                }
            }
        }

        // Validate that local capabilities have :language, but only if the
        // :provider field is explicitly present and set to "Local". Do not
        // assume a missing provider implies Local.
        if let Some(provider_val) = cap_map.get(&MapKey::String(":provider".to_string())) {
            let provider = match provider_val {
                Value::String(s) => s.clone(),
                Value::Keyword(k) => format!(":{}", k.0),
                Value::Symbol(rtfs::ast::Symbol(s)) => s.clone(),
                _ => "".to_string(),
            };

            if provider.to_lowercase() == "local" {
                use super::language_utils::validate_local_capability_has_language;
                if let Err(e) = validate_local_capability_has_language(cap_map) {
                    return Err(format!("Local capability validation failed: {}", e));
                }
            }
        }

        // Validate :permissions and :effects if present (should be vectors)
        for vec_field in &[":permissions", ":effects"] {
            if let Some(vec_val) = cap_map.get(&MapKey::String(vec_field.to_string())) {
                if !matches!(vec_val, Value::Vector(_) | Value::List(_)) {
                    return Err(format!("Capability {} must be a vector or list", vec_field));
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canonical_plan_schema_validation() {
        let mut plan_map = HashMap::new();
        plan_map.insert(
            MapKey::String(":type".to_string()),
            Value::String("plan".to_string()),
        );
        plan_map.insert(
            MapKey::String(":name".to_string()),
            Value::String("test-plan".to_string()),
        );
        plan_map.insert(
            MapKey::String(":body".to_string()),
            Value::String("(do (step \"test\" {}))".to_string()),
        );

        assert!(CanonicalPlanSchema::validate(&plan_map).is_ok());
    }

    #[test]
    fn test_canonical_plan_missing_required_field() {
        let mut plan_map = HashMap::new();
        plan_map.insert(
            MapKey::String(":type".to_string()),
            Value::String("plan".to_string()),
        );
        // Missing :name and :body

        assert!(CanonicalPlanSchema::validate(&plan_map).is_err());
    }

    #[test]
    fn test_canonical_capability_schema_validation() {
        use rtfs::ast::MapKey;
        use rtfs::runtime::values::Value;

        let mut cap_map = HashMap::new();
        cap_map.insert(
            MapKey::String(":id".to_string()),
            Value::String("test.cap".to_string()),
        );
        cap_map.insert(
            MapKey::String(":name".to_string()),
            Value::String("Test Capability".to_string()),
        );
        cap_map.insert(
            MapKey::String(":version".to_string()),
            Value::String("1.0.0".to_string()),
        );
        cap_map.insert(
            MapKey::String(":description".to_string()),
            Value::String("A test capability".to_string()),
        );
        cap_map.insert(
            MapKey::String(":language".to_string()),
            Value::String("rust".to_string()),
        );

        let result = CanonicalCapabilitySchema::validate(&cap_map);
        if let Err(e) = &result {
            eprintln!("Validation error: {}", e);
        }
        assert!(result.is_ok());
    }
}
