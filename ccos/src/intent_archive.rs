//! Intent Archive: Content-addressable, immutable storage for all intents
//!
//! Implements robust, efficient archival and retrieval using the unified CCOS storage abstraction.

use super::storage::{Archivable, ContentAddressableArchive, InMemoryArchive};
use super::types::{Intent, IntentId};
use rtfs::runtime::error::RuntimeError;
use std::sync::Arc;

/// Implementation of Archivable for Intent
impl Archivable for Intent {
    // Use the default canonical JSON-based content_hash() provided by the Archivable trait
    fn entity_id(&self) -> String {
        self.intent_id.clone()
    }

    fn entity_type(&self) -> &'static str {
        "Intent"
    }
}

/// Trait for intent archives - allows multiple implementations
pub trait IntentArchive: ContentAddressableArchive<Intent> {
    /// Store an intent, returning its content hash
    fn archive_intent(&self, intent: Intent) -> Result<String, RuntimeError> {
        self.store(intent).map_err(|e| RuntimeError::Generic(e))
    }

    /// Retrieve an intent by intent_id (primary ID) by scanning stored entities.
    fn get_by_intent_id(&self, intent_id: &IntentId) -> Option<Arc<Intent>> {
        for hash in self.list_hashes() {
            if let Ok(Some(ent)) = self.retrieve(&hash) {
                if ent.entity_id() == *intent_id {
                    return Some(Arc::new(ent));
                }
            }
        }
        None
    }

    /// Retrieve all intents with a specific goal
    fn get_by_goal(&self, goal: &str) -> Vec<Arc<Intent>> {
        let mut out = Vec::new();
        for hash in self.list_hashes() {
            if let Ok(Some(ent)) = self.retrieve(&hash) {
                if ent.goal == goal {
                    out.push(Arc::new(ent));
                }
            }
        }
        out
    }

    /// Retrieve all intents with a specific status
    fn get_by_status(&self, status: &super::types::IntentStatus) -> Vec<Arc<Intent>> {
        let mut out = Vec::new();
        for hash in self.list_hashes() {
            if let Ok(Some(ent)) = self.retrieve(&hash) {
                if ent.status == *status {
                    out.push(Arc::new(ent));
                }
            }
        }
        out
    }
}

/// In-memory implementation of IntentArchive
pub type InMemoryIntentArchive = InMemoryArchive<Intent>;

impl IntentArchive for InMemoryIntentArchive {}

/// Convenience constructor for in-memory intent archive
pub fn create_in_memory_intent_archive() -> InMemoryIntentArchive {
    InMemoryIntentArchive::new()
}

#[cfg(test)]
mod tests {
    use super::super::types::{Intent, IntentStatus};
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_intent_archivable_implementation() {
        let intent = Intent {
            intent_id: "intent-123".to_string(),
            name: Some("Test Intent".to_string()),
            original_request: "Test request".to_string(),
            goal: "Complete testing".to_string(),
            constraints: HashMap::new(),
            preferences: HashMap::new(),
            success_criteria: None,
            status: IntentStatus::Active,
            created_at: 123456789,
            updated_at: 123456789,
            metadata: HashMap::new(),
        };

        // Test basic fields are present and correct
        assert_eq!(intent.intent_id, "intent-123");
        assert_eq!(intent.goal, "Complete testing");
        assert_eq!(intent.status, IntentStatus::Active);
        assert_eq!(intent.created_at, 123456789);

        // Hash should be consistent
        let hash1 = intent.content_hash();
        let hash2 = intent.content_hash();
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_in_memory_intent_archive() {
        let archive = create_in_memory_intent_archive();
        let intent = Intent {
            intent_id: "intent-123".to_string(),
            name: Some("Test Intent".to_string()),
            original_request: "Test request".to_string(),
            goal: "Complete testing".to_string(),
            constraints: HashMap::new(),
            preferences: HashMap::new(),
            success_criteria: None,
            status: IntentStatus::Active,
            created_at: 123456789,
            updated_at: 123456789,
            metadata: HashMap::new(),
        };

        let hash = archive.archive_intent(intent.clone()).unwrap();

        // Test retrieval by hash
        let retrieved = archive.retrieve(&hash).unwrap();
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.intent_id, "intent-123");

        // Test retrieval by intent_id
        let by_id = archive.get_by_intent_id(&"intent-123".to_string()).unwrap();
        assert_eq!(by_id.intent_id, "intent-123");

        // Test retrieval by goal
        let by_goal = archive.get_by_goal("Complete testing");
        assert_eq!(by_goal.len(), 1);
        assert_eq!(by_goal[0].intent_id, "intent-123");

        // Test retrieval by status
        let by_status = archive.get_by_status(&IntentStatus::Active);
        assert_eq!(by_status.len(), 1);
        assert_eq!(by_status[0].intent_id, "intent-123");

        // Test stats
        let stats = archive.stats();
        assert_eq!(stats.total_entities, 1);

        // Test integrity
        archive.verify_integrity().unwrap();
    }
}
