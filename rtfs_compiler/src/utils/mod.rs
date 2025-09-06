//! Utility functions for RTFS value formatting and display

use crate::runtime::values::Value;
use crate::ast::MapKey;

/// Format a MapKey in proper RTFS syntax
pub fn format_map_key(key: &MapKey) -> String {
    match key {
        MapKey::String(s) => format!("\"{}\"", s),
        MapKey::Keyword(k) => format!(":{}", k.0),
        MapKey::Integer(i) => i.to_string(),
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
                let entries: Vec<String> = map.iter()
                    .map(|(k, v)| format!("  {} {}", format_map_key(k), format_rtfs_value_pretty(v)))
                    .collect();
                format!("{{\n{}\n}}", entries.join("\n"))
            }
        }
        Value::Vector(vec) => {
            if vec.is_empty() {
                "[]".to_string()
            } else {
                let entries: Vec<String> = vec.iter()
                    .map(|v| format!("  {}", format_rtfs_value_pretty(v)))
                    .collect();
                format!("[\n{}\n]", entries.join("\n"))
            }
        }
        Value::List(list) => {
            if list.is_empty() {
                "()".to_string()
            } else {
                let entries: Vec<String> = list.iter()
                    .map(|v| format_rtfs_value_pretty(v))
                    .collect();
                format!("({})", entries.join(" "))
            }
        }
        _ => format_rtfs_value(value)
    }
}