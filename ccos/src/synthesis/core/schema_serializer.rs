//! Shared utilities for serializing TypeExpr to RTFS schema format
//!
//! This module provides common functions for converting RTFS TypeExpr AST nodes
//! into human-readable RTFS schema strings for capability definitions.

use crate::utils::value_conversion;
use rtfs::ast::TypeExpr;

/// Convert TypeExpr to compact RTFS schema string (uses built-in Display)
///
/// This is suitable for simple inline schemas and uses the TypeExpr's Display trait.
/// Format: `[:vector :string]`, `[[:key type] [:key2 type?]]`
pub fn type_expr_to_rtfs_compact(expr: &TypeExpr) -> String {
    expr.to_string()
}

/// Convert TypeExpr to human-readable RTFS schema string with formatting
///
/// This is suitable for capability definitions where we want:
/// - Maps formatted as `{ :key type }` instead of `[[:key type]]`
/// - Optional fields marked with `;;optional` comments
/// - Multiline formatting for readability
///
/// # Example
/// ```rtfs
/// {
///   :userId :string
///   :expand :bool ;; optional
/// }
/// ```
pub fn type_expr_to_rtfs_pretty(expr: &TypeExpr) -> String {
    match expr {
        TypeExpr::Primitive(prim) => match prim {
            rtfs::ast::PrimitiveType::Int => ":int".to_string(),
            rtfs::ast::PrimitiveType::Float => ":float".to_string(),
            rtfs::ast::PrimitiveType::String => ":string".to_string(),
            rtfs::ast::PrimitiveType::Bool => ":bool".to_string(),
            rtfs::ast::PrimitiveType::Nil => ":nil".to_string(),
            _ => format!("{}", expr), // Use Display for other primitives
        },
        TypeExpr::Vector(inner) => {
            format!("[:vector {}]", type_expr_to_rtfs_pretty(inner))
        }
        TypeExpr::Map { entries, wildcard } => {
            if entries.is_empty() && wildcard.is_none() {
                return ":map".to_string();
            }

            let mut map_parts = vec!["{".to_string()];

            for entry in entries {
                let key_str = value_conversion::map_key_to_string(&rtfs::ast::MapKey::Keyword(entry.key.clone()));
                let value_str = type_expr_to_rtfs_pretty(&entry.value_type);
                if entry.optional {
                    map_parts.push(format!("    :{} {} ;; optional", key_str, value_str));
                } else {
                    map_parts.push(format!("    :{} {}", key_str, value_str));
                }
            }

            if let Some(wildcard_type) = wildcard {
                map_parts.push(format!(
                    "    :* {}",
                    type_expr_to_rtfs_pretty(wildcard_type)
                ));
            }

            map_parts.push("  }".to_string());
            map_parts.join("\n")
        }
        TypeExpr::Any => ":any".to_string(),
        TypeExpr::Never => ":never".to_string(),
        // For other complex types, fall back to Display
        _ => format!("{}", expr),
    }
}

/// Convert TypeExpr to RTFS schema string (alias for compact format)
///
/// This is the default serialization format that uses TypeExpr's Display trait.
pub fn type_expr_to_rtfs_string(expr: &TypeExpr) -> String {
    type_expr_to_rtfs_compact(expr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtfs::ast::{Keyword, MapTypeEntry, PrimitiveType};

    #[test]
    fn test_compact_primitive_types() {
        let int_type = TypeExpr::Primitive(PrimitiveType::Int);
        assert_eq!(type_expr_to_rtfs_compact(&int_type), ":int");

        let str_type = TypeExpr::Primitive(PrimitiveType::String);
        assert_eq!(type_expr_to_rtfs_compact(&str_type), ":string");
    }

    #[test]
    fn test_pretty_map_type() {
        let map_type = TypeExpr::Map {
            entries: vec![
                MapTypeEntry {
                    key: Keyword("userId".to_string()),
                    value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                    optional: false,
                },
                MapTypeEntry {
                    key: Keyword("expand".to_string()),
                    value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Bool)),
                    optional: true,
                },
            ],
            wildcard: None,
        };

        let result = type_expr_to_rtfs_pretty(&map_type);
        assert!(result.contains(":userId :string"));
        assert!(result.contains(":expand :bool ;; optional"));
        assert!(result.starts_with("{"));
        assert!(result.ends_with("}"));
    }

    #[test]
    fn test_vector_type() {
        let vec_type = TypeExpr::Vector(Box::new(TypeExpr::Primitive(PrimitiveType::Int)));
        assert_eq!(type_expr_to_rtfs_pretty(&vec_type), "[:vector :int]");
    }
}
