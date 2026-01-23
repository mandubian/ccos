//! Argument normalization for capabilities.
//!
//! This module provides utilities to normalize positional arguments to map format
//! based on a TypeExpr::Map schema. This allows capabilities to use map-only schemas
//! while still accepting positional arguments for convenience.

use rtfs::ast::{MapKey, MapTypeEntry, TypeExpr};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::Value;
use std::collections::HashMap;

/// Normalize positional arguments to a map based on a TypeExpr::Map schema.
///
/// # Rules:
/// - If args is a single Map value that contains at least one expected field key, return it unchanged (passthrough).
/// - If args is a list/vector AND schema is TypeExpr::Map:
///   - Extract required fields (non-optional entries) in declaration order.
///   - If args length matches number of required fields, map positional args to field names.
///   - If schema has wildcard entries, normalization fails (ambiguous).
///   - If optional fields are not trailing (i.e., a required field follows an optional), reject.
/// - Optional fields must be passed via map syntax.
///
/// # Returns
/// - `Ok(Value::Map(...))` with the normalized map
/// - `Err(RuntimeError)` if normalization fails
///
/// # Examples
/// ```ignore
/// // Schema: {:handle Int :line String}
/// // (call cap 1 "hello") → {:handle 1 :line "hello"}
/// // (call cap {:handle 1 :line "hello"}) → passthrough
/// ```
pub fn normalize_args_to_map(args: Vec<Value>, schema: &TypeExpr) -> RuntimeResult<Value> {
    // Only handle Map schemas
    let (entries, wildcard) = match schema {
        TypeExpr::Map { entries, wildcard } => (entries, wildcard),
        TypeExpr::Union(_) => {
            return Err(RuntimeError::Generic(
                "Cannot normalize positional args to Union schema; use explicit map syntax"
                    .to_string(),
            ));
        }
        _ => {
            // Non-map schemas don't need normalization - just wrap in vector
            return Ok(Value::Vector(args));
        }
    };

    // Reject wildcard schemas - ambiguous positional mapping
    if wildcard.is_some() {
        return Err(RuntimeError::Generic(
            "Cannot normalize positional args to map schema with wildcard entries".to_string(),
        ));
    }

    // Validate optional fields are trailing only
    if !validate_trailing_optionals(entries) {
        return Err(RuntimeError::Generic(
            "Cannot normalize: optional fields must be trailing (after all required fields)"
                .to_string(),
        ));
    }

    // Extract required fields in declaration order
    let required_fields: Vec<&MapTypeEntry> = entries.iter().filter(|e| !e.optional).collect();
    let required_count = required_fields.len();

    // Case 1: Single map argument - check if passthrough
    if args.len() == 1 {
        if let Value::Map(ref map) = args[0] {
            if is_passthrough_map(map, entries) {
                return Ok(args.into_iter().next().unwrap());
            }
            // Single map arg but doesn't have expected keys - treat as positional for single-field schema
            if required_count == 1 {
                return normalize_positional_to_map(&args, &required_fields);
            }
            // Multi-field schema with map that doesn't match - error
            return Err(RuntimeError::Generic(format!(
                "Map argument missing required fields. Expected keys: [{}]",
                field_names_string(&required_fields)
            )));
        }
    }

    // Case 2: Zero args with optional-only schema → empty map
    if args.is_empty() && required_count == 0 {
        return Ok(Value::Map(HashMap::new()));
    }

    // Case 3: Zero args with required fields → error
    if args.is_empty() && required_count > 0 {
        return Err(RuntimeError::Generic(format!(
            "Missing required arguments. Expected {} positional args for fields: [{}]",
            required_count,
            field_names_string(&required_fields)
        )));
    }

    // Case 4: Positional args with optional-only schema → error (must use map)
    if !args.is_empty() && required_count == 0 {
        return Err(RuntimeError::Generic(
            "Schema has only optional fields; positional args not supported. Use map syntax."
                .to_string(),
        ));
    }

    // Case 5: Positional args - normalize to map
    if args.len() == required_count {
        return normalize_positional_to_map(&args, &required_fields);
    }

    // Wrong number of positional args
    Err(RuntimeError::Generic(format!(
        "Expected {} positional args for fields [{}], or a map with those keys. Got {} args.",
        required_count,
        field_names_string(&required_fields),
        args.len()
    )))
}

