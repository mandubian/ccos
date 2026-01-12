//! Schema refinement for synthesized capabilities.
//!
//! This module provides functionality to refine capability schemas
//! based on runtime execution results.

use crate::synthesis::introspection::schema_inferrer::{
    infer_schema_from_value, is_generic_schema,
};
use rtfs::runtime::values::Value;
use std::fs;
use std::path::PathBuf;

/// Result of a schema refinement attempt.
#[derive(Debug, Clone)]
pub struct SchemaRefinementResult {
    pub capability_id: String,
    pub original_output_schema: String,
    pub inferred_output_schema: String,
    pub was_updated: bool,
    pub capability_path: Option<PathBuf>,
}

/// Refine a capability's output schema based on runtime output.
///
/// Returns the inferred schema and whether it differs from the declared one.
pub fn infer_output_schema_from_result(
    capability_id: &str,
    output: &Value,
    declared_schema: Option<&str>,
) -> SchemaRefinementResult {
    let inferred = infer_schema_from_value(output);
    let original = declared_schema.unwrap_or(":any").to_string();

    let should_update = is_generic_schema(&original) && !is_generic_schema(&inferred);

    SchemaRefinementResult {
        capability_id: capability_id.to_string(),
        original_output_schema: original,
        inferred_output_schema: inferred,
        was_updated: should_update,
        capability_path: None,
    }
}

/// Update a capability file with the refined output schema.
///
/// This reads the capability file, finds the `:output-schema` field,
/// and replaces it with the inferred schema.
pub fn update_capability_output_schema(
    capability_path: &PathBuf,
    new_schema: &str,
) -> Result<bool, String> {
    let content = fs::read_to_string(capability_path)
        .map_err(|e| format!("Failed to read capability file: {}", e))?;

    // Pattern to find :output-schema followed by the schema value
    // Handles both `:output-schema :any` and `:output-schema [:map ...]` patterns
    let output_schema_pattern = r":output-schema\s+([^\n]+)";
    let re =
        regex::Regex::new(output_schema_pattern).map_err(|e| format!("Invalid regex: {}", e))?;

    if !re.is_match(&content) {
        return Ok(false);
    }

    // Check if the current schema is generic
    if let Some(caps) = re.captures(&content) {
        let current_schema = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        if !is_generic_schema(current_schema.trim()) {
            // Don't overwrite precise schemas
            log::debug!(
                "Skipping schema update for {} - already has precise schema: {}",
                capability_path.display(),
                current_schema
            );
            return Ok(false);
        }
    }

    // Replace the schema
    let updated = re.replace(&content, format!(":output-schema {}", new_schema).as_str());

    if updated == content {
        return Ok(false);
    }

    // Write back
    fs::write(capability_path, updated.as_ref())
        .map_err(|e| format!("Failed to write capability file: {}", e))?;

    log::info!(
        "Updated output schema in {} to: {}",
        capability_path.display(),
        new_schema
    );

    Ok(true)
}

