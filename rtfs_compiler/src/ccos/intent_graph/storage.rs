//! Storage layer for Intent Graph

use super::super::intent_storage::{IntentFilter, IntentStorage, StorageFactory};
use super::super::types::{EdgeType, StorableIntent, IntentId};
use super::config::IntentGraphConfig;
use crate::runtime::error::RuntimeError;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Serialize, Deserialize};

/// Main storage wrapper for Intent Graph
pub struct IntentGraphStorage {
    storage: Box<dyn IntentStorage>,
    metadata: HashMap<IntentId, IntentMetadata>,
}

impl std::fmt::Debug for IntentGraphStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IntentGraphStorage")
            .field("storage", &"Box<dyn IntentStorage>")
            .field("metadata", &self.metadata)
            .finish()
    }
}

impl IntentGraphStorage {
    pub async fn new(config: IntentGraphConfig) -> Self {
        let storage = StorageFactory::create(config.to_storage_config()).await;
        Self {
            storage,
            metadata: HashMap::new(),
        }
    }

    pub async fn store_intent(&mut self, intent: StorableIntent) -> Result<(), RuntimeError> {
        let intent_id = intent.intent_id.clone();
        let metadata = IntentMetadata::new(&intent);
        
        self.storage.store_intent(intent).await
            .map_err(|e| RuntimeError::StorageError(e.to_string()))?;
        
        self.metadata.insert(intent_id, metadata);
        Ok(())
    }

    pub async fn get_intent(&self, intent_id: &IntentId) -> Result<Option<StorableIntent>, RuntimeError> {
        self.storage.get_intent(intent_id).await
            .map_err(|e| RuntimeError::StorageError(e.to_string()))
    }

    pub async fn update_intent(&mut self, intent: &StorableIntent) -> Result<(), RuntimeError> {
        self.storage.update_intent(intent.clone()).await
            .map_err(|e| RuntimeError::StorageError(e.to_string()))?;
        
        // Update metadata if it exists
        if let Some(metadata) = self.metadata.get_mut(&intent.intent_id) {
            metadata.last_accessed = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            metadata.access_count += 1;
        }
        
        Ok(())
    }

    pub async fn store_edge(&mut self, edge: Edge) -> Result<(), RuntimeError> {
        self.storage.store_edge(&edge).await
            .map_err(|e| RuntimeError::StorageError(e.to_string()))
    }

    pub async fn create_edge(&mut self, from: IntentId, to: IntentId, edge_type: EdgeType) -> Result<(), RuntimeError> {
        let edge = Edge {
            from,
            to,
            edge_type,
            metadata: Some(HashMap::new()),
            weight: None,
        };
        self.store_edge(edge).await
    }

    /// Delete an edge between two intents
    pub async fn delete_edge(&mut self, edge: &Edge) -> Result<(), RuntimeError> {
        self.storage.delete_edge(edge).await
            .map_err(|e| RuntimeError::StorageError(e.to_string()))
    }

    pub async fn get_edges(&self) -> Result<Vec<Edge>, RuntimeError> {
        self.storage.get_edges().await
            .map_err(|e| RuntimeError::StorageError(e.to_string()))
    }

    pub async fn get_edges_for_intent(&self, intent_id: &IntentId) -> Result<Vec<Edge>, RuntimeError> {
        self.storage.get_edges_for_intent(intent_id).await
            .map_err(|e| RuntimeError::StorageError(e.to_string()))
    }

    pub async fn get_related_intents(&self, intent_id: &IntentId) -> Result<Vec<StorableIntent>, RuntimeError> {
        let edges = self.storage.get_edges_for_intent(intent_id).await
            .map_err(|e| RuntimeError::StorageError(e.to_string()))?;
        
        let mut related = Vec::new();
        for edge in edges {
            let other_id = if edge.from == *intent_id { &edge.to } else { &edge.from };
            if let Some(intent) = self.storage.get_intent(other_id).await
                .map_err(|e| RuntimeError::StorageError(e.to_string()))? {
                related.push(intent);
            }
        }
        
        Ok(related)
    }

    pub async fn get_dependent_intents(&self, intent_id: &IntentId) -> Result<Vec<StorableIntent>, RuntimeError> {
        let edges = self.storage.get_edges_for_intent(intent_id).await
            .map_err(|e| RuntimeError::StorageError(e.to_string()))?;
        
        let mut dependent = Vec::new();
        for edge in edges {
            if edge.to == *intent_id && edge.edge_type == EdgeType::DependsOn {
                if let Some(intent) = self.storage.get_intent(&edge.from).await
                    .map_err(|e| RuntimeError::StorageError(e.to_string()))? {
                    dependent.push(intent);
                }
            }
        }
        
        Ok(dependent)
    }

    pub async fn get_subgoals(&self, intent_id: &IntentId) -> Result<Vec<StorableIntent>, RuntimeError> {
        let edges = self.storage.get_edges_for_intent(intent_id).await
            .map_err(|e| RuntimeError::StorageError(e.to_string()))?;
        
        let mut subgoals = Vec::new();
        for edge in edges {
            if edge.from == *intent_id && edge.edge_type == EdgeType::IsSubgoalOf {
                if let Some(intent) = self.storage.get_intent(&edge.to).await
                    .map_err(|e| RuntimeError::StorageError(e.to_string()))? {
                    subgoals.push(intent);
                }
            }
        }
        
        Ok(subgoals)
    }

