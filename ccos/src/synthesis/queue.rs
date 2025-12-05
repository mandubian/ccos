//! Synthesis queue utilities.
//!
//! This queue is used when automatic synthesis cannot produce a runnable capability.
//! It stores a structured artifact that humans (or a stronger LLM/codegen path)
//! can reify later.

use chrono::Utc;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::utils::fs::find_workspace_root;

/// Status of a queued synthesis artifact.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SynthQueueStatus {
    NeedsImpl,
    Generated,
    Failed,
}

/// Artifact describing a capability that needs implementation/reification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynthQueueItem {
    /// Suggested capability id (fully qualified).
    pub capability_id: String,
    /// Natural language description of the intent.
    pub description: String,
    /// Input schema (if known).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<serde_json::Value>,
    /// Output schema (if known).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<serde_json::Value>,
    /// Example input payload (if available).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example_input: Option<serde_json::Value>,
    /// Example output payload (if available).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example_output: Option<serde_json::Value>,
    /// Source intent/goal that triggered this artifact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_intent: Option<String>,
    /// Free-form notes or synthesis diagnostics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Current status.
    pub status: SynthQueueStatus,
    /// Timestamp (RFC3339).
    pub created_at: String,
}

impl SynthQueueItem {
    pub fn needs_impl(
        capability_id: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            capability_id: capability_id.into(),
            description: description.into(),
            input_schema: None,
            output_schema: None,
            example_input: None,
            example_output: None,
            source_intent: None,
            notes: None,
            status: SynthQueueStatus::NeedsImpl,
            created_at: Utc::now().to_rfc3339(),
        }
    }
}

/// Queue writer backed by the filesystem.
pub struct SynthQueue {
    base_dir: PathBuf,
}

impl SynthQueue {
    /// Create a new queue at the provided path or the default under workspace root.
    pub fn new(base_dir: Option<PathBuf>) -> Self {
        let default_dir = find_workspace_root().join("capabilities/pending_synth");
        Self {
            base_dir: base_dir.unwrap_or(default_dir),
        }
    }

    /// Enqueue an artifact as JSON. Returns the path written.
    pub fn enqueue(&self, item: SynthQueueItem) -> RuntimeResult<PathBuf> {
        if !self.base_dir.exists() {
            fs::create_dir_all(&self.base_dir).map_err(|e| {
                RuntimeError::Generic(format!(
                    "Failed to create synthesis queue dir {}: {}",
                    self.base_dir.display(),
                    e
                ))
            })?;
        }

        let file_name = Self::sanitize_file_name(&item.capability_id);
        let path = self.base_dir.join(format!("{}-{}.json", file_name, Utc::now().timestamp()));
        let json = serde_json::to_string_pretty(&item).map_err(|e| {
            RuntimeError::Generic(format!("Failed to serialize synth queue item: {}", e))
        })?;

        fs::write(&path, json).map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to write synth queue item to {}: {}",
                path.display(),
                e
            ))
        })?;

        Ok(path)
    }

    fn sanitize_file_name(id: &str) -> String {
        id.chars()
            .map(|c| match c {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => c,
                _ => '-',
            })
            .collect()
    }
}


