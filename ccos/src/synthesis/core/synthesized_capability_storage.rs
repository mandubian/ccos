//! Synthesized Capability Storage
//!
//! This module handles persisting synthesized inline RTFS capabilities so they can be
//! reused by future planning sessions. When the planner generates inline RTFS code for
//! transformations (group-by, filter, map, etc.), this module extracts and saves them
//! as proper capability definitions.
//!
//! ## Storage Location
//!
//! Synthesized capabilities are saved to:
//! - `$CCOS_SYNTHESIZED_CAPABILITY_STORAGE` if set
//! - Otherwise: `<workspace>/capabilities/synthesized/`
//!
//! ## Capability Format
//!
//! Each synthesized capability is saved as an RTFS file with:
//! - Unique ID derived from the description (e.g., `synthesized/group-issues-by-author-abc123`)
//! - Input/output schema inferred from usage context
//! - The inline RTFS implementation

use crate::utils::fs::{get_workspace_root, sanitize_filename};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use std::collections::HashMap;
use std::path::PathBuf;

/// Represents a synthesized capability extracted from a plan
#[derive(Debug, Clone)]
pub struct SynthesizedCapability {
    /// Unique capability ID (e.g., "synthesized/group-by-author-abc123")
    pub id: String,
    /// Human-readable description
    pub description: String,
    /// The inline RTFS implementation code
    pub implementation: String,
    /// Input schema (RTFS type expression string)
    pub input_schema: String,
    /// Output schema (RTFS type expression string)
    pub output_schema: String,
    /// Additional metadata (e.g., source plan ID, creation time)
    pub metadata: HashMap<String, String>,
}

impl SynthesizedCapability {
    /// Create a new synthesized capability
    pub fn new(description: &str, implementation: &str) -> Self {
        let id = generate_capability_id(description);
        Self {
            id,
            description: description.to_string(),
            implementation: implementation.to_string(),
            input_schema: ":any".to_string(),
            output_schema: ":any".to_string(),
            metadata: HashMap::new(),
        }
    }

    /// Set input schema
    pub fn with_input_schema(mut self, schema: &str) -> Self {
        self.input_schema = schema.to_string();
        self
    }

    /// Set output schema
    pub fn with_output_schema(mut self, schema: &str) -> Self {
        self.output_schema = schema.to_string();
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: &str, value: &str) -> Self {
        self.metadata.insert(key.to_string(), value.to_string());
        self
    }

    /// Convert to RTFS capability definition
    pub fn to_rtfs(&self) -> String {
        let timestamp = chrono::Utc::now().to_rfc3339();
        let escaped_desc = escape_string(&self.description);
        let escaped_id = escape_string(&self.id);

        let metadata_block = if self.metadata.is_empty() {
            String::new()
        } else {
            let entries: Vec<String> = self
                .metadata
                .iter()
                .map(|(k, v)| format!(":{} \"{}\"", k, escape_string(v)))
                .collect();
            format!("  :metadata {{{}}}\n", entries.join(" "))
        };

        // Pretty-print the implementation if possible
        let implementation_pretty = match rtfs::parser::parse_expression(&self.implementation) {
            Ok(expr) => crate::rtfs_bridge::expression_to_pretty_rtfs_string(&expr),
            Err(_) => self.implementation.clone(),
        };
        let implementation_indented = indent_block(&implementation_pretty, "    ");

        format!(
            r#";; Synthesized capability from planner
;; Generated: {timestamp}
;; Description: {desc}
(capability "{id}"
  :name "{name}"
  :description "{desc}"
  :version "1.0.0"
  :language "rtfs20"
  :permissions []
  :effects []
  :input-schema {input_schema}
  :output-schema {output_schema}
{metadata}  :implementation
{implementation}
)
"#,
            timestamp = timestamp,
            id = escaped_id,
            name = sanitize_name(&self.description),
            desc = escaped_desc,
            input_schema = self.input_schema,
            output_schema = self.output_schema,
            metadata = metadata_block,
            implementation = implementation_indented,
        )
    }
}

/// Storage for synthesized capabilities
pub struct SynthesizedCapabilityStorage {
    /// Root directory for storing synthesized capabilities
    storage_root: PathBuf,
}

impl SynthesizedCapabilityStorage {
    /// Create a new storage instance
    pub fn new() -> Self {
        let storage_root = get_synthesized_capability_storage_path();
        Self { storage_root }
    }

    /// Create with custom storage path
    pub fn with_path(path: PathBuf) -> Self {
        Self { storage_root: path }
    }

