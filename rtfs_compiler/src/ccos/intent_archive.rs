//! Intent Archive: Content-addressable, immutable storage for all intents
//!
//! Implements robust, efficient archival and retrieval using the unified CCOS storage abstraction.

use crate::runtime::error::RuntimeError;
use crate::runtime::values::Value;
use super::types::{Intent, IntentId};
use super::storage::{Archivable, ContentAddressableArchive, InMemoryArchive};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;

/// Implementation of Archivable for Intent
impl Archivable for Intent {
    type Id = IntentId;
    type SecondaryId = String; // We can use goal or status as secondary indexing
    
    fn primary_id(&self) -> &Self::Id {
        &self.intent_id
    }
    
    fn secondary_ids(&self) -> Vec<Self::SecondaryId> {
        // Index by goal and status for quick searches
        vec![
            format!("goal:{}", self.goal),
            format!("status:{:?}", self.status),
        ]
    }
    
    fn content_hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.intent_id.as_bytes());
        hasher.update(self.original_request.as_bytes());
        hasher.update(self.goal.as_bytes());
        
        // Hash constraints in sorted order for consistency
        let mut constraint_keys: Vec<_> = self.constraints.keys().collect();
        constraint_keys.sort();
        for key in constraint_keys {
            hasher.update(key.as_bytes());
            hasher.update(format!("{:?}", self.constraints[key]).as_bytes());
        }
        
        // Hash preferences in sorted order
        let mut pref_keys: Vec<_> = self.preferences.keys().collect();
        pref_keys.sort();
        for key in pref_keys {
            hasher.update(key.as_bytes());
            hasher.update(format!("{:?}", self.preferences[key]).as_bytes());
        }
        
        if let Some(ref criteria) = self.success_criteria {
            hasher.update(format!("{:?}", criteria).as_bytes());
        }
        
        hasher.update(format!("{:?}", self.status).as_bytes());
        hasher.update(self.created_at.to_le_bytes());
        
        format!("{:x}", hasher.finalize())
    }
    
    fn created_at(&self) -> u64 {
        self.created_at
    }
    
    fn metadata(&self) -> &HashMap<String, Value> {
        &self.metadata
    }
}

/// Trait for intent archives - allows multiple implementations
pub trait IntentArchive: ContentAddressableArchive<Intent> {
    /// Store an intent, returning its content hash
    fn archive_intent(&mut self, intent: Intent) -> Result<String, RuntimeError> {
        self.archive(intent)
    }
    
    /// Retrieve an intent by intent_id (primary ID)
    fn get_by_intent_id(&self, intent_id: &IntentId) -> Option<Arc<Intent>> {
        self.get_by_primary_id(intent_id)
    }
    
    /// Retrieve all intents with a specific goal
    fn get_by_goal(&self, goal: &str) -> Vec<Arc<Intent>> {
        self.get_by_secondary_id(&format!("goal:{}", goal))
    }
    
    /// Retrieve all intents with a specific status
    fn get_by_status(&self, status: &super::types::IntentStatus) -> Vec<Arc<Intent>> {
        self.get_by_secondary_id(&format!("status:{:?}", status))
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
    use super::*;
    use super::super::types::{Intent, IntentStatus};
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
        
        // Test Archivable implementation
        assert_eq!(intent.primary_id(), &"intent-123".to_string());
        let secondary_ids = intent.secondary_ids();
        assert!(secondary_ids.contains(&"goal:Complete testing".to_string()));
        assert!(secondary_ids.contains(&"status:Active".to_string()));
        assert_eq!(intent.created_at(), 123456789);
        
        // Hash should be consistent
        let hash1 = intent.content_hash();
        let hash2 = intent.content_hash();
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_in_memory_intent_archive() {
        let mut archive = create_in_memory_intent_archive();
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
        let retrieved = archive.get_by_hash(&hash).unwrap();
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
        assert_eq!(stats.unique_hashes, 1);
        
        // Test integrity
        archive.verify_integrity().unwrap();
    }
}
