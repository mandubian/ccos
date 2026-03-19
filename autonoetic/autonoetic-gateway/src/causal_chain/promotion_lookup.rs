//! Promotion Lookup via Causal Chain.
//!
//! Verifies that promotion records actually exist in the causal chain
//! (tamper-evidence for promotion claims).

use crate::causal_chain::{read_all_entries_across_segments, CausalLogger};
use autonoetic_types::causal_chain::CausalChainEntry;
use autonoetic_types::promotion::PromotionRole;
use std::path::Path;

pub struct PromotionLookup {
    history_dir: std::path::PathBuf,
}

impl PromotionLookup {
    pub fn new(history_dir: std::path::PathBuf) -> Self {
        Self { history_dir }
    }

    pub fn history_dir(&self) -> &Path {
        &self.history_dir
    }

    /// Finds all promotion.record causal chain entries for a given content handle.
    pub fn find_promotion_entries(
        &self,
        content_handle: &str,
    ) -> anyhow::Result<Vec<CausalChainEntry>> {
        let entries = read_all_entries_across_segments(&self.history_dir)?;
        let mut matching = Vec::new();

        for entry in entries {
            if entry.category == "tool" && entry.action == "promotion.record" {
                if let Some(payload) = &entry.payload {
                    if let Some(args) = payload.get("arguments") {
                        if let Some(handle) = args.get("content_handle") {
                            if handle.as_str() == Some(content_handle) {
                                matching.push(entry);
                            }
                        }
                    }
                }
            }
        }

        Ok(matching)
    }

    /// Verifies that a successful promotion.record call exists in the causal chain
    /// for the given content handle and role.
    pub fn verify_promotion(
        &self,
        content_handle: &str,
        role: &PromotionRole,
    ) -> anyhow::Result<bool> {
        let entries = self.find_promotion_entries(content_handle)?;
        let role_str = role.as_str();

        for entry in entries {
            if matches!(
                entry.status,
                autonoetic_types::causal_chain::EntryStatus::Success
            ) {
                if let Some(payload) = &entry.payload {
                    if let Some(args) = payload.get("arguments") {
                        if let Some(entry_role) = args.get("role") {
                            if entry_role.as_str() == Some(role_str) {
                                if let Some(pass) = args.get("pass") {
                                    if pass.as_bool() == Some(true) {
                                        return Ok(true);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(false)
    }

    /// Returns the agent_id that recorded the promotion for a given content handle and role.
    pub fn get_recorder(
        &self,
        content_handle: &str,
        role: &PromotionRole,
    ) -> anyhow::Result<Option<String>> {
        let entries = self.find_promotion_entries(content_handle)?;
        let role_str = role.as_str();

        for entry in entries {
            if matches!(
                entry.status,
                autonoetic_types::causal_chain::EntryStatus::Success
            ) {
                if let Some(payload) = &entry.payload {
                    if let Some(args) = payload.get("arguments") {
                        if let Some(entry_role) = args.get("role") {
                            if entry_role.as_str() == Some(role_str) {
                                return Ok(Some(entry.actor_id.clone()));
                            }
                        }
                    }
                }
            }
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_promotion_lookup_verify() {
        let temp = tempdir().unwrap();
        let history_dir = temp.path().to_path_buf();

        let logger = CausalLogger::new(history_dir.join("causal_chain.jsonl")).unwrap();

        logger
            .log(
                "evaluator.default",
                "session-1",
                Some("turn-1"),
                1,
                "tool",
                "promotion.record",
                autonoetic_types::causal_chain::EntryStatus::Success,
                Some(serde_json::json!({
                    "arguments": {
                        "content_handle": "sha256:abc123",
                        "role": "evaluator",
                        "pass": true
                    }
                })),
            )
            .unwrap();

        let lookup = PromotionLookup::new(history_dir);
        let result = lookup
            .verify_promotion("sha256:abc123", &PromotionRole::Evaluator)
            .unwrap();

        assert!(result);
    }

    #[test]
    fn test_promotion_lookup_not_found() {
        let temp = tempdir().unwrap();
        let history_dir = temp.path().to_path_buf();

        let logger = CausalLogger::new(history_dir.join("causal_chain.jsonl")).unwrap();

        logger
            .log(
                "evaluator.default",
                "session-1",
                Some("turn-1"),
                1,
                "tool",
                "promotion.record",
                autonoetic_types::causal_chain::EntryStatus::Success,
                Some(serde_json::json!({
                    "arguments": {
                        "content_handle": "sha256:abc123",
                        "role": "evaluator",
                        "pass": true
                    }
                })),
            )
            .unwrap();

        let lookup = PromotionLookup::new(history_dir);
        let result = lookup
            .verify_promotion("sha256:different", &PromotionRole::Evaluator)
            .unwrap();

        assert!(!result);
    }

    #[test]
    fn test_promotion_lookup_wrong_role() {
        let temp = tempdir().unwrap();
        let history_dir = temp.path().to_path_buf();

        let logger = CausalLogger::new(history_dir.join("causal_chain.jsonl")).unwrap();

        logger
            .log(
                "evaluator.default",
                "session-1",
                Some("turn-1"),
                1,
                "tool",
                "promotion.record",
                autonoetic_types::causal_chain::EntryStatus::Success,
                Some(serde_json::json!({
                    "arguments": {
                        "content_handle": "sha256:abc123",
                        "role": "evaluator",
                        "pass": true
                    }
                })),
            )
            .unwrap();

        let lookup = PromotionLookup::new(history_dir);
        let result = lookup
            .verify_promotion("sha256:abc123", &PromotionRole::Auditor)
            .unwrap();

        assert!(!result);
    }
}