    /// Save a synthesized capability to disk
    pub fn save(&self, capability: &SynthesizedCapability) -> RuntimeResult<PathBuf> {
        // Create storage directory
        std::fs::create_dir_all(&self.storage_root).map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to create synthesized capability storage: {}",
                e
            ))
        })?;

        // Create capability directory (use sanitized ID as dir name)
        let dir_name = sanitize_filename(&capability.id.replace("synthesized/", ""));
        let capability_dir = self.storage_root.join(&dir_name);
        std::fs::create_dir_all(&capability_dir).map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to create capability directory '{}': {}",
                capability_dir.display(),
                e
            ))
        })?;

        // Write capability file
        let capability_file = capability_dir.join("capability.rtfs");
        let rtfs_content = capability.to_rtfs();

        std::fs::write(&capability_file, &rtfs_content).map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to write synthesized capability '{}': {}",
                capability_file.display(),
                e
            ))
        })?;

        log::info!(
            "💾 Saved synthesized capability: {} → {}",
            capability.id,
            capability_file.display()
        );

        Ok(capability_file)
    }

    /// Check if a capability with this ID already exists
    pub fn exists(&self, id: &str) -> bool {
        let dir_name = sanitize_filename(&id.replace("synthesized/", ""));
        let capability_file = self.storage_root.join(&dir_name).join("capability.rtfs");
        capability_file.exists()
    }

    /// List all stored synthesized capability IDs
    pub fn list_ids(&self) -> RuntimeResult<Vec<String>> {
        if !self.storage_root.exists() {
            return Ok(Vec::new());
        }

        let mut ids = Vec::new();
        for entry in std::fs::read_dir(&self.storage_root).map_err(|e| {
            RuntimeError::Generic(format!("Failed to read storage directory: {}", e))
        })? {
            let entry = entry.map_err(|e| {
                RuntimeError::Generic(format!("Failed to read directory entry: {}", e))
            })?;
            if entry.path().is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    ids.push(format!("synthesized/{}", name));
                }
            }
        }
        Ok(ids)
    }
}

impl Default for SynthesizedCapabilityStorage {
    fn default() -> Self {
        Self::new()
    }
}

/// Get the storage path for synthesized capabilities
pub fn get_synthesized_capability_storage_path() -> PathBuf {
    std::env::var("CCOS_SYNTHESIZED_CAPABILITY_STORAGE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| get_workspace_root().join("capabilities/synthesized"))
}

/// Generate a unique capability ID from a description
fn generate_capability_id(description: &str) -> String {
    let slug = slugify(description);
    let hash = fnv1a64(description);
    format!("synthesized/{}-{:08x}", slug, hash as u32)
}

/// Create a URL-safe slug from text
fn slugify(text: &str) -> String {
    let lowercase = text.to_lowercase();
    let slug: String = lowercase
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();

    // Collapse multiple dashes and trim
    let mut result = String::new();
    let mut last_was_dash = false;
    for c in slug.chars() {
        if c == '-' {
            if !last_was_dash && !result.is_empty() {
                result.push(c);
                last_was_dash = true;
            }
        } else {
            result.push(c);
            last_was_dash = false;
        }
    }

    // Truncate to reasonable length
    let truncated: String = result.chars().take(40).collect();
    truncated.trim_end_matches('-').to_string()
}

/// Simple FNV-1a 64-bit hash
fn fnv1a64(s: &str) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut hash = OFFSET_BASIS;
    for b in s.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Escape a string for RTFS
fn escape_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Create a sanitized function name from description
fn sanitize_name(description: &str) -> String {
    let slug = slugify(description);
    slug.replace('-', "_")
}

/// Indent a block of text
fn indent_block(text: &str, indent: &str) -> String {
    text.lines()
        .map(|line| {
            if line.trim().is_empty() {
                String::new()
            } else {
                format!("{}{}", indent, line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_capability_id() {
        let id = generate_capability_id("group issues by author");
        assert!(id.starts_with("synthesized/group-issues-by-author-"));
    }

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("Group By Author"), "group-by-author");
        assert_eq!(slugify("filter items with high score"), "filter-items-with-high-score");
    }

    #[test]
    fn test_capability_to_rtfs() {
        let cap = SynthesizedCapability::new(
            "group items by category",
            "(fn [input] (group-by :category (get input :items)))",
        )
        .with_input_schema("[:map [:items [:vector :any]]]")
        .with_output_schema("[:map :string [:vector :any]]");

        let rtfs = cap.to_rtfs();
        assert!(rtfs.contains("capability \"synthesized/"));
        assert!(rtfs.contains(":description \"group items by category\""));
        assert!(rtfs.contains("group-by :category"));
    }
}