/// Check if optional fields are all trailing (after required fields)
fn validate_trailing_optionals(entries: &[MapTypeEntry]) -> bool {
    let mut seen_optional = false;
    for entry in entries {
        if entry.optional {
            seen_optional = true;
        } else if seen_optional {
            // Required field after optional - invalid
            return false;
        }
    }
    true
}

/// Check if a map value should be treated as passthrough (already has expected keys)
fn is_passthrough_map(map: &HashMap<MapKey, Value>, entries: &[MapTypeEntry]) -> bool {
    // If the schema has no required fields (i.e., optional-only), any map is valid passthrough.
    //
    // This is important for many OpenAPI-derived capabilities where all query params are optional,
    // but callers still need to pass a map containing those optional keys.
    let has_required_fields = entries.iter().any(|e| !e.optional);
    if !has_required_fields {
        return true;
    }

    // If any required field key is present, treat as passthrough
    entries
        .iter()
        .filter(|e| !e.optional)
        .any(|e| map.contains_key(&MapKey::Keyword(e.key.clone())))
}

/// Convert positional args to a map based on required fields in declaration order
fn normalize_positional_to_map(
    args: &[Value],
    required_fields: &[&MapTypeEntry],
) -> RuntimeResult<Value> {
    let mut result = HashMap::new();

    for (i, entry) in required_fields.iter().enumerate() {
        let key = MapKey::Keyword(entry.key.clone());
        let value = args.get(i).cloned().ok_or_else(|| {
            RuntimeError::Generic(format!(
                "Missing positional argument for field :{}",
                entry.key.0
            ))
        })?;
        result.insert(key, value);
    }

    Ok(Value::Map(result))
}

