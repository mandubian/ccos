//! Inline RTFS adapters for bridging schema mismatches between steps
//!
//! When tool A's output schema doesn't match tool B's input schema,
//! this module generates inline RTFS expressions to bridge the gap.

use serde_json::Value as JsonValue;
use std::path::PathBuf;

/// Load sample output from capability metadata file.
///
/// Looks up the capability file and extracts the `:sample-output` from the `:metadata` section.
pub fn load_capability_sample(capability_id: &str) -> Option<JsonValue> {
    let path = find_capability_file_for_bridge(capability_id)?;
    let content = std::fs::read_to_string(&path).ok()?;
    extract_sample_from_rtfs_content(&content)
}

/// Find capability file path from capability ID
fn find_capability_file_for_bridge(capability_id: &str) -> Option<PathBuf> {
    // Use the schema_refiner's find function if available, or implement a simple lookup
    if let Some(path) =
        crate::synthesis::introspection::schema_refiner::find_capability_file(capability_id)
    {
        return Some(path);
    }

    // Fallback: try common locations
    let root = crate::utils::fs::get_workspace_root();

    // Try generated capabilities directory
    let generated_path = root
        .join("capabilities/generated")
        .join(capability_id)
        .join("capability.rtfs");
    if generated_path.exists() {
        return Some(generated_path);
    }

    None
}

/// Extract sample-output from RTFS capability file content
fn extract_sample_from_rtfs_content(content: &str) -> Option<JsonValue> {
    // Look for :sample-output in :metadata section
    // Pattern: :sample-output "..."
    let sample_pattern = r#":sample-output\s+"([^"\\]*(?:\\.[^"\\]*)*)""#;
    let re = regex::Regex::new(sample_pattern).ok()?;

    if let Some(caps) = re.captures(content) {
        let sample_str = caps.get(1)?.as_str();
        // Unescape the string
        let unescaped = sample_str
            .replace("\\\"", "\"")
            .replace("\\n", "\n")
            .replace("\\\\", "\\");
        // Try to parse as JSON
        serde_json::from_str(&unescaped).ok()
    } else {
        None
    }
}

/// Represents a detected schema mismatch and how to bridge it
#[derive(Debug, Clone)]
pub enum AdapterKind {
    /// Extract a field from a map: `(get :field input)`
    FieldExtract { field: String },
    /// Extract nested content from MCP response: `(get :text (first (get :content input)))`
    McpContentExtract,
    /// Extract and parse JSON from MCP content: `(parse-json (get :text (first (get :content input))))`
    McpContentExtractAndParse,
    /// Extract and parse JSON, then get a specific field
    McpContentExtractParseField { field: String },
    /// Wrap scalar in array: `[input]`
    ArrayWrap,
    /// No adapter needed
    None,
}

/// Result of schema analysis between producer and consumer
#[derive(Debug)]
pub struct SchemaBridge {
    pub kind: AdapterKind,
    /// Human-readable description of the transform
    pub description: String,
}

impl SchemaBridge {
    /// Analyze source and target schemas to determine if an adapter is needed
    pub fn detect(
        source_output: Option<&JsonValue>,
        target_input: Option<&JsonValue>,
        source_sample: Option<&JsonValue>,
    ) -> Self {
        // If we have a sample of the actual output, use that for detection
        if let Some(sample) = source_sample {
            return Self::detect_from_sample(sample, target_input);
        }

        // Fallback: schema-only analysis
        Self::detect_from_schemas(source_output, target_input)
    }