    pub async fn get_conflicting_intents(&self, intent_id: &IntentId) -> Result<Vec<StorableIntent>, RuntimeError> {
        let edges = self.storage.get_edges_for_intent(intent_id).await
            .map_err(|e| RuntimeError::StorageError(e.to_string()))?;
        
        let mut conflicting = Vec::new();
        for edge in edges {
            if edge.edge_type == EdgeType::ConflictsWith {
                let other_id = if edge.from == *intent_id { &edge.to } else { &edge.from };
                if let Some(intent) = self.storage.get_intent(other_id).await
                    .map_err(|e| RuntimeError::StorageError(e.to_string()))? {
                    conflicting.push(intent);
                }
            }
        }
        
        Ok(conflicting)
    }

    pub async fn list_intents(&self, filter: IntentFilter) -> Result<Vec<StorableIntent>, RuntimeError> {
        self.storage.list_intents(filter).await
            .map_err(|e| RuntimeError::StorageError(e.to_string()))
    }

    pub async fn health_check(&self) -> Result<(), RuntimeError> {
        self.storage.health_check().await
            .map_err(|e| RuntimeError::StorageError(e.to_string()))
    }

    pub async fn backup(&self, path: &std::path::Path) -> Result<(), RuntimeError> {
        self.storage.backup(path).await
            .map_err(|e| RuntimeError::StorageError(e.to_string()))
    }

    pub async fn restore(&mut self, path: &std::path::Path) -> Result<(), RuntimeError> {
        self.storage.restore(path).await
            .map_err(|e| RuntimeError::StorageError(e.to_string()))?;
        
        // Rebuild metadata
        self.rebuild_metadata().await?;
        Ok(())
    }

    async fn rebuild_metadata(&mut self) -> Result<(), RuntimeError> {
        let all_intents = self.storage.list_intents(IntentFilter::default()).await
            .map_err(|e| RuntimeError::StorageError(e.to_string()))?;
        
        self.metadata.clear();
        for intent in all_intents {
            let metadata = IntentMetadata::new(&intent);
            self.metadata.insert(intent.intent_id.clone(), metadata);
        }
        
        Ok(())
    }

    // Sync helper methods for virtualization layer (blocking, for compatibility)
    pub fn get_intent_sync(&self, intent_id: &IntentId) -> Option<StorableIntent> {
        // Note: This is a temporary solution. In production, this should be async
        futures::executor::block_on(self.get_intent(intent_id)).ok().flatten()
    }

    pub fn get_all_intents_sync(&self) -> Vec<StorableIntent> {
        futures::executor::block_on(self.list_intents(IntentFilter::default())).unwrap_or_default()
    }

    pub fn get_connected_intents_sync(&self, intent_id: &IntentId) -> Vec<IntentId> {
        let edges = futures::executor::block_on(self.get_edges_for_intent(intent_id)).unwrap_or_default();
        let mut connected = Vec::new();
        
        for edge in edges {
            if edge.from == *intent_id {
                connected.push(edge.to);
            } else if edge.to == *intent_id {
                connected.push(edge.from);
            }
        }
        
        connected
    }

    pub fn has_edge_sync(&self, from: &IntentId, to: &IntentId) -> bool {
        let edges = futures::executor::block_on(self.get_edges()).unwrap_or_default();
        edges.iter().any(|edge| 
            (edge.from == *from && edge.to == *to) || 
            (edge.from == *to && edge.to == *from)
        )
    }
}

/// Metadata for intent graph operations
#[derive(Debug, Clone)]
pub struct IntentMetadata {
    pub last_accessed: u64,
    pub access_count: u64,
    pub relevance_score: f64,
    pub complexity_score: f64,
}

impl IntentMetadata {
    pub fn new(intent: &StorableIntent) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            last_accessed: now,
            access_count: 0,
            relevance_score: 0.5, // Default neutral score
            complexity_score: Self::calculate_complexity(intent),
        }
    }

    fn calculate_complexity(intent: &StorableIntent) -> f64 {
        let mut complexity = 0.0;

        // Base complexity from goal length
        complexity += intent.goal.len() as f64 * 0.01;

        // Complexity from constraints
        complexity += intent.constraints.len() as f64 * 0.1;

        // Complexity from preferences
        complexity += intent.preferences.len() as f64 * 0.05;

        // Complexity from success criteria
        if intent.success_criteria.is_some() {
            complexity += 0.5;
        }

        complexity.min(1.0) // Cap at 1.0
    }
}

/// Edge representation for the intent graph
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Edge {
    pub from: IntentId,
    pub to: IntentId,
    pub edge_type: EdgeType,
    pub metadata: Option<HashMap<String, String>>,
    pub weight: Option<f64>,
}

impl Edge {
    pub fn new(from: IntentId, to: IntentId, edge_type: EdgeType) -> Self {
        Self {
            from,
            to,
            edge_type,
            metadata: None,
            weight: None,
        }
    }
    
    pub fn with_weight(mut self, weight: f64) -> Self {
        self.weight = Some(weight);
        self
    }
    
    pub fn with_metadata(mut self, metadata: HashMap<String, String>) -> Self {
        self.metadata = Some(metadata);
        self
    }
}
