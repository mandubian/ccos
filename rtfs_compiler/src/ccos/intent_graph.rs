//! Intent Graph Implementation
//!
//! This module implements the Living Intent Graph - a dynamic, multi-layered data structure
//! that stores and manages user intents with their relationships and lifecycle.

use super::intent_storage::{IntentFilter, IntentStorage, StorageFactory, StorageConfig};
use super::types::{EdgeType, ExecutionResult, StorableIntent, IntentId, IntentStatus};
use crate::runtime::error::RuntimeError;
use crate::runtime::values::Value;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Configuration for Intent Graph storage backend
#[derive(Debug, Clone)]
pub struct IntentGraphConfig {
    pub storage_path: Option<PathBuf>,
}

impl Default for IntentGraphConfig {
    fn default() -> Self {
        Self {
            storage_path: None,
        }
    }
}

impl IntentGraphConfig {
    pub fn with_file_storage(path: PathBuf) -> Self {
        Self {
            storage_path: Some(path),
        }
    }

    pub fn with_in_memory_storage() -> Self {
        Self {
            storage_path: None,
        }
    }
    
    pub fn to_storage_config(&self) -> StorageConfig {
        match &self.storage_path {
            Some(path) => StorageConfig::File { path: path.clone() },
            None => StorageConfig::InMemory,
        }
    }
}

/// Persistent storage backend for the Intent Graph with metadata caching
pub struct IntentGraphStorage {
    storage: Box<dyn IntentStorage>,
    metadata: HashMap<IntentId, IntentMetadata>,
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

