//! Shared utilities for serializing TypeExpr to RTFS schema format
//!
//! This module provides common functions for converting RTFS TypeExpr AST nodes
//! into human-readable RTFS schema strings for capability definitions.

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
    type_expr_to_rtfs_pretty_indented(expr, 0)
}

fn type_expr_to_rtfs_pretty_indented(expr: &TypeExpr, indent_level: usize) -> String {
    let indent = " ".repeat(indent_level * 2);
    let child_indent = " ".repeat((indent_level + 1) * 2);

    match expr {
        TypeExpr::Primitive(prim) => match prim {
            rtfs::ast::PrimitiveType::Int => ":int".to_string(),
            rtfs::ast::PrimitiveType::Float => ":float".to_string(),
            rtfs::ast::PrimitiveType::String => ":string".to_string(),
            rtfs::ast::PrimitiveType::Bool => ":bool".to_string(),
            rtfs::ast::PrimitiveType::Nil => ":nil".to_string(),
            _ => format!("{}", expr),
        },
        TypeExpr::Vector(inner) => {
            let inner_str = type_expr_to_rtfs_pretty_indented(inner, indent_level);
            if inner_str.contains('\n') {
                format!(
                    "[:vector\n{}{}\n{}]",
                    child_indent,
                    inner_str.trim(),
                    indent
                )
            } else {
                format!("[:vector {}]", inner_str)
            }
        }
        TypeExpr::Map { entries, wildcard } => {
            if entries.is_empty() && wildcard.is_none() {
                return ":map".to_string();
            }

            let mut lines = Vec::new();
            lines.push("[:map".to_string());

            for entry in entries {
                let key_str = entry.key.to_string();
                let value_str =
                    type_expr_to_rtfs_pretty_indented(&entry.value_type, indent_level + 1);
                let optional_suffix = if entry.optional { "?" } else { "" };

                if value_str.contains('\n') {
                    // If value is multiline, we format like:
                    //   [:key
                    //     [:vector
                    //       ...
                    //     ]
                    //   ]
                    lines.push(format!("{}[{}{}", child_indent, key_str, optional_suffix));
                    lines.push(format!(" {}", value_str.trim_start())); // Value indented
                    lines.push(format!("{}]", child_indent));
                } else {
                    lines.push(format!(
                        "{}[{} {}{}]",
                        child_indent,
                        key_str,
                        value_str.trim_start(),
                        optional_suffix
                    ));
                }
            }

            if let Some(wildcard_type) = wildcard {
                let value_str = type_expr_to_rtfs_pretty_indented(wildcard_type, indent_level + 1);
                lines.push(format!(
                    "{}[{} {}]",
                    child_indent,
                    ":*",
                    value_str.trim_start()
                ));
            }

            lines.push(format!("{}]", indent));
            lines.join("\n")
        }
        TypeExpr::Any => ":any".to_string(),
        TypeExpr::Never => ":never".to_string(),
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
        assert!(result.contains("[:userId :string]"));
        assert!(result.contains("[:expand :bool?]"));
        assert!(result.starts_with("[:map"));
        assert!(result.ends_with("]"));
    }

    #[test]
    fn test_vector_type() {
        let vec_type = TypeExpr::Vector(Box::new(TypeExpr::Primitive(PrimitiveType::Int)));
        assert_eq!(type_expr_to_rtfs_pretty(&vec_type), "[:vector :int]");
    }
}