/// Format field names for error messages
fn field_names_string(fields: &[&MapTypeEntry]) -> String {
    fields
        .iter()
        .map(|e| format!(":{}", e.key.0))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtfs::ast::{Keyword, PrimitiveType};

    fn make_map_schema(fields: &[(&str, bool)]) -> TypeExpr {
        TypeExpr::Map {
            entries: fields
                .iter()
                .map(|(name, optional)| MapTypeEntry {
                    key: Keyword(name.to_string()),
                    value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                    optional: *optional,
                })
                .collect(),
            wildcard: None,
        }
    }

    #[test]
    fn test_normalize_positional_to_map_two_fields() {
        let schema = make_map_schema(&[("handle", false), ("line", false)]);
        let args = vec![Value::Integer(1), Value::String("hello".into())];

        let result = normalize_args_to_map(args, &schema).unwrap();

        match result {
            Value::Map(map) => {
                assert_eq!(
                    map.get(&MapKey::Keyword(Keyword("handle".into()))),
                    Some(&Value::Integer(1))
                );
                assert_eq!(
                    map.get(&MapKey::Keyword(Keyword("line".into()))),
                    Some(&Value::String("hello".into()))
                );
            }
            _ => panic!("Expected Map"),
        }
    }

    #[test]
    fn test_normalize_map_passthrough() {
        let schema = make_map_schema(&[("handle", false), ("line", false)]);
        let mut input_map = HashMap::new();
        input_map.insert(MapKey::Keyword(Keyword("handle".into())), Value::Integer(1));
        input_map.insert(
            MapKey::Keyword(Keyword("line".into())),
            Value::String("hello".into()),
        );
        let args = vec![Value::Map(input_map.clone())];

        let result = normalize_args_to_map(args, &schema).unwrap();

        match result {
            Value::Map(map) => {
                assert_eq!(map.len(), 2);
                assert_eq!(
                    map.get(&MapKey::Keyword(Keyword("handle".into()))),
                    Some(&Value::Integer(1))
                );
            }
            _ => panic!("Expected Map"),
        }
    }

    #[test]
    fn test_normalize_single_arg_single_field() {
        let schema = make_map_schema(&[("path", false)]);
        let args = vec![Value::String("/tmp/foo.txt".into())];

        let result = normalize_args_to_map(args, &schema).unwrap();

        match result {
            Value::Map(map) => {
                assert_eq!(map.len(), 1);
                assert_eq!(
                    map.get(&MapKey::Keyword(Keyword("path".into()))),
                    Some(&Value::String("/tmp/foo.txt".into()))
                );
            }
            _ => panic!("Expected Map"),
        }
    }

    #[test]
    fn test_normalize_wrong_arg_count_error() {
        let schema = make_map_schema(&[("handle", false), ("line", false)]);
        let args = vec![Value::Integer(1)]; // Missing second arg

        let result = normalize_args_to_map(args, &schema);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Expected 2 positional args"));
    }

    #[test]
    fn test_normalize_wildcard_schema_error() {
        let schema = TypeExpr::Map {
            entries: vec![MapTypeEntry {
                key: Keyword("id".into()),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
                optional: false,
            }],
            wildcard: Some(Box::new(TypeExpr::Any)),
        };
        let args = vec![Value::Integer(1)];

        let result = normalize_args_to_map(args, &schema);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("wildcard"));
    }

    #[test]
    fn test_normalize_optional_only_zero_args() {
        let schema = make_map_schema(&[("timeout", true), ("retries", true)]);
        let args = vec![];

        let result = normalize_args_to_map(args, &schema).unwrap();

        match result {
            Value::Map(map) => {
                assert!(map.is_empty());
            }
            _ => panic!("Expected empty Map"),
        }
    }

    #[test]
    fn test_normalize_optional_only_with_positional_error() {
        let schema = make_map_schema(&[("timeout", true), ("retries", true)]);
        let args = vec![Value::Integer(5)];

        let result = normalize_args_to_map(args, &schema);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("only optional fields"));
    }

    #[test]
    fn test_normalize_mixed_optional_error() {
        // Optional field between required fields - should reject
        let schema = TypeExpr::Map {
            entries: vec![
                MapTypeEntry {
                    key: Keyword("a".into()),
                    value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
                    optional: false,
                },
                MapTypeEntry {
                    key: Keyword("b".into()),
                    value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                    optional: true, // Optional in the middle
                },
                MapTypeEntry {
                    key: Keyword("c".into()),
                    value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
                    optional: false, // Required after optional
                },
            ],
            wildcard: None,
        };
        let args = vec![Value::Integer(1), Value::Integer(2)];

        let result = normalize_args_to_map(args, &schema);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("optional fields must be trailing"));
    }

    #[test]
    fn test_normalize_union_schema_error() {
        let schema = TypeExpr::Union(vec![
            TypeExpr::Primitive(PrimitiveType::Int),
            TypeExpr::Primitive(PrimitiveType::String),
        ]);
        let args = vec![Value::Integer(1)];

        let result = normalize_args_to_map(args, &schema);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Union schema"));
    }

    #[test]
    fn test_normalize_single_map_field_disambiguation() {
        // Schema: {:data Map}
        let schema = TypeExpr::Map {
            entries: vec![MapTypeEntry {
                key: Keyword("data".into()),
                value_type: Box::new(TypeExpr::Map {
                    entries: vec![],
                    wildcard: Some(Box::new(TypeExpr::Any)),
                }),
                optional: false,
            }],
            wildcard: None,
        };

        // Input: {:foo 1 :bar 2} - should normalize to {:data {:foo 1 :bar 2}}
        let mut input_map = HashMap::new();
        input_map.insert(MapKey::Keyword(Keyword("foo".into())), Value::Integer(1));
        input_map.insert(MapKey::Keyword(Keyword("bar".into())), Value::Integer(2));
        let args = vec![Value::Map(input_map)];

        let result = normalize_args_to_map(args, &schema).unwrap();

        match result {
            Value::Map(map) => {
                assert!(map.contains_key(&MapKey::Keyword(Keyword("data".into()))));
            }
            _ => panic!("Expected Map with :data key"),
        }
    }
}