    /// Detect adapter from actual sample data
    fn detect_from_sample(sample: &JsonValue, target_input: Option<&JsonValue>) -> Self {
        // Check if target expects a vector
        let target_expects_vector = target_input
            .and_then(|s| s.get("type"))
            .map(|t| t.as_str() == Some("array"))
            .unwrap_or(false);

        // Case 1: Sample is a map with a single array field like {issues: [...]}
        if let JsonValue::Object(map) = sample {
            // Check for common MCP response patterns: {content: [{text: "...json..."}]}
            if let Some(content) = map.get("content") {
                if let Some(content_arr) = content.as_array() {
                    // Try to get the text from the first content item
                    if let Some(first_item) = content_arr.first() {
                        if let Some(text) = first_item.get("text").and_then(|t| t.as_str()) {
                            // Try to parse the text as JSON
                            if let Ok(parsed) = serde_json::from_str::<JsonValue>(text) {
                                if let JsonValue::Object(inner_map) = &parsed {
                                    // Always look for common array field names
                                    // Even without explicit target schema, passing the full object
                                    // typically causes type errors when the consumer wants a vector
                                    for field_name in &[
                                        "issues", "items", "results", "data", "records", "entries",
                                    ] {
                                        if let Some(field_value) = inner_map.get(*field_name) {
                                            if field_value.is_array() {
                                                return SchemaBridge {
                                                    kind:
                                                        AdapterKind::McpContentExtractParseField {
                                                            field: field_name.to_string(),
                                                        },
                                                    description: format!(
                                                        "Extract and parse '{}' from MCP content",
                                                        field_name
                                                    ),
                                                };
                                            }
                                        }
                                    }
                                    // If parsed is an object but no common array field, just parse
                                    return SchemaBridge {
                                        kind: AdapterKind::McpContentExtractAndParse,
                                        description: "Extract and parse JSON from MCP content"
                                            .to_string(),
                                    };
                                }
                                // If parsed is array directly
                                if parsed.is_array() {
                                    return SchemaBridge {
                                        kind: AdapterKind::McpContentExtractAndParse,
                                        description:
                                            "Extract and parse JSON array from MCP content"
                                                .to_string(),
                                    };
                                }
                            }
                        }
                    }
                    // Fallback: just extract content (legacy behavior)
                    return SchemaBridge {
                        kind: AdapterKind::McpContentExtract,
                        description: "Extract text from MCP content response".to_string(),
                    };
                }
            }

            // Check for single array field like {issues: [...]}
            let array_fields: Vec<_> = map
                .iter()
                .filter(|(_, v)| v.is_array())
                .map(|(k, _)| k.clone())
                .collect();

            if array_fields.len() == 1 && target_expects_vector {
                let field = array_fields[0].clone();
                return SchemaBridge {
                    kind: AdapterKind::FieldExtract {
                        field: field.clone(),
                    },
                    description: format!("Extract '{}' array from map", field),
                };
            }

            // Check for common field names that are typically arrays
            for field_name in &["issues", "items", "results", "data", "records", "entries"] {
                if let Some(field_value) = map.get(*field_name) {
                    if field_value.is_array() && target_expects_vector {
                        return SchemaBridge {
                            kind: AdapterKind::FieldExtract {
                                field: field_name.to_string(),
                            },
                            description: format!("Extract '{}' array from response", field_name),
                        };
                    }
                }
            }
        }

        // Case 2: Sample is scalar but target expects array
        if target_expects_vector && !sample.is_array() {
            return SchemaBridge {
                kind: AdapterKind::ArrayWrap,
                description: "Wrap scalar value in array".to_string(),
            };
        }

        // No adapter needed
        SchemaBridge {
            kind: AdapterKind::None,
            description: "Schemas compatible".to_string(),
        }
    }

    /// Detect adapter from schema definitions only (less accurate)
    fn detect_from_schemas(
        _source_output: Option<&JsonValue>,
        _target_input: Option<&JsonValue>,
    ) -> Self {
        // Without sample data, we can't reliably detect mismatches
        // Return None and let runtime handle it
        SchemaBridge {
            kind: AdapterKind::None,
            description: "No sample data for schema analysis".to_string(),
        }
    }

    /// Generate RTFS expression to apply this adapter
    pub fn generate_rtfs_expr(&self, input_var: &str) -> String {
        match &self.kind {
            AdapterKind::FieldExtract { field } => {
                format!("(get :{} {})", field, input_var)
            }
            AdapterKind::McpContentExtract => {
                format!("(get :text (first (get :content {})))", input_var)
            }
            AdapterKind::McpContentExtractAndParse => {
                format!(
                    "(parse-json (get :text (first (get :content {}))))",
                    input_var
                )
            }
            AdapterKind::McpContentExtractParseField { field } => {
                format!(
                    "(get :{} (parse-json (get :text (first (get :content {})))))",
                    field, input_var
                )
            }
            AdapterKind::ArrayWrap => {
                format!("[{}]", input_var)
            }
            AdapterKind::None => input_var.to_string(),
        }
    }

    /// Check if this bridge requires an adapter
    pub fn needs_adapter(&self) -> bool {
        !matches!(self.kind, AdapterKind::None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_detect_issues_field() {
        let sample = json!({
            "issues": [{"id": 1}, {"id": 2}],
            "totalCount": 2
        });
        let target = json!({"type": "array"});

        let bridge = SchemaBridge::detect(None, Some(&target), Some(&sample));

        assert!(matches!(bridge.kind, AdapterKind::FieldExtract { field } if field == "issues"));
        assert_eq!(bridge.generate_rtfs_expr("step_1"), "(get :issues step_1)");
    }

    #[test]
    fn test_no_adapter_needed() {
        let sample = json!([1, 2, 3]);
        let target = json!({"type": "array"});

        let bridge = SchemaBridge::detect(None, Some(&target), Some(&sample));

        assert!(matches!(bridge.kind, AdapterKind::None));
    }
}
