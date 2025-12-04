//! Value conversion utilities for RTFS ↔ JSON
//!
//! Provides shared functions for converting between RTFS Value types and serde_json::Value.
//! This module consolidates duplicate conversion logic scattered across the codebase.

use rtfs::ast::{Keyword, MapKey};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use std::collections::HashMap;

/// Convert RTFS Value to serde_json::Value
///
/// Handles all RTFS value types including Nil, primitives, collections, and special types.
/// For types that cannot be serialized (functions, errors), returns an error.
pub fn rtfs_value_to_json(value: &Value) -> RuntimeResult<serde_json::Value> {
    match value {
        Value::Nil => Ok(serde_json::Value::Null),
        Value::Boolean(b) => Ok(serde_json::Value::Bool(*b)),
        Value::Integer(i) => Ok(serde_json::Value::Number(serde_json::Number::from(*i))),
        Value::Float(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .ok_or_else(|| RuntimeError::Generic("Invalid float value for JSON".to_string())),
        Value::String(s) => Ok(serde_json::Value::String(s.clone())),
        Value::Keyword(k) => {
            // Strip leading colon for JSON
            let s = &k.0;
            Ok(serde_json::Value::String(if s.starts_with(':') {
                s[1..].to_string()
            } else {
                s.clone()
            }))
        }
        Value::Symbol(s) => Ok(serde_json::Value::String(s.0.clone())),
        Value::Vector(v) => {
            let json_array: Result<Vec<serde_json::Value>, RuntimeError> =
                v.iter().map(rtfs_value_to_json).collect();
            Ok(serde_json::Value::Array(json_array?))
        }
        Value::List(l) => {
            // Convert List to Array (same as Vector)
            let json_array: Result<Vec<serde_json::Value>, RuntimeError> =
                l.iter().map(rtfs_value_to_json).collect();
            Ok(serde_json::Value::Array(json_array?))
        }
        Value::Map(m) => {
            let mut json_obj = serde_json::Map::new();
            for (key, val) in m.iter() {
                let key_str = map_key_to_string(key);
                json_obj.insert(key_str, rtfs_value_to_json(val)?);
            }
            Ok(serde_json::Value::Object(json_obj))
        }
        Value::Timestamp(ts) => Ok(serde_json::Value::String(format!("@{}", ts))),
        Value::Uuid(uuid) => Ok(serde_json::Value::String(format!("@{}", uuid))),
        Value::ResourceHandle(handle) => Ok(serde_json::Value::String(format!("@{}", handle))),
        Value::Function(_) => Err(RuntimeError::Generic(
            "Cannot serialize functions to JSON".to_string(),
        )),
        Value::FunctionPlaceholder(_) => Err(RuntimeError::Generic(
            "Cannot serialize function placeholders to JSON".to_string(),
        )),
        Value::Error(e) => Err(RuntimeError::Generic(format!(
            "Cannot serialize errors to JSON: {}",
            e.message
        ))),
    }
}

/// Convert serde_json::Value to RTFS Value
///
/// Handles all JSON value types and converts them to appropriate RTFS types.
/// Object keys starting with ':' are converted to RTFS Keywords.
pub fn json_to_rtfs_value(json: &serde_json::Value) -> RuntimeResult<Value> {
    match json {
        serde_json::Value::Null => Ok(Value::Nil),
        serde_json::Value::Bool(b) => Ok(Value::Boolean(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Value::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(Value::Float(f))
            } else {
                // Fallback: convert to string if number is too large
                Ok(Value::String(n.to_string()))
            }
        }
        serde_json::Value::String(s) => Ok(Value::String(s.clone())),
        serde_json::Value::Array(arr) => {
            let values: Result<Vec<Value>, RuntimeError> =
                arr.iter().map(json_to_rtfs_value).collect();
            Ok(Value::Vector(values?))
        }
        serde_json::Value::Object(obj) => {
            let mut map = HashMap::new();
            for (k, v) in obj.iter() {
                // Use keyword for object keys that start with ':'
                // Otherwise use string key
                let key_str = strip_leading_colon(k);
                let map_key = if k.starts_with(':') {
                    MapKey::Keyword(Keyword(key_str))
                } else {
                    MapKey::String(k.clone())
                };
                map.insert(map_key, json_to_rtfs_value(v)?);
            }
            Ok(Value::Map(map))
        }
    }
}

/// Convert MapKey to String representation
///
/// Used for converting RTFS map keys to JSON object keys.
pub fn map_key_to_string(key: &MapKey) -> String {
    match key {
        MapKey::String(s) => s.clone(),
        MapKey::Keyword(k) => {
            let s = &k.0;
            // Strip leading colon for JSON keys
            if s.starts_with(':') {
                s[1..].to_string()
            } else {
                s.clone()
            }
        }
        MapKey::Integer(i) => i.to_string(),
    }
}

/// Strip leading colon from a string
///
/// Helper function for handling keyword strings that may or may not have a leading colon.
pub fn strip_leading_colon(input: &str) -> String {
    input.trim_start_matches(':').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rtfs_to_json_basic() {
        let rtfs_val = Value::Integer(42);
        let json_val = rtfs_value_to_json(&rtfs_val).unwrap();
        assert_eq!(json_val, serde_json::json!(42));
    }

    #[test]
    fn test_json_to_rtfs_basic() {
        let json_val = serde_json::json!(42);
        let rtfs_val = json_to_rtfs_value(&json_val).unwrap();
        assert!(matches!(rtfs_val, Value::Integer(42)));
    }

    #[test]
    fn test_map_conversion() {
        let mut map = HashMap::new();
        map.insert(
            MapKey::String("key".to_string()),
            Value::String("value".to_string()),
        );
        let rtfs_val = Value::Map(map);
        let json_val = rtfs_value_to_json(&rtfs_val).unwrap();
        assert_eq!(json_val["key"], "value");
    }

    #[test]
    fn test_keyword_stripping() {
        let keyword = Value::Keyword(Keyword(":test".to_string()));
        let json_val = rtfs_value_to_json(&keyword).unwrap();
        assert_eq!(json_val.as_str(), Some("test"));
    }
}
