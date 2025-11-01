//! Types for Working Memory entries and metadata.
//!
//! Design goals:
//! - Small, serializable structures with clear provenance fields.
//! - Token-aware fields for budget enforcement upstream.
//! - Minimal helpers to keep this file focused on data types.
//!
//! Unit tests are colocated at the bottom of this file.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Identifier for a working memory entry (opaque string).
pub type WorkingMemoryId = String;

/// Minimal metadata to capture provenance and governance hooks.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkingMemoryMeta {
    /// Optional causal action id this entry was derived from.
    pub action_id: Option<String>,
    /// Optional plan id providing higher-level provenance.
    pub plan_id: Option<String>,
    /// Optional intent id tied to the distilled content.
    pub intent_id: Option<String>,
    /// Optional step id for finer-grained linkage.
    pub step_id: Option<String>,
    /// Optional provider identifier (e.g., capability provider).
    pub provider: Option<String>,
    /// Optional attestation hash for provenance verification.
    pub attestation_hash: Option<String>,
    /// Optional content hash used for idempotency/deduplication.
    pub content_hash: Option<String>,
    /// Arbitrary extra metadata for governance/analytics.
    pub extra: HashMap<String, String>,
}

/// A compact, queryable entry stored in Working Memory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkingMemoryEntry {
    /// Opaque identifier.
    pub id: WorkingMemoryId,
    /// Short human-readable summary/title.
    pub title: String,
    /// Compact content payload (e.g., markdown or JSON string).
    pub content: String,
    /// Semantic tags for retrieval (OR semantics by default).
    pub tags: HashSet<String>,
    /// Unix timestamp (seconds) for recency and time-window queries.
    pub timestamp_s: u64,
    /// Approximate token count used by reducers and budget enforcers.
    pub approx_tokens: usize,
    /// Provenance and governance metadata.
    pub meta: WorkingMemoryMeta,
}

impl WorkingMemoryEntry {
    /// Lightweight token estimation based on content length.
    /// This is intentionally simple; upstream can inject a better estimator.
    pub fn estimate_tokens_from_len(len: usize) -> usize {
        // Heuristic: ~4 chars per token as safe default for English text.
        // Avoid float ops to keep this minimal and deterministic.
        (len / 4).max(1)
    }

    /// Convenient constructor that auto-fills approx_tokens if not provided.
    pub fn new_with_estimate(
        id: WorkingMemoryId,
        title: impl Into<String>,
        content: impl Into<String>,
        tags: impl IntoIterator<Item = impl Into<String>>,
        timestamp_s: u64,
        meta: WorkingMemoryMeta,
    ) -> Self {
        let title = title.into();
        let content = content.into();
        let approx_tokens = Self::estimate_tokens_from_len(content.len());
        let tags = tags.into_iter().map(|t| t.into()).collect::<HashSet<_>>();
        Self {
            id,
            title,
            content,
            tags,
            timestamp_s,
            approx_tokens,
            meta,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens_from_len_minimum_one() {
        assert_eq!(WorkingMemoryEntry::estimate_tokens_from_len(0), 1);
        assert_eq!(WorkingMemoryEntry::estimate_tokens_from_len(1), 1);
        assert_eq!(WorkingMemoryEntry::estimate_tokens_from_len(3), 1);
        assert_eq!(WorkingMemoryEntry::estimate_tokens_from_len(4), 1);
        assert_eq!(WorkingMemoryEntry::estimate_tokens_from_len(5), 1);
        assert_eq!(WorkingMemoryEntry::estimate_tokens_from_len(8), 2);
    }

    #[test]
    fn test_entry_construction_with_estimate() {
        let meta = WorkingMemoryMeta {
            action_id: Some("act-1".into()),
            plan_id: Some("plan-1".into()),
            intent_id: None,
            step_id: None,
            provider: Some("demo.provider:v1".into()),
            attestation_hash: None,
            content_hash: Some("hash123".into()),
            extra: HashMap::new(),
        };

        let entry = WorkingMemoryEntry::new_with_estimate(
            "wm-1".into(),
            "Title",
            "Some short content",
            ["wisdom", "distillation", "causal-chain"],
            1_700_000_000,
            meta.clone(),
        );

        assert_eq!(entry.id, "wm-1");
        assert!(entry.approx_tokens >= 1);
        assert!(entry.tags.contains("wisdom"));
        assert!(entry.tags.contains("distillation"));
        assert!(entry.tags.contains("causal-chain"));
        assert_eq!(entry.meta, meta);
    }
}
