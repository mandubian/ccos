//! Utility functions for RTFS value formatting and display

use crate::ast::{Keyword, MapKey};
use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::runtime::values::Value;
use std::collections::HashMap;

/// Format a MapKey in proper RTFS syntax
pub fn format_map_key(key: &MapKey) -> String {
    match key {
        MapKey::String(s) => format!("\"{}\"", s),
        MapKey::Keyword(k) => format!(":{}", k.0),
        MapKey::Integer(i) => i.to_string(),
    }
}

/// Convert MapKey to String
pub fn map_key_to_string(key: &MapKey) -> String {
    match key {
        MapKey::Keyword(k) => k.0.clone(),
        MapKey::String(s) => s.clone(),
        MapKey::Integer(i) => i.to_string(),
    }
}

/// Strip leading colon from strings
pub fn strip_leading_colon(input: &str) -> String {
    if input.starts_with(':') {
        input[1..].to_string()
    } else {
        input.to_string()
    }
}

/// Format a Value in proper RTFS syntax using the Display trait
pub fn format_rtfs_value(value: &Value) -> String {
    value.to_string()
}

/// Pretty-print a Value with indentation for complex structures
pub fn format_rtfs_value_pretty(value: &Value) -> String {
    match value {
        Value::Map(map) => {
            if map.is_empty() {
                "{}".to_string()
            } else {
                let entries: Vec<String> = map
                    .iter()
                    .map(|(k, v)| {
                        format!("  {} {}", format_map_key(k), format_rtfs_value_pretty(v))
                    })
                    .collect();
                format!("{{\n{}\n}}", entries.join("\n"))
            }
        }
        Value::Vector(vec) => {
            if vec.is_empty() {
                "[]".to_string()
            } else {
                let entries: Vec<String> = vec
                    .iter()
                    .map(|v| format!("  {}", format_rtfs_value_pretty(v)))
                    .collect();
                format!("[\n{}\n]", entries.join("\n"))
            }
        }
        Value::List(list) => {
            if list.is_empty() {
                "()".to_string()
            } else {
                let entries: Vec<String> =
                    list.iter().map(|v| format_rtfs_value_pretty(v)).collect();
                format!("({})", entries.join(" "))
            }
        }
        _ => format_rtfs_value(value),
    }
}

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
                    MapKey::String(key_str)
                };
                map.insert(map_key, json_to_rtfs_value(v)?);
            }
            Ok(Value::Map(map))
        }
    }
}