    pub async fn get_edges(&self) -> Result<Vec<Edge>, RuntimeError> {
        self.storage.get_edges().await
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
        
        // Clear and rebuild metadata cache
        self.metadata.clear();
        let all_intents = self.storage.list_intents(IntentFilter::default()).await
            .map_err(|e| RuntimeError::StorageError(e.to_string()))?;
        
        for intent in all_intents {
            let metadata = IntentMetadata::new(&intent);
            self.metadata.insert(intent.intent_id.clone(), metadata);
        }
        
        Ok(())
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

/// Virtualization layer for context horizon management
pub struct IntentGraphVirtualization {
    context_manager: ContextWindowManager,
    semantic_search: SemanticSearchEngine,
    graph_traversal: GraphTraversalEngine,
}

impl IntentGraphVirtualization {
    pub fn new() -> Self {
        Self {
            context_manager: ContextWindowManager::new(),
            semantic_search: SemanticSearchEngine::new(),
            graph_traversal: GraphTraversalEngine::new(),
        }
    }

    pub fn find_relevant_intents(
        &self,
        query: &str,
        storage: &IntentGraphStorage,
    ) -> Vec<IntentId> {
        // Simple keyword-based search for now
        // In a full implementation, this would use semantic embeddings
        let mut relevant = Vec::new();

        // TODO: This should be implemented properly with async/await
        // For now, return empty to avoid compilation errors
        
        // Sort by relevance score
        relevant.sort_by(|a, b| {
            let score_a = storage
                .metadata
                .get(a)
                .map(|m| m.relevance_score)
                .unwrap_or(0.0);
            let score_b = storage
                .metadata
                .get(b)
                .map(|m| m.relevance_score)
                .unwrap_or(0.0);
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        relevant
    }

    pub fn load_context_window(
        &self,
        intent_ids: &[IntentId],
        storage: &IntentGraphStorage,
    ) -> Vec<StorableIntent> {
        // TODO: These methods need to be made async to work with the new storage interface
        // For now, return empty to enable compilation
        let context_intents = Vec::new();
        let _loaded_ids: HashSet<IntentId> = HashSet::new();

        // Load primary intents - needs async
        // for intent_id in intent_ids { ... }

        // Load related intents - needs async  
        // for intent_id in intent_ids { ... }

        context_intents
    }
}

/// Manages context window constraints
pub struct ContextWindowManager {
    max_intents: usize,
    max_tokens: usize,
}

impl ContextWindowManager {
    pub fn new() -> Self {
        Self {
            max_intents: 50,  // Reasonable default
            max_tokens: 8000, // Conservative token limit
        }
    }

    pub fn estimate_tokens(&self, intents: &[StorableIntent]) -> usize {
        let mut total_tokens = 0;

        for intent in intents {
            // Rough token estimation
            total_tokens += intent.goal.len() / 4; // ~4 chars per token
            total_tokens += intent.constraints.len() * 10; // ~10 tokens per constraint
            total_tokens += intent.preferences.len() * 8; // ~8 tokens per preference
        }

        total_tokens
    }

    pub fn should_truncate(&self, intents: &[StorableIntent]) -> bool {
        intents.len() > self.max_intents || self.estimate_tokens(intents) > self.max_tokens
    }
}

/// Semantic search engine (placeholder for now)
pub struct SemanticSearchEngine;

impl SemanticSearchEngine {
    pub fn new() -> Self {
        Self
    }
}

/// Graph traversal engine (placeholder for now)
pub struct GraphTraversalEngine;

impl GraphTraversalEngine {
    pub fn new() -> Self {
        Self
    }
}

/// Lifecycle management for intents
pub struct IntentLifecycleManager;

impl IntentLifecycleManager {
    pub async fn archive_completed_intents(
        &self,
        storage: &mut IntentGraphStorage,
    ) -> Result<(), RuntimeError> {
        let completed_filter = IntentFilter {
            status: Some(IntentStatus::Completed),
            ..Default::default()
        };
        
        let completed_intents = storage.list_intents(completed_filter).await?;

        for mut intent in completed_intents {
            intent.status = IntentStatus::Archived;
            intent.updated_at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            
            storage.update_intent(&intent).await?;
        }

        Ok(())
    }

    pub async fn infer_edges(&self, storage: &mut IntentGraphStorage) -> Result<(), RuntimeError> {
        // Simple edge inference based on goal similarity
        // In a full implementation, this would use more sophisticated NLP

        let all_intents = storage.list_intents(IntentFilter::default()).await?;

        for i in 0..all_intents.len() {
            for j in (i + 1)..all_intents.len() {
                let intent_a = &all_intents[i];
                let intent_b = &all_intents[j];

                // Check for potential conflicts based on resource constraints
                if self.detect_resource_conflict(intent_a, intent_b) {
                    let edge = Edge::new(
                        intent_a.intent_id.clone(),
                        intent_b.intent_id.clone(),
                        EdgeType::ConflictsWith,
                    );
                    storage.store_edge(edge).await?;
                }
            }
        }

        Ok(())
    }

    fn detect_resource_conflict(&self, intent_a: &StorableIntent, intent_b: &StorableIntent) -> bool {
        // Simple conflict detection based on cost constraints
        let cost_a = intent_a
            .constraints
            .get("max_cost")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(f64::INFINITY);
        let cost_b = intent_b
            .constraints
            .get("max_cost")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(f64::INFINITY);

        // If both have very low cost constraints, they might conflict
        cost_a < 10.0 && cost_b < 10.0
    }
}

/// Main Intent Graph implementation with persistent storage
pub struct IntentGraph {
    storage: IntentGraphStorage,
    virtualization: IntentGraphVirtualization,
    lifecycle: IntentLifecycleManager,
    rt: tokio::runtime::Handle,
}

impl IntentGraph {
    pub fn new() -> Result<Self, RuntimeError> {
        Self::with_config(IntentGraphConfig::default())
    }

    pub fn with_config(config: IntentGraphConfig) -> Result<Self, RuntimeError> {
        // For synchronous creation, we need to handle the case where no runtime exists
        let rt = if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle
        } else {
            // If no runtime exists, create a simple one for this instance
            let runtime = tokio::runtime::Runtime::new()
                .map_err(|e| RuntimeError::StorageError(format!("Failed to create runtime: {}", e)))?;
            let handle = runtime.handle().clone();
            // Keep the runtime alive
            std::mem::forget(runtime);
            handle
        };

        // Create storage synchronously using the runtime
        let storage = rt.block_on(async { IntentGraphStorage::new(config).await });

        Ok(Self {
            storage,
            virtualization: IntentGraphVirtualization::new(),
            lifecycle: IntentLifecycleManager,
            rt,
        })
    }

    /// Create a new IntentGraph asynchronously (for use within existing async contexts)
    pub async fn new_async(config: IntentGraphConfig) -> Result<Self, RuntimeError> {
        let storage = IntentGraphStorage::new(config).await;
        
        // Get the current runtime handle for future operations
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| RuntimeError::StorageError("No tokio runtime available".to_string()))?;

        Ok(Self {
            storage,
            virtualization: IntentGraphVirtualization::new(),
            lifecycle: IntentLifecycleManager,
            rt,
        })
    }

    /// Store a new intent in the graph
    pub fn store_intent(&mut self, intent: StorableIntent) -> Result<(), RuntimeError> {
        self.rt.block_on(async {
            self.storage.store_intent(intent).await?;
            self.lifecycle.infer_edges(&mut self.storage).await?;
            Ok(())
        })
    }

    /// Get an intent by ID
    pub fn get_intent(&self, intent_id: &IntentId) -> Option<StorableIntent> {
        self.rt.block_on(async {
            self.storage.get_intent(intent_id).await.unwrap_or(None)
        })
    }

    /// Update an intent with execution results
    pub fn update_intent(
        &mut self,
        intent: StorableIntent,
        result: &ExecutionResult,
    ) -> Result<(), RuntimeError> {
        let updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut intent = intent;
        intent.updated_at = updated_at;

        // Update status based on result
        intent.status = if result.success {
            IntentStatus::Completed
        } else {
            IntentStatus::Failed
        };

        self.rt.block_on(async {
            self.storage.update_intent(&intent).await
        })
    }

    /// Find relevant intents for a query
    pub fn find_relevant_intents(&self, query: &str) -> Vec<StorableIntent> {
        self.rt.block_on(async {
            let filter = IntentFilter {
                goal_contains: Some(query.to_string()),
                ..Default::default()
            };
            self.storage.list_intents(filter).await.unwrap_or_default()
        })
    }

    /// Load context window for a set of intent IDs
    pub fn load_context_window(&self, intent_ids: &[IntentId]) -> Vec<StorableIntent> {
        self.rt.block_on(async {
            let mut context_intents = Vec::new();
            let mut loaded_ids = HashSet::new();

            // Load primary intents
            for intent_id in intent_ids {
                if let Ok(Some(intent)) = self.storage.get_intent(intent_id).await {
                    context_intents.push(intent);
                    loaded_ids.insert(intent_id.clone());
                }
            }

            // Load related intents (dependencies, etc.)
            for intent_id in intent_ids {
                if let Ok(dependent) = self.storage.get_dependent_intents(intent_id).await {
                    for dep_intent in dependent {
                        if !loaded_ids.contains(&dep_intent.intent_id) {
                            context_intents.push(dep_intent.clone());
                            loaded_ids.insert(dep_intent.intent_id);
                        }
                    }
                }
            }

            context_intents
        })
    }

    /// Get related intents for a given intent
    pub fn get_related_intents(&self, intent_id: &IntentId) -> Vec<StorableIntent> {
        self.rt.block_on(async {
            self.storage.get_related_intents(intent_id).await.unwrap_or_default()
        })
    }

    /// Get dependent intents for a given intent
    pub fn get_dependent_intents(&self, intent_id: &IntentId) -> Vec<StorableIntent> {
        self.rt.block_on(async {
            self.storage.get_dependent_intents(intent_id).await.unwrap_or_default()
        })
    }

    /// Get subgoals for a given intent
    pub fn get_subgoals(&self, intent_id: &IntentId) -> Vec<StorableIntent> {
        self.rt.block_on(async {
            self.storage.get_subgoals(intent_id).await.unwrap_or_default()
        })
    }

    /// Get conflicting intents for a given intent
    pub fn get_conflicting_intents(&self, intent_id: &IntentId) -> Vec<StorableIntent> {
        self.rt.block_on(async {
            self.storage.get_conflicting_intents(intent_id).await.unwrap_or_default()
        })
    }

    /// Archive completed intents
    pub fn archive_completed_intents(&mut self) -> Result<(), RuntimeError> {
        self.rt.block_on(async {
            self.lifecycle.archive_completed_intents(&mut self.storage).await
        })
    }

    /// Get all active intents
    pub fn get_active_intents(&self) -> Vec<StorableIntent> {
        self.rt.block_on(async {
            let filter = IntentFilter {
                status: Some(IntentStatus::Active),
                ..Default::default()
            };
            self.storage.list_intents(filter).await.unwrap_or_default()
        })
    }

    /// Get intent count by status
    pub fn get_intent_count_by_status(&self) -> HashMap<IntentStatus, usize> {
        self.rt.block_on(async {
            let all_intents = self.storage.list_intents(IntentFilter::default()).await.unwrap_or_default();
            let mut counts = HashMap::new();

            for intent in all_intents {
                *counts.entry(intent.status).or_insert(0) += 1;
            }

            counts
        })
    }

    /// Create an edge between two intents
    pub fn create_edge(
        &mut self,
        from_intent: IntentId,
        to_intent: IntentId,
        edge_type: EdgeType,
    ) -> Result<(), RuntimeError> {
        let edge = Edge::new(from_intent, to_intent, edge_type);
        self.rt.block_on(async {
            self.storage.store_edge(edge).await
        })
    }

    /// Health check for the storage backend
    pub fn health_check(&self) -> Result<(), RuntimeError> {
        self.rt.block_on(async {
            self.storage.health_check().await
        })
    }

    /// Backup the intent graph to a file
    pub fn backup(&self, path: &std::path::Path) -> Result<(), RuntimeError> {
        self.rt.block_on(async {
            self.storage.backup(path).await
        })
    }

    /// Restore the intent graph from a backup file
    pub fn restore(&mut self, path: &std::path::Path) -> Result<(), RuntimeError> {
        self.rt.block_on(async {
            self.storage.restore(path).await
        })
    }
}

// Minimal Edge struct to resolve missing type errors
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[derive(PartialEq)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub edge_type: EdgeType,
}

impl Edge {
    pub fn new(from: String, to: String, edge_type: EdgeType) -> Self {
        Self { from, to, edge_type }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::values::Value;
    use tempfile::tempdir;
    use crate::ccos::types::StorableIntent;

    #[test]
    fn test_intent_graph_creation() {
        let graph = IntentGraph::new();
        assert!(graph.is_ok());
    }

    #[test]
    fn test_store_and_retrieve_intent() {
        let mut graph = IntentGraph::new().unwrap();
        let intent = StorableIntent::new("Test goal".to_string());
        let intent_id = intent.intent_id.clone();

        assert!(graph.store_intent(intent).is_ok());
        let retrieved = graph.get_intent(&intent_id);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().goal, "Test goal");
    }

    #[test]
    fn test_find_relevant_intents() {
        let mut graph = IntentGraph::new().unwrap();

        let intent1 = StorableIntent::new("Analyze sales data".to_string());
        let intent2 = StorableIntent::new("Generate report".to_string());
        let intent3 = StorableIntent::new("Send email".to_string());

        graph.store_intent(intent1).unwrap();
        graph.store_intent(intent2).unwrap();
        graph.store_intent(intent3).unwrap();

        let relevant = graph.find_relevant_intents("sales");
        assert_eq!(relevant.len(), 1);
        assert_eq!(relevant[0].goal, "Analyze sales data");
    }

    #[test]
    fn test_intent_lifecycle() {
        let mut graph = IntentGraph::new().unwrap();
        let intent = StorableIntent::new("Test goal".to_string());
        let intent_id = intent.intent_id.clone();

        graph.store_intent(intent).unwrap();

        // Initially active
        let retrieved = graph.get_intent(&intent_id).unwrap();
        assert_eq!(retrieved.status, IntentStatus::Active);

        // Update with successful result
        let result = ExecutionResult {
            success: true,
            value: Value::Nil,
            metadata: HashMap::new(),
        };

        // Update intent with the same ID
        let mut update_intent = StorableIntent::new("Test goal".to_string());
        update_intent.intent_id = intent_id.clone();
        graph.update_intent(update_intent, &result).unwrap();

        // Should be completed
        let final_intent = graph.get_intent(&intent_id).unwrap();
        assert_eq!(final_intent.status, IntentStatus::Completed);
    }

    #[test]
    fn test_file_storage_persistence() {
        let temp_dir = tempdir().unwrap();
        let storage_path = temp_dir.path().join("intent_graph.json");

        // Create graph with file storage
        let config = IntentGraphConfig::with_file_storage(storage_path.clone());
        let mut graph = IntentGraph::with_config(config).unwrap();

        let intent = StorableIntent::new("Persistent test goal".to_string());
        let intent_id = intent.intent_id.clone();

        graph.store_intent(intent).unwrap();

        // Create new graph instance and verify data persists
        let config2 = IntentGraphConfig::with_file_storage(storage_path);
        let graph2 = IntentGraph::with_config(config2).unwrap();
        
        let retrieved = graph2.get_intent(&intent_id);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().goal, "Persistent test goal");
    }

    #[test]
    fn test_intent_edges() {
        let mut graph = IntentGraph::new().unwrap();

        let intent1 = StorableIntent::new("Main task".to_string());
        let intent2 = StorableIntent::new("Dependent task".to_string());
        let intent1_id = intent1.intent_id.clone();
        let intent2_id = intent2.intent_id.clone();

        graph.store_intent(intent1).unwrap();
        graph.store_intent(intent2).unwrap();

        // Create dependency edge
        graph.create_edge(intent2_id.clone(), intent1_id.clone(), EdgeType::DependsOn).unwrap();

        // Check dependent intents
        let dependents = graph.get_dependent_intents(&intent1_id);
        assert_eq!(dependents.len(), 1);
        assert_eq!(dependents[0].goal, "Dependent task");
    }

    #[test]
    fn test_backup_restore() {
        let temp_dir = tempdir().unwrap();
        let backup_path = temp_dir.path().join("backup.json");

        let mut graph = IntentGraph::new().unwrap();
        let intent = StorableIntent::new("Backup test".to_string());
        let intent_id = intent.intent_id.clone();

        graph.store_intent(intent).unwrap();

        // Backup
        graph.backup(&backup_path).unwrap();

        // Create new graph and restore
        let mut new_graph = IntentGraph::new().unwrap();
        new_graph.restore(&backup_path).unwrap();

        let restored = new_graph.get_intent(&intent_id);
        assert!(restored.is_some());
        assert_eq!(restored.unwrap().goal, "Backup test");
    }

    #[test]
    fn test_active_intents_filter() {
        let mut graph = IntentGraph::new().unwrap();

        let mut intent1 = StorableIntent::new("Active task".to_string());
        intent1.status = IntentStatus::Active;

        let mut intent2 = StorableIntent::new("Completed task".to_string());
        intent2.status = IntentStatus::Completed;

        graph.store_intent(intent1).unwrap();
        graph.store_intent(intent2).unwrap();

        let active_intents = graph.get_active_intents();
        assert_eq!(active_intents.len(), 1);
        assert_eq!(active_intents[0].goal, "Active task");
    }

    #[test]
    fn test_health_check() {
        let graph = IntentGraph::new().unwrap();
        assert!(graph.health_check().is_ok());
    }
}
