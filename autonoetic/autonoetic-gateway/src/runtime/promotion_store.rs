//! Content Promotion Registry.
//!
//! Tracks promotion status (evaluator/auditor validation) per content handle.
//! This is the authoritative source for whether content has passed validation gates.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use autonoetic_types::promotion::{Finding, PromotionRecord, PromotionRole};

/// Thread-safe promotion registry mapping content handles to promotion records.
pub struct PromotionStore {
    store_path: std::path::PathBuf,
    records: Arc<Mutex<HashMap<String, PromotionRecord>>>,
}

impl PromotionStore {
    /// Creates a new PromotionStore, loading existing records from disk.
    pub fn new(gateway_dir: &Path) -> anyhow::Result<Self> {
        let store_path = gateway_dir.join("promotion_registry.json");
        let records = if store_path.exists() {
            let json = std::fs::read_to_string(&store_path)?;
            let records: HashMap<String, PromotionRecord> = serde_json::from_str(&json)?;
            tracing::info!(
                target: "promotion_store",
                path = %store_path.display(),
                count = records.len(),
                "Loaded existing promotion registry"
            );
            records
        } else {
            HashMap::new()
        };

        Ok(Self {
            store_path,
            records: Arc::new(Mutex::new(records)),
        })
    }

    /// Records or updates a promotion record for a content handle.
    ///
    /// If a record already exists for this content handle, updates the role-specific fields.
    pub fn record_promotion(
        &self,
        content_handle: String,
        role: PromotionRole,
        agent_id: &str,
        pass: bool,
        findings: Vec<Finding>,
        summary: Option<String>,
    ) -> anyhow::Result<PromotionRecord> {
        let timestamp = chrono::Utc::now().to_rfc3339();

        let mut records = self.records.lock().unwrap();

        let record = records
            .entry(content_handle.clone())
            .or_insert_with(|| PromotionRecord {
                content_handle: content_handle.clone(),
                evaluator_id: None,
                evaluator_pass: false,
                evaluator_findings: vec![],
                evaluator_timestamp: None,
                auditor_id: None,
                auditor_pass: false,
                auditor_findings: vec![],
                auditor_timestamp: None,
                promotion_gate_version: "1.0".to_string(),
            });

        match role {
            PromotionRole::Evaluator => {
                record.evaluator_id = Some(agent_id.to_string());
                record.evaluator_pass = pass;
                record.evaluator_findings = findings;
                record.evaluator_timestamp = Some(timestamp);
                tracing::info!(
                    target: "promotion_store",
                    content_handle = %content_handle,
                    agent_id = %agent_id,
                    pass = pass,
                    findings_count = record.evaluator_findings.len(),
                    "Recorded evaluator promotion"
                );
            }
            PromotionRole::Auditor => {
                record.auditor_id = Some(agent_id.to_string());
                record.auditor_pass = pass;
                record.auditor_findings = findings;
                record.auditor_timestamp = Some(timestamp);
                tracing::info!(
                    target: "promotion_store",
                    content_handle = %content_handle,
                    agent_id = %agent_id,
                    pass = pass,
                    findings_count = record.auditor_findings.len(),
                    "Recorded auditor promotion"
                );
            }
        }

        if let Some(summary) = summary {
            tracing::debug!(
                target: "promotion_store",
                content_handle = %content_handle,
                summary = %summary,
                "Promotion summary recorded"
            );
        }

        let record = record.clone();
        drop(records);

        self.save()?;

        Ok(record)
    }

    /// Gets a promotion record by content handle.
    pub fn get_promotion(&self, content_handle: &str) -> Option<PromotionRecord> {
        let records = self.records.lock().unwrap();
        records.get(content_handle).cloned()
    }

    /// Lists all promotion records.
    pub fn list_promotions(&self) -> Vec<PromotionRecord> {
        let records = self.records.lock().unwrap();
        records.values().cloned().collect()
    }

    /// Returns true if a content handle has passed promotion for the given role.
    pub fn has_passed(&self, content_handle: &str, role: &PromotionRole) -> bool {
        let records = self.records.lock().unwrap();
        if let Some(record) = records.get(content_handle) {
            match role {
                PromotionRole::Evaluator => record.evaluator_pass,
                PromotionRole::Auditor => record.auditor_pass,
            }
        } else {
            false
        }
    }

    /// Returns true if a content handle has passed both evaluator and auditor promotion.
    pub fn is_fully_promoted(&self, content_handle: &str) -> bool {
        let records = self.records.lock().unwrap();
        if let Some(record) = records.get(content_handle) {
            record.evaluator_pass && record.auditor_pass
        } else {
            false
        }
    }

