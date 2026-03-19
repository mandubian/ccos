//! Content Promotion Registry.
//!
//! Tracks promotion status (evaluator/auditor validation) per artifact.
//! This is the authoritative source for whether an artifact has passed validation gates.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use autonoetic_types::promotion::{Finding, PromotionRecord, PromotionRole};

/// Thread-safe promotion registry mapping artifact IDs to promotion records.
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

    /// Records or updates a promotion record for an artifact.
    ///
    /// If a record already exists for this artifact, updates the role-specific fields.
    pub fn record_promotion(
        &self,
        artifact_id: String,
        artifact_digest: Option<String>,
        role: PromotionRole,
        agent_id: &str,
        pass: bool,
        findings: Vec<Finding>,
        summary: Option<String>,
    ) -> anyhow::Result<PromotionRecord> {
        let timestamp = chrono::Utc::now().to_rfc3339();

        let mut records = self.records.lock().unwrap();

        let record = records
            .entry(artifact_id.clone())
            .or_insert_with(|| PromotionRecord {
                artifact_id: artifact_id.clone(),
                artifact_digest: artifact_digest.clone(),
                evaluator_id: None,
                evaluator_pass: false,
                evaluator_findings: vec![],
                evaluator_timestamp: None,
                auditor_id: None,
                auditor_pass: false,
                auditor_findings: vec![],
                auditor_timestamp: None,
                promotion_gate_version: "2.0".to_string(),
            });

        match role {
            PromotionRole::Evaluator => {
                record.evaluator_id = Some(agent_id.to_string());
                record.evaluator_pass = pass;
                record.evaluator_findings = findings;
                record.evaluator_timestamp = Some(timestamp);
                tracing::info!(
                    target: "promotion_store",
                    artifact_id = %artifact_id,
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
                    artifact_id = %artifact_id,
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
                artifact_id = %artifact_id,
                summary = %summary,
                "Promotion summary recorded"
            );
        }

        let record = record.clone();
        drop(records);

        self.save()?;

        Ok(record)
    }

    /// Gets a promotion record by artifact ID.
    pub fn get_promotion(&self, artifact_id: &str) -> Option<PromotionRecord> {
        let records = self.records.lock().unwrap();
        records.get(artifact_id).cloned()
    }

    /// Lists all promotion records.
    pub fn list_promotions(&self) -> Vec<PromotionRecord> {
        let records = self.records.lock().unwrap();
        records.values().cloned().collect()
    }

    /// Returns true if an artifact has passed promotion for the given role.
    pub fn has_passed(&self, artifact_id: &str, role: &PromotionRole) -> bool {
        let records = self.records.lock().unwrap();
        if let Some(record) = records.get(artifact_id) {
            match role {
                PromotionRole::Evaluator => record.evaluator_pass,
                PromotionRole::Auditor => record.auditor_pass,
            }
        } else {
            false
        }
    }

    /// Returns true if an artifact has passed both evaluator and auditor promotion.
    pub fn is_fully_promoted(&self, artifact_id: &str) -> bool {
        let records = self.records.lock().unwrap();
        if let Some(record) = records.get(artifact_id) {
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

        let artifact_id = "art_abc123".to_string();

        let record = store
            .record_promotion(
                artifact_id.clone(),
                Some("sha256:abc123".to_string()),
                PromotionRole::Evaluator,
                "evaluator.default",
                true,
                vec![test_finding()],
                Some("All tests passed".to_string()),
            )
            .unwrap();

        assert_eq!(record.artifact_id, artifact_id);
        assert_eq!(record.evaluator_id, Some("evaluator.default".to_string()));
        assert!(record.evaluator_pass);
        assert_eq!(record.evaluator_findings.len(), 1);

        let retrieved = store.get_promotion(&artifact_id).unwrap();
        assert_eq!(retrieved.artifact_id, artifact_id);
        assert!(retrieved.evaluator_pass);
    }

    #[test]
    fn test_promotion_store_both_roles() {
        let temp = tempdir().unwrap();
        let store = PromotionStore::new(temp.path()).unwrap();

        let artifact_id = "art_both".to_string();

        store
            .record_promotion(
                artifact_id.clone(),
                None,
                PromotionRole::Evaluator,
                "evaluator.default",
                true,
                vec![],
                None,
            )
            .unwrap();

        store
            .record_promotion(
                artifact_id.clone(),
                None,
                PromotionRole::Auditor,
                "auditor.default",
                true,
                vec![],
                None,
            )
            .unwrap();

        assert!(store.has_passed(&artifact_id, &PromotionRole::Evaluator));
        assert!(store.has_passed(&artifact_id, &PromotionRole::Auditor));
        assert!(store.is_fully_promoted(&artifact_id));
    }

    #[test]
    fn test_promotion_store_evaluator_fail() {
        let temp = tempdir().unwrap();
        let store = PromotionStore::new(temp.path()).unwrap();

        let artifact_id = "art_fail".to_string();

        store
            .record_promotion(
                artifact_id.clone(),
                None,
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

        assert!(!store.has_passed(&artifact_id, &PromotionRole::Evaluator));
        assert!(!store.is_fully_promoted(&artifact_id));
    }

    #[test]
    fn test_promotion_store_update_role() {
        let temp = tempdir().unwrap();
        let store = PromotionStore::new(temp.path()).unwrap();

        let artifact_id = "art_update".to_string();

        store
            .record_promotion(
                artifact_id.clone(),
                None,
                PromotionRole::Evaluator,
                "evaluator.default",
                false,
                vec![],
                None,
            )
            .unwrap();

        store
            .record_promotion(
                artifact_id.clone(),
                None,
                PromotionRole::Evaluator,
                "evaluator.default",
                true,
                vec![],
                None,
            )
            .unwrap();

        let record = store.get_promotion(&artifact_id).unwrap();
        assert!(record.evaluator_pass);
        assert_eq!(record.evaluator_id, Some("evaluator.default".to_string()));
    }

    #[test]
    fn test_promotion_store_persistence() {
        let temp = tempdir().unwrap();

        let artifact_id = "art_persist".to_string();

        {
            let store = PromotionStore::new(temp.path()).unwrap();
            store
                .record_promotion(
                    artifact_id.clone(),
                    None,
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
            let record = store.get_promotion(&artifact_id).unwrap();
            assert!(record.evaluator_pass);
            assert_eq!(record.evaluator_id, Some("evaluator.default".to_string()));
        }
    }

    #[test]
    fn test_promotion_store_not_found() {
        let temp = tempdir().unwrap();
        let store = PromotionStore::new(temp.path()).unwrap();

        assert!(store.get_promotion("art_nonexistent").is_none());
        assert!(!store.has_passed("art_nonexistent", &PromotionRole::Evaluator));
        assert!(!store.is_fully_promoted("art_nonexistent"));
    }
}
