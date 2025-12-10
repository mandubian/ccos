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

use crate::utils::fs::get_workspace_root;

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
    pub fn needs_impl(capability_id: impl Into<String>, description: impl Into<String>) -> Self {
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
        let default_dir = get_workspace_root().join("storage/pending_synth");
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
        let path = self
            .base_dir
            .join(format!("{}-{}.json", file_name, Utc::now().timestamp()));
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

// ============================================
// PendingPlanQueue - Plans needing external review
// ============================================

/// Status of a queued plan for external review.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PendingPlanStatus {
    NeedsReview,
    Reviewed,
    Approved,
    Rejected,
}

/// A plan that failed validation and needs external review.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingPlanItem {
    /// Plan identifier (goal hash or similar)
    pub plan_id: String,
    /// The RTFS plan code
    pub rtfs_plan: String,
    /// Original goal/intent description
    pub goal: String,
    /// Validation errors that triggered escalation
    pub validation_errors: Vec<String>,
    /// Auto-repair attempts made
    pub repair_attempts: usize,
    /// Additional context (resolutions, grounding data)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<serde_json::Value>,
    /// Current status
    pub status: PendingPlanStatus,
    /// Timestamp (RFC3339)
    pub created_at: String,
}

impl PendingPlanItem {
    pub fn needs_review(
        plan_id: impl Into<String>,
        rtfs_plan: impl Into<String>,
        goal: impl Into<String>,
        validation_errors: Vec<String>,
        repair_attempts: usize,
    ) -> Self {
        Self {
            plan_id: plan_id.into(),
            rtfs_plan: rtfs_plan.into(),
            goal: goal.into(),
            validation_errors,
            repair_attempts,
            context: None,
            status: PendingPlanStatus::NeedsReview,
            created_at: Utc::now().to_rfc3339(),
        }
    }

    pub fn with_context(mut self, ctx: serde_json::Value) -> Self {
        self.context = Some(ctx);
        self
    }
}

/// Queue for plans that need external review.
pub struct PendingPlanQueue {
    base_dir: PathBuf,
}

impl PendingPlanQueue {
    /// Create queue at the provided path or default under workspace root.
    pub fn new(base_dir: Option<PathBuf>) -> Self {
        let default_dir = get_workspace_root().join("storage/pending_validation");
        Self {
            base_dir: base_dir.unwrap_or(default_dir),
        }
    }

    /// Enqueue a plan for external review. Returns the path written.
    pub fn enqueue(&self, item: PendingPlanItem) -> RuntimeResult<PathBuf> {
        if !self.base_dir.exists() {
            fs::create_dir_all(&self.base_dir).map_err(|e| {
                RuntimeError::Generic(format!(
                    "Failed to create pending plan queue dir {}: {}",
                    self.base_dir.display(),
                    e
                ))
            })?;
        }

        let file_name = SynthQueue::sanitize_file_name(&item.plan_id);
        let path = self
            .base_dir
            .join(format!("{}-{}.json", file_name, Utc::now().timestamp()));
        let json = serde_json::to_string_pretty(&item).map_err(|e| {
            RuntimeError::Generic(format!("Failed to serialize pending plan item: {}", e))
        })?;

        fs::write(&path, json).map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to write pending plan item to {}: {}",
                path.display(),
                e
            ))
        })?;

        log::info!("Plan queued for external review: {}", path.display());
        Ok(path)
    }

    /// List all pending plans.
    pub fn list_pending(&self) -> RuntimeResult<Vec<PendingPlanItem>> {
        if !self.base_dir.exists() {
            return Ok(vec![]);
        }

        let mut items = Vec::new();
        for entry in fs::read_dir(&self.base_dir).map_err(|e| {
            RuntimeError::Generic(format!("Failed to read pending plan queue dir: {}", e))
        })? {
            let entry = entry.map_err(|e| RuntimeError::Generic(e.to_string()))?;
            let path = entry.path();
            if path.extension().map(|s| s == "json").unwrap_or(false) {
                let content =
                    fs::read_to_string(&path).map_err(|e| RuntimeError::Generic(e.to_string()))?;
                if let Ok(item) = serde_json::from_str::<PendingPlanItem>(&content) {
                    items.push(item);
                }
            }
        }
        Ok(items)
    }
}

