//! Action Archive: Content-addressable, immutable storage for all actions
//!
//! Implements robust, efficient archival and retrieval using the unified CCOS storage abstraction.

use crate::runtime::error::RuntimeError;
use crate::runtime::values::Value;
use super::types::{Action, ActionId, IntentId, PlanId, ActionType};
use super::storage::{Archivable, ContentAddressableArchive, InMemoryArchive};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;

/// Implementation of Archivable for Action
impl Archivable for Action {
    type Id = ActionId;
    type SecondaryId = String; // We can use plan_id, intent_id, or action_type as secondary indexing
    
    fn primary_id(&self) -> &Self::Id {
        &self.id
    }
    
    fn secondary_ids(&self) -> Vec<Self::SecondaryId> {
        let mut secondary_ids = vec![
            format!("type:{:?}", self.action_type),
            format!("function:{}", self.function_name),
        ];
        
        if let Some(ref plan_id) = self.plan_id {
            secondary_ids.push(format!("plan:{}", plan_id));
        }
        
        if let Some(ref intent_id) = self.intent_id {
            secondary_ids.push(format!("intent:{}", intent_id));
        }
        
        secondary_ids
    }
    
    fn content_hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.id.as_bytes());
        hasher.update(format!("{:?}", self.action_type).as_bytes());
        hasher.update(self.function_name.as_bytes());
        
        // Hash parameters in sorted order for consistency
        let mut param_keys: Vec<_> = self.parameters.keys().collect();
        param_keys.sort();
        for key in param_keys {
            hasher.update(key.as_bytes());
            hasher.update(format!("{:?}", self.parameters[key]).as_bytes());
        }
        
        if let Some(ref result) = self.result {
            hasher.update(format!("{:?}", result).as_bytes());
        }
        
        if let Some(ref plan_id) = self.plan_id {
            hasher.update(plan_id.as_bytes());
        }
        
        if let Some(ref intent_id) = self.intent_id {
            hasher.update(intent_id.as_bytes());
        }
        
        hasher.update(self.timestamp.to_le_bytes());
        
        // Hash metadata in sorted order
        let mut meta_keys: Vec<_> = self.metadata.keys().collect();
        meta_keys.sort();
        for key in meta_keys {
            hasher.update(key.as_bytes());
            hasher.update(format!("{:?}", self.metadata[key]).as_bytes());
        }
        
        format!("{:x}", hasher.finalize())
    }
    
    fn created_at(&self) -> u64 {
        self.timestamp
    }
    
    fn metadata(&self) -> &HashMap<String, Value> {
        &self.metadata
    }
}

/// Trait for action archives - allows multiple implementations
pub trait ActionArchive: ContentAddressableArchive<Action> {
    /// Store an action, returning its content hash
    fn archive_action(&mut self, action: Action) -> Result<String, RuntimeError> {
        self.archive(action)
    }
    
    /// Retrieve an action by action_id (primary ID)
    fn get_by_action_id(&self, action_id: &ActionId) -> Option<Arc<Action>> {
        self.get_by_primary_id(action_id)
    }
    
    /// Retrieve all actions for a specific plan
    fn get_by_plan_id(&self, plan_id: &PlanId) -> Vec<Arc<Action>> {
        self.get_by_secondary_id(&format!("plan:{}", plan_id))
    }
    
    /// Retrieve all actions for a specific intent
    fn get_by_intent_id(&self, intent_id: &IntentId) -> Vec<Arc<Action>> {
        self.get_by_secondary_id(&format!("intent:{}", intent_id))
    }
    
    /// Retrieve all actions of a specific type
    fn get_by_action_type(&self, action_type: &ActionType) -> Vec<Arc<Action>> {
        self.get_by_secondary_id(&format!("type:{:?}", action_type))
    }
    
    /// Retrieve all actions that called a specific function
    fn get_by_function_name(&self, function_name: &str) -> Vec<Arc<Action>> {
        self.get_by_secondary_id(&format!("function:{}", function_name))
    }
}

/// In-memory implementation of ActionArchive
pub type InMemoryActionArchive = InMemoryArchive<Action>;

impl ActionArchive for InMemoryActionArchive {}

/// Convenience constructor for in-memory action archive
pub fn create_in_memory_action_archive() -> InMemoryActionArchive {
    InMemoryActionArchive::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::types::{Action, ActionType};
    use std::collections::HashMap;

    #[test]
    fn test_action_archivable_implementation() {
        let action = Action {
            id: "action-123".to_string(),
            action_type: ActionType::CapabilityCall,
            function_name: "test_capability".to_string(),
            parameters: HashMap::new(),
            result: Some(Value::String("success".to_string())),
            plan_id: Some("plan-456".to_string()),
            intent_id: Some("intent-789".to_string()),
            timestamp: 123456789,
            metadata: HashMap::new(),
        };
        
        // Test Archivable implementation
        assert_eq!(action.primary_id(), &"action-123".to_string());
        let secondary_ids = action.secondary_ids();
        assert!(secondary_ids.contains(&"type:CapabilityCall".to_string()));
        assert!(secondary_ids.contains(&"function:test_capability".to_string()));
        assert!(secondary_ids.contains(&"plan:plan-456".to_string()));
        assert!(secondary_ids.contains(&"intent:intent-789".to_string()));
        assert_eq!(action.created_at(), 123456789);
        
        // Hash should be consistent
        let hash1 = action.content_hash();
        let hash2 = action.content_hash();
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_in_memory_action_archive() {
        let mut archive = create_in_memory_action_archive();
        let action = Action {
            id: "action-123".to_string(),
            action_type: ActionType::CapabilityCall,
            function_name: "test_capability".to_string(),
            parameters: HashMap::new(),
            result: Some(Value::String("success".to_string())),
            plan_id: Some("plan-456".to_string()),
            intent_id: Some("intent-789".to_string()),
            timestamp: 123456789,
            metadata: HashMap::new(),
        };
        
        let hash = archive.archive_action(action.clone()).unwrap();
        
        // Test retrieval by hash
        let retrieved = archive.get_by_hash(&hash).unwrap();
        assert_eq!(retrieved.id, "action-123");
        
        // Test retrieval by action_id
        let by_id = archive.get_by_action_id(&"action-123".to_string()).unwrap();
        assert_eq!(by_id.id, "action-123");
        
        // Test retrieval by plan_id
        let by_plan = archive.get_by_plan_id(&"plan-456".to_string());
        assert_eq!(by_plan.len(), 1);
        assert_eq!(by_plan[0].id, "action-123");
        
        // Test retrieval by intent_id
        let by_intent = archive.get_by_intent_id(&"intent-789".to_string());
        assert_eq!(by_intent.len(), 1);
        assert_eq!(by_intent[0].id, "action-123");
        
        // Test retrieval by action type
        let by_type = archive.get_by_action_type(&ActionType::CapabilityCall);
        assert_eq!(by_type.len(), 1);
        assert_eq!(by_type[0].id, "action-123");
        
        // Test retrieval by function name
        let by_function = archive.get_by_function_name("test_capability");
        assert_eq!(by_function.len(), 1);
        assert_eq!(by_function[0].id, "action-123");
        
        // Test stats
        let stats = archive.stats();
        assert_eq!(stats.total_entities, 1);
        assert_eq!(stats.unique_hashes, 1);
        
        // Test integrity
        archive.verify_integrity().unwrap();
    }
}
