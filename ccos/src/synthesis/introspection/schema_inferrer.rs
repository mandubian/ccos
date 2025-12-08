//! Schema inference from runtime values.
//!
//! This module provides functionality to infer RTFS schema expressions
//! from actual runtime values, enabling a posteriori schema refinement
//! for synthesized capabilities.

use rtfs::ast::MapKey;
use rtfs::runtime::values::Value;
use std::collections::{HashMap, HashSet};

/// Infer an RTFS schema string from a runtime Value.
///
/// This converts actual values into schema forms like:
/// - `int?` for integers
/// - `string?` for strings
/// - `[:vector int?]` for homogeneous vectors
/// - `[:map [:key1 string?] [:key2 int?]]` for maps
///
/// For empty collections or heterogeneous data, uses `:any`.
pub fn infer_schema_from_value(value: &Value) -> String {
    infer_schema_impl(value, 0)
}

/// Infer schema with depth tracking to prevent infinite recursion.
fn infer_schema_impl(value: &Value, depth: usize) -> String {
    // Limit recursion depth to avoid stack overflow on deeply nested structures
    if depth > 10 {
        return ":any".to_string();
    }

    match value {
        Value::Nil => "nil?".to_string(),
        Value::Boolean(_) => "boolean?".to_string(),
        Value::Integer(_) => "int?".to_string(),
        Value::Float(_) => "float?".to_string(),
        Value::String(_) => "string?".to_string(),
        Value::Timestamp(_) => "string?".to_string(), // Timestamps are strings
        Value::Uuid(_) => "string?".to_string(),      // UUIDs are strings
        Value::ResourceHandle(_) => "string?".to_string(),
        Value::Symbol(_) => "symbol?".to_string(),
        Value::Keyword(_) => "keyword?".to_string(),
        Value::Vector(vec) => infer_vector_schema(vec, depth),
        Value::List(list) => infer_vector_schema(list, depth), // Treat lists as vectors
        Value::Map(map) => infer_map_schema(map, depth),
        Value::Function(_) | Value::FunctionPlaceholder(_) => "fn?".to_string(),
        Value::Error(_) => ":any".to_string(),
    }
}

/// Infer schema for a vector by analyzing element types.
fn infer_vector_schema(vec: &[Value], depth: usize) -> String {
    if vec.is_empty() {
        return "[:vector :any]".to_string();
    }

    // Sample up to 10 elements to determine element type
    let sample_size = vec.len().min(10);
    let mut element_schemas: HashSet<String> = HashSet::new();

    for item in vec.iter().take(sample_size) {
        element_schemas.insert(infer_schema_impl(item, depth + 1));
    }

    if element_schemas.len() == 1 {
        // Homogeneous vector
        let elem_schema = element_schemas.into_iter().next().unwrap();
        format!("[:vector {}]", elem_schema)
    } else {
        // Heterogeneous vector - use :any for elements
        "[:vector :any]".to_string()
    }
}

/// Infer schema for a map by analyzing key-value pairs.
fn infer_map_schema(map: &HashMap<MapKey, Value>, depth: usize) -> String {
    if map.is_empty() {
        return "[:map]".to_string();
    }

    // Collect key schemas
    let mut key_schemas: Vec<String> = Vec::new();

    for (key, value) in map {
        let key_name = map_key_to_string(key);
        let value_schema = infer_schema_impl(value, depth + 1);
        key_schemas.push(format!("[:{} {}]", key_name, value_schema));
    }

    // Sort for consistent output
    key_schemas.sort();

    format!("[:map {}]", key_schemas.join(" "))
}

/// Convert a MapKey to a string suitable for schema representation.
fn map_key_to_string(key: &MapKey) -> String {
    match key {
        MapKey::Keyword(kw) => kw.0.clone(),
        MapKey::String(s) => s.clone(),
        MapKey::Integer(i) => i.to_string(),
    }
}

/// Infer a schema from a JSON value (for external data).
///
/// This is useful for inferring schemas from MCP responses or other
/// JSON-based data sources.
pub fn infer_schema_from_json(json: &serde_json::Value) -> String {
    match json {
        serde_json::Value::Null => "nil?".to_string(),
        serde_json::Value::Bool(_) => "boolean?".to_string(),
        serde_json::Value::Number(n) => {
            if n.is_i64() {
                "int?".to_string()
            } else {
                "float?".to_string()
            }
        }
        serde_json::Value::String(_) => "string?".to_string(),
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                "[:vector :any]".to_string()
            } else {
                // Sample first element
                let elem_schema = infer_schema_from_json(&arr[0]);
                format!("[:vector {}]", elem_schema)
            }
        }
        serde_json::Value::Object(obj) => {
            if obj.is_empty() {
                "[:map]".to_string()
            } else {
                let mut key_schemas: Vec<String> = obj
                    .iter()
                    .map(|(k, v)| format!("[:{} {}]", k, infer_schema_from_json(v)))
                    .collect();
                key_schemas.sort();
                format!("[:map {}]", key_schemas.join(" "))
            }
        }
    }
}

/// Check if a schema is generic (uses :any).
pub fn is_generic_schema(schema: &str) -> bool {
    schema == ":any" || schema == "[:vector :any]" || schema == "[:map]"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_primitives() {
        assert_eq!(infer_schema_from_value(&Value::Integer(42)), "int?");
        assert_eq!(
            infer_schema_from_value(&Value::String("hello".into())),
            "string?"
        );
        assert_eq!(infer_schema_from_value(&Value::Boolean(true)), "boolean?");
        assert_eq!(infer_schema_from_value(&Value::Float(3.14)), "float?");
    }

    #[test]
    fn test_infer_vector() {
        let vec = Value::Vector(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
        ]);
        assert_eq!(infer_schema_from_value(&vec), "[:vector int?]");
    }

    #[test]
    fn test_infer_empty_vector() {
        let vec = Value::Vector(vec![]);
        assert_eq!(infer_schema_from_value(&vec), "[:vector :any]");
    }

    #[test]
    fn test_infer_map() {
        use rtfs::ast::Keyword;
        let mut map = HashMap::new();
        map.insert(MapKey::Keyword(Keyword("count".into())), Value::Integer(5));
        map.insert(
            MapKey::Keyword(Keyword("name".into())),
            Value::String("test".into()),
        );

        let schema = infer_schema_from_value(&Value::Map(map));
        // Schema should contain both keys (order may vary due to sorting)
        assert!(schema.contains("[:count int?]"));
        assert!(schema.contains("[:name string?]"));
    }

    #[test]
    fn test_is_generic_schema() {
        assert!(is_generic_schema(":any"));
        assert!(is_generic_schema("[:vector :any]"));
        assert!(is_generic_schema("[:map]"));
        assert!(!is_generic_schema("int?"));
        assert!(!is_generic_schema("[:vector int?]"));
    }
}