/// Find the capability file path for a given capability ID.
///
/// Looks in standard locations: capabilities/generated/
pub fn find_capability_file(capability_id: &str) -> Option<PathBuf> {
    use crate::utils::fs::get_workspace_root;

    let root = get_workspace_root();

    // Check generated capabilities
    let generated_dir = crate::utils::fs::get_configured_generated_path();
    if let Ok(entries) = fs::read_dir(&generated_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let dir_name = path.file_name().unwrap_or_default().to_string_lossy();
                // Match by capability ID prefix (generated capabilities use slug of the ID)
                if dir_name.contains(&capability_id.replace("/", "-").replace(".", "-"))
                    || dir_name.contains(&slugify_capability_id(capability_id))
                {
                    let cap_file = path.join("capability.rtfs");
                    if cap_file.exists() {
                        return Some(cap_file);
                    }
                }
            }
        }
    }

    // Check discovered/approved/pending capabilities
    let workspace_root = crate::utils::fs::get_workspace_root();
    let search_dirs = vec![
        workspace_root.join("capabilities/servers/approved"),
        workspace_root.join("capabilities/servers/pending"),
    ];

    for dir in search_dirs {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let cap_file = path.join("capability.rtfs");
                    let module_file = path.join("capabilities.rtfs");

                    for f in &[cap_file, module_file] {
                        if f.exists() {
                            // Read and check if ID matches
                            if let Ok(content) = fs::read_to_string(f) {
                                if content.contains(&format!("\"{}\"", capability_id)) {
                                    return Some(f.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

/// Convert a capability ID to a slug for directory matching.
fn slugify_capability_id(id: &str) -> String {
    id.to_lowercase()
        .replace("/", "-")
        .replace(".", "-")
        .replace("_", "-")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect()
}

// ============================================
// Metadata Sample Capture
// ============================================

/// Maximum length for sample strings (truncate if longer)
const SAMPLE_MAX_LEN: usize = 500;

/// Format a Value as an RTFS string for metadata storage
fn value_to_rtfs_sample(value: &Value) -> String {
    let s = format!("{}", value);
    if s.len() > SAMPLE_MAX_LEN {
        format!("{}... [truncated]", &s[..SAMPLE_MAX_LEN])
    } else {
        s
    }
}

/// Update a capability file with sample input/output metadata.
///
/// Adds or updates the `:metadata` section with sample values.
pub fn update_capability_metadata_samples(
    capability_path: &PathBuf,
    sample_input: Option<&Value>,
    sample_output: &Value,
) -> Result<bool, String> {
    let content = fs::read_to_string(capability_path)
        .map_err(|e| format!("Failed to read capability file: {}", e))?;

    // Generate sample strings
    let output_sample = value_to_rtfs_sample(sample_output);
    let input_sample = sample_input.map(value_to_rtfs_sample);
    let timestamp = chrono::Utc::now().to_rfc3339();

    // Check if :metadata section exists
    let metadata_pattern = r":metadata\s*\{[^}]*\}";
    let metadata_re =
        regex::Regex::new(metadata_pattern).map_err(|e| format!("Invalid regex: {}", e))?;

    let updated_content = if metadata_re.is_match(&content) {
        // Update existing metadata section
        // For simplicity, we'll replace the whole metadata block
        let new_metadata = if let Some(inp) = &input_sample {
            format!(
                ":metadata {{\n    :sample-input \"{}\"\n    :sample-output \"{}\"\n    :captured-at \"{}\"\n  }}",
                escape_rtfs_string(inp),
                escape_rtfs_string(&output_sample),
                timestamp
            )
        } else {
            format!(
                ":metadata {{\n    :sample-output \"{}\"\n    :captured-at \"{}\"\n  }}",
                escape_rtfs_string(&output_sample),
                timestamp
            )
        };
        metadata_re
            .replace(&content, new_metadata.as_str())
            .to_string()
    } else {
        // Insert metadata before closing paren of capability definition
        // Find the last closing paren
        let insert_metadata = if let Some(inp) = &input_sample {
            format!(
                "\n  :metadata {{\n    :sample-input \"{}\"\n    :sample-output \"{}\"\n    :captured-at \"{}\"\n  }}",
                escape_rtfs_string(inp),
                escape_rtfs_string(&output_sample),
                timestamp
            )
        } else {
            format!(
                "\n  :metadata {{\n    :sample-output \"{}\"\n    :captured-at \"{}\"\n  }}",
                escape_rtfs_string(&output_sample),
                timestamp
            )
        };

        // Find the position to insert (before last closing paren)
        if let Some(pos) = content.rfind(')') {
            let mut new_content = content.clone();
            new_content.insert_str(pos, &insert_metadata);
            new_content
        } else {
            return Ok(false);
        }
    };

    if updated_content == content {
        return Ok(false);
    }

    // Write back
    fs::write(capability_path, &updated_content)
        .map_err(|e| format!("Failed to write capability file: {}", e))?;

    log::info!("Updated metadata samples in {}", capability_path.display());

    Ok(true)
}

/// Escape a string for RTFS string literal
fn escape_rtfs_string(s: &str) -> String {
    s.replace("\\", "\\\\")
        .replace("\"", "\\\"")
        .replace("\n", "\\n")
        .replace("\r", "\\r")
        .replace("\t", "\\t")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_output_schema() {
        let output = Value::Map(std::collections::HashMap::new());
        let result = infer_output_schema_from_result("test.cap", &output, Some(":any"));
        assert!(result.was_updated || result.inferred_output_schema == "[:map]");
    }

    #[test]
    fn test_slugify() {
        assert_eq!(
            slugify_capability_id("generated/my-cap"),
            "generated-my-cap"
        );
        assert_eq!(
            slugify_capability_id("mcp.github/search_issues"),
            "mcp-github-search-issues"
        );
    }
}