    /// Saves the promotion registry to disk.
    fn save(&self) -> anyhow::Result<()> {
        let records = self.records.lock().unwrap();
        if let Some(parent) = self.store_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&*records)?;
        std::fs::write(&self.store_path, json)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use autonoetic_types::promotion::FindingSeverity;
    use tempfile::tempdir;

    fn test_finding() -> Finding {
        Finding {
            severity: FindingSeverity::Info,
            description: "Test passed".to_string(),
            evidence: Some("Test output".to_string()),
        }
    }

    #[test]
    fn test_promotion_store_record_and_get() {
        let temp = tempdir().unwrap();
        let store = PromotionStore::new(temp.path()).unwrap();

        let handle = "sha256:abc123def456".to_string();

        let record = store
            .record_promotion(
                handle.clone(),
                PromotionRole::Evaluator,
                "evaluator.default",
                true,
                vec![test_finding()],
                Some("All tests passed".to_string()),
            )
            .unwrap();

        assert_eq!(record.content_handle, handle);
        assert_eq!(record.evaluator_id, Some("evaluator.default".to_string()));
        assert!(record.evaluator_pass);
        assert_eq!(record.evaluator_findings.len(), 1);

        let retrieved = store.get_promotion(&handle).unwrap();
        assert_eq!(retrieved.content_handle, handle);
        assert!(retrieved.evaluator_pass);
    }

    #[test]
    fn test_promotion_store_both_roles() {
        let temp = tempdir().unwrap();
        let store = PromotionStore::new(temp.path()).unwrap();

        let handle = "sha256:abc123".to_string();

        store
            .record_promotion(
                handle.clone(),
                PromotionRole::Evaluator,
                "evaluator.default",
                true,
                vec![],
                None,
            )
            .unwrap();

        store
            .record_promotion(
                handle.clone(),
                PromotionRole::Auditor,
                "auditor.default",
                true,
                vec![],
                None,
            )
            .unwrap();

        assert!(store.has_passed(&handle, &PromotionRole::Evaluator));
        assert!(store.has_passed(&handle, &PromotionRole::Auditor));
        assert!(store.is_fully_promoted(&handle));
    }

    #[test]
    fn test_promotion_store_evaluator_fail() {
        let temp = tempdir().unwrap();
        let store = PromotionStore::new(temp.path()).unwrap();

        let handle = "sha256:abc123".to_string();

        store
            .record_promotion(
                handle.clone(),
                PromotionRole::Evaluator,
                "evaluator.default",
                false,
                vec![Finding {
                    severity: FindingSeverity::Error,
                    description: "Test failed".to_string(),
                    evidence: None,
                }],
                None,
            )
            .unwrap();

        assert!(!store.has_passed(&handle, &PromotionRole::Evaluator));
        assert!(!store.is_fully_promoted(&handle));
    }

    #[test]
    fn test_promotion_store_update_role() {
        let temp = tempdir().unwrap();
        let store = PromotionStore::new(temp.path()).unwrap();

        let handle = "sha256:abc123".to_string();

        store
            .record_promotion(
                handle.clone(),
                PromotionRole::Evaluator,
                "evaluator.default",
                false,
                vec![],
                None,
            )
            .unwrap();

        store
            .record_promotion(
                handle.clone(),
                PromotionRole::Evaluator,
                "evaluator.default",
                true,
                vec![],
                None,
            )
            .unwrap();

        let record = store.get_promotion(&handle).unwrap();
        assert!(record.evaluator_pass);
        assert_eq!(record.evaluator_id, Some("evaluator.default".to_string()));
    }

    #[test]
    fn test_promotion_store_persistence() {
        let temp = tempdir().unwrap();

        let handle = "sha256:abc123".to_string();

        {
            let store = PromotionStore::new(temp.path()).unwrap();
            store
                .record_promotion(
                    handle.clone(),
                    PromotionRole::Evaluator,
                    "evaluator.default",
                    true,
                    vec![],
                    None,
                )
                .unwrap();
        }

        {
            let store = PromotionStore::new(temp.path()).unwrap();
            let record = store.get_promotion(&handle).unwrap();
            assert!(record.evaluator_pass);
            assert_eq!(record.evaluator_id, Some("evaluator.default".to_string()));
        }
    }

    #[test]
    fn test_promotion_store_not_found() {
        let temp = tempdir().unwrap();
        let store = PromotionStore::new(temp.path()).unwrap();

        assert!(store.get_promotion("sha256:nonexistent").is_none());
        assert!(!store.has_passed("sha256:nonexistent", &PromotionRole::Evaluator));
        assert!(!store.is_fully_promoted("sha256:nonexistent"));
    }
}
