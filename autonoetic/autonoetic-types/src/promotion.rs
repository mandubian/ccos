//! Content Promotion Registry types.
//!
//! Tracks promotion status (evaluator/auditor validation) per content handle.

use serde::{Deserialize, Serialize};

/// A finding from evaluator or auditor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub severity: FindingSeverity,
    pub description: String,
    pub evidence: Option<String>,
}

/// Severity level of a finding.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FindingSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

/// Role that recorded the promotion.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PromotionRole {
    Evaluator,
    Auditor,
}

impl PromotionRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            PromotionRole::Evaluator => "evaluator",
            PromotionRole::Auditor => "auditor",
        }
    }
}

/// Content promotion record linking validation results to a content handle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionRecord {
    /// SHA256 content handle this promotion applies to.
    pub content_handle: String,
    /// Agent who validated (evaluator.default).
    #[serde(default)]
    pub evaluator_id: Option<String>,
    /// Whether evaluator passed.
    #[serde(default)]
    pub evaluator_pass: bool,
    /// Findings from evaluator.
    #[serde(default)]
    pub evaluator_findings: Vec<Finding>,
    /// Timestamp of evaluator validation (ISO 8601).
    #[serde(default)]
    pub evaluator_timestamp: Option<String>,
    /// Agent who audited (auditor.default).
    #[serde(default)]
    pub auditor_id: Option<String>,
    /// Whether auditor passed.
    #[serde(default)]
    pub auditor_pass: bool,
    /// Findings from auditor.
    #[serde(default)]
    pub auditor_findings: Vec<Finding>,
    /// Timestamp of auditor validation (ISO 8601).
    #[serde(default)]
    pub auditor_timestamp: Option<String>,
    /// Version of promotion gate schema.
    pub promotion_gate_version: String,
}

/// Arguments for the `promotion.record` tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionRecordArgs {
    /// SHA256 handle of content being promoted.
    pub content_handle: String,
    /// Role recording this promotion (evaluator or auditor).
    pub role: PromotionRole,
    /// Whether this role's validation passed.
    pub pass: bool,
    /// Findings from this validation.
    #[serde(default)]
    pub findings: Vec<Finding>,
    /// Human-readable summary of the validation.
    #[serde(default)]
    pub summary: Option<String>,
}

/// Response from the `promotion.record` tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionRecordResponse {
    pub ok: bool,
    pub promotion_record: PromotionRecord,
}

/// Arguments for the `promotion.query` tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionQueryArgs {
    /// SHA256 handle to query promotion status for.
    pub content_handle: String,
}

/// Response from the `promotion.query` tool.
/// Returns None if no promotion record exists for the handle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionQueryResponse {
    pub content_handle: String,
    pub evaluator_pass: Option<bool>,
    pub auditor_pass: Option<bool>,
    pub evaluator_id: Option<String>,
    pub auditor_id: Option<String>,
    pub evaluator_findings: Vec<Finding>,
    pub auditor_findings: Vec<Finding>,
    pub evaluator_timestamp: Option<String>,
    pub auditor_timestamp: Option<String>,
    pub promotion_gate_version: String,
}
