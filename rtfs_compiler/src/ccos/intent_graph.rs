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
use serde::{Serialize, Deserialize};

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

    /// Create an edge with weight and metadata
    pub fn create_weighted_edge(
        &mut self,
        from_intent: IntentId,
        to_intent: IntentId,
        edge_type: EdgeType,
        weight: f64,
        metadata: HashMap<String, String>,
    ) -> Result<(), RuntimeError> {
        let edge = Edge::new(from_intent, to_intent, edge_type)
            .with_weight(weight)
            .with_metadata_map(metadata);
        self.rt.block_on(async {
            self.storage.store_edge(edge).await
        })
    }

    /// Get all edges for a specific intent
    pub fn get_edges_for_intent(&self, intent_id: &IntentId) -> Vec<Edge> {
        self.rt.block_on(async {
            self.storage.get_edges_for_intent(intent_id).await
                .unwrap_or_else(|_| Vec::new())
        })
    }

    /// Get parent intents (intents that this intent depends on)
    pub fn get_parent_intents(&self, intent_id: &IntentId) -> Vec<StorableIntent> {
        let edges = self.get_edges_for_intent(intent_id);
        let mut parents = Vec::new();
        
        for edge in edges {
            // For parent relationship: intent_id is the 'from' field, parent is the 'to' field
            if edge.from == *intent_id && (edge.edge_type == EdgeType::DependsOn || edge.edge_type == EdgeType::IsSubgoalOf) {
                if let Some(parent) = self.get_intent(&edge.to) {
                    parents.push(parent);
                }
            }
        }
        
        parents
    }

    /// Get child intents (intents that depend on this intent)
    pub fn get_child_intents(&self, intent_id: &IntentId) -> Vec<StorableIntent> {
        let edges = self.get_edges_for_intent(intent_id);
        let mut children = Vec::new();
        
        for edge in edges {
            // For child relationship: intent_id is the 'to' field, child is the 'from' field
            if edge.to == *intent_id && (edge.edge_type == EdgeType::DependsOn || edge.edge_type == EdgeType::IsSubgoalOf) {
                if let Some(child) = self.get_intent(&edge.from) {
                    children.push(child);
                }
            }
        }
        
        children
    }

    /// Get the complete hierarchy for an intent (parents and children)
    pub fn get_intent_hierarchy(&self, intent_id: &IntentId) -> Vec<StorableIntent> {
        let mut hierarchy = Vec::new();
        let mut visited = HashSet::new();
        
        self.collect_hierarchy_recursive(intent_id, &mut hierarchy, &mut visited);
        
        hierarchy
    }
    
    /// Helper method to collect hierarchy recursively with cycle detection
    fn collect_hierarchy_recursive(
        &self,
        intent_id: &IntentId,
        hierarchy: &mut Vec<StorableIntent>,
        visited: &mut HashSet<IntentId>,
    ) {
        // Prevent cycles
        if visited.contains(intent_id) {
            return;
        }
        visited.insert(intent_id.clone());
        
        // Add the current intent
        if let Some(intent) = self.get_intent(intent_id) {
            hierarchy.push(intent);
        }
        
        // Add all parents recursively
        let parents = self.get_parent_intents(intent_id);
        for parent in &parents {
            self.collect_hierarchy_recursive(&parent.intent_id, hierarchy, visited);
        }
        
        // Add all children recursively
        let children = self.get_child_intents(intent_id);
        for child in &children {
            self.collect_hierarchy_recursive(&child.intent_id, hierarchy, visited);
        }
    }

    /// Find intents by edge type relationship
    pub fn find_intents_by_relationship(&self, intent_id: &IntentId, edge_type: EdgeType) -> Vec<StorableIntent> {
        let edges = self.get_edges_for_intent(intent_id);
        let mut related = Vec::new();
        
        for edge in edges {
            if edge.edge_type == edge_type {
                let related_id = if edge.from == *intent_id { &edge.to } else { &edge.from };
                if let Some(intent) = self.get_intent(related_id) {
                    related.push(intent);
                }
            }
        }
        
        related
    }

    /// Get strongly connected intents (bidirectional relationships)
    pub fn get_strongly_connected_intents(&self, intent_id: &IntentId) -> Vec<StorableIntent> {
        let edges = self.get_edges_for_intent(intent_id);
        let mut connected = HashSet::new();
        
        for edge in edges {
            let other_id = if edge.from == *intent_id { &edge.to } else { &edge.from };
            
            // Check if there's a reverse edge of the same type
            let reverse_edges = self.get_edges_for_intent(other_id);
            let has_reverse = reverse_edges.iter().any(|e| {
                // For a true bidirectional relationship, we need:
                // 1. The reverse edge goes from the other intent to this intent
                // 2. It's the same edge type
                // 3. It's not the same edge (different direction)
                e.from == *other_id && 
                e.to == *intent_id && 
                e.edge_type == edge.edge_type &&
                !(e.from == edge.from && e.to == edge.to) // Not the same edge
            });
            
            if has_reverse {
                if let Some(intent) = self.get_intent(other_id) {
                    connected.insert(intent.intent_id.clone());
                }
            }
        }
        
        // Convert back to Vec<StorableIntent>
        connected.into_iter()
            .filter_map(|id| self.get_intent(&id))
            .collect()
    }

    /// Calculate relationship strength between two intents
    pub fn get_relationship_strength(&self, intent_a: &IntentId, intent_b: &IntentId) -> f64 {
        let edges = self.get_edges_for_intent(intent_a);
        
        for edge in edges {
            if edge.to == *intent_b || edge.from == *intent_b {
                return edge.weight.unwrap_or(1.0);
            }
        }
        
        0.0 // No relationship found
    }

    /// Find intents with high relationship weights
    pub fn get_high_weight_relationships(&self, intent_id: &IntentId, threshold: f64) -> Vec<(StorableIntent, f64)> {
        let edges = self.get_edges_for_intent(intent_id);
        let mut high_weight = Vec::new();
        
        for edge in edges {
            if let Some(weight) = edge.weight {
                if weight >= threshold {
                    let other_id = if edge.from == *intent_id { &edge.to } else { &edge.from };
                    if let Some(intent) = self.get_intent(other_id) {
                        high_weight.push((intent, weight));
                    }
                }
            }
        }
        
        // Sort by weight descending
        high_weight.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        high_weight
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

    /// Store an entire subgraph starting from a root intent
    /// This stores the root intent and all its descendants (children, grandchildren, etc.)
    pub fn store_subgraph_from_root(&mut self, root_intent_id: &IntentId, path: &std::path::Path) -> Result<(), RuntimeError> {
        self.rt.block_on(async {
            // Get the root intent
            let root_intent = self.storage.get_intent(root_intent_id).await?;
            if root_intent.is_none() {
                return Err(RuntimeError::StorageError(format!("Root intent {} not found", root_intent_id)));
            }
            
            // Collect all descendants recursively
            let mut subgraph_intents = vec![root_intent.unwrap()];
            let mut subgraph_edges = Vec::new();
            let mut visited = HashSet::new();
            
            self.collect_subgraph_recursive(root_intent_id, &mut subgraph_intents, &mut subgraph_edges, &mut visited).await?;
            
            // Create backup data for the subgraph
            let backup_data = SubgraphBackupData {
                intents: subgraph_intents.into_iter().map(|i| (i.intent_id.clone(), i)).collect(),
                edges: subgraph_edges,
                root_intent_id: root_intent_id.clone(),
                version: "1.0".to_string(),
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            };
            
            // Serialize and save
            let json = serde_json::to_string_pretty(&backup_data)
                .map_err(|e| RuntimeError::StorageError(format!("Serialization error: {}", e)))?;
            
            tokio::fs::write(path, json).await
                .map_err(|e| RuntimeError::StorageError(format!("IO error: {}", e)))?;
            
            Ok(())
        })
    }

    /// Store an entire subgraph containing a child intent and all its ancestors
    /// This stores the child intent and all its parents (up to root intents)
    pub fn store_subgraph_from_child(&mut self, child_intent_id: &IntentId, path: &std::path::Path) -> Result<(), RuntimeError> {
        self.rt.block_on(async {
            // Get the child intent
            let child_intent = self.storage.get_intent(child_intent_id).await?;
            if child_intent.is_none() {
                return Err(RuntimeError::StorageError(format!("Child intent {} not found", child_intent_id)));
            }
            
            // Collect all ancestors recursively
            let mut subgraph_intents = vec![child_intent.unwrap()];
            let mut subgraph_edges = Vec::new();
            let mut visited = HashSet::new();
            
            self.collect_ancestor_subgraph_recursive(child_intent_id, &mut subgraph_intents, &mut subgraph_edges, &mut visited).await?;
            
            // Create backup data for the subgraph
            let backup_data = SubgraphBackupData {
                intents: subgraph_intents.into_iter().map(|i| (i.intent_id.clone(), i)).collect(),
                edges: subgraph_edges,
                root_intent_id: child_intent_id.clone(), // For child-based subgraphs, use child as reference
                version: "1.0".to_string(),
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            };
            
            // Serialize and save
            let json = serde_json::to_string_pretty(&backup_data)
                .map_err(|e| RuntimeError::StorageError(format!("Serialization error: {}", e)))?;
            
            tokio::fs::write(path, json).await
                .map_err(|e| RuntimeError::StorageError(format!("IO error: {}", e)))?;
            
            Ok(())
        })
    }

    /// Restore a subgraph from a backup file
    /// This restores intents and edges without affecting existing data
    pub fn restore_subgraph(&mut self, path: &std::path::Path) -> Result<(), RuntimeError> {
        self.rt.block_on(async {
            // Read and deserialize the backup data
            let content = tokio::fs::read_to_string(path).await
                .map_err(|e| RuntimeError::StorageError(format!("IO error: {}", e)))?;
            
            let backup_data: SubgraphBackupData = serde_json::from_str(&content)
                .map_err(|e| RuntimeError::StorageError(format!("Deserialization error: {}", e)))?;
            
            // Restore intents
            for (_, intent) in backup_data.intents {
                self.storage.store_intent(intent).await?;
            }
            
            // Restore edges
            for edge in backup_data.edges {
                self.storage.store_edge(edge).await?;
            }
            
            Ok(())
        })
    }

    /// Helper method to collect all descendants of a root intent
    async fn collect_subgraph_recursive(
        &self,
        intent_id: &IntentId,
        intents: &mut Vec<StorableIntent>,
        edges: &mut Vec<Edge>,
        visited: &mut HashSet<IntentId>,
    ) -> Result<(), RuntimeError> {
        if visited.contains(intent_id) {
            return Ok(());
        }
        visited.insert(intent_id.clone());
        
        // Get all edges for this intent
        let all_edges = self.storage.get_edges_for_intent(intent_id).await?;
        
        for edge in all_edges {
            // For child relationships in IsSubgoalOf: 
            // If edge is "A -> B" with type IsSubgoalOf, it means "A is a subgoal of B"
            // So B (the 'to' field) is the parent, and A (the 'from' field) is the child
            // If we're looking for children of intent_id, we need edges where intent_id is the 'to' field
            if edge.to == *intent_id && edge.edge_type == EdgeType::IsSubgoalOf {
                // This is a child relationship
                edges.push(edge.clone());
                
                // Get the child intent (the 'from' field)
                if let Some(child_intent) = self.storage.get_intent(&edge.from).await? {
                    intents.push(child_intent);
                    
                    // Recursively collect descendants using Box::pin
                    Box::pin(self.collect_subgraph_recursive(&edge.from, intents, edges, visited)).await?;
                }
            }
            
            // Also include all other edges that connect intents in the subgraph
            // This ensures that RelatedTo, DependsOn, etc. edges are preserved
            if edge.from == *intent_id || edge.to == *intent_id {
                // Check if the other intent is already in our subgraph or will be added
                let other_id = if edge.from == *intent_id { &edge.to } else { &edge.from };
                
                // If this is not an IsSubgoalOf edge, we need to check if the other intent
                // is part of our hierarchical subgraph
                if edge.edge_type != EdgeType::IsSubgoalOf {
                    // For non-hierarchical edges, we need to ensure the other intent is in our subgraph
                    // We'll add it if it's not already visited and not already in our intents list
                    if !visited.contains(other_id) && !intents.iter().any(|i| i.intent_id == *other_id) {
                        if let Some(other_intent) = self.storage.get_intent(other_id).await? {
                            intents.push(other_intent);
                        }
                    }
                }
                
                // Add the edge if it's not already in our list
                if !edges.iter().any(|e| e.from == edge.from && e.to == edge.to && e.edge_type == edge.edge_type) {
                    edges.push(edge.clone());
                }
            }
        }
        
        Ok(())
    }

    /// Helper method to collect all ancestors of a child intent
    async fn collect_ancestor_subgraph_recursive(
        &self,
        intent_id: &IntentId,
        intents: &mut Vec<StorableIntent>,
        edges: &mut Vec<Edge>,
        visited: &mut HashSet<IntentId>,
    ) -> Result<(), RuntimeError> {
        if visited.contains(intent_id) {
            return Ok(());
        }
        visited.insert(intent_id.clone());
        
        // Get all edges for this intent
        let all_edges = self.storage.get_edges_for_intent(intent_id).await?;
        
        for edge in all_edges {
            // For parent relationships in IsSubgoalOf:
            // If edge is "A -> B" with type IsSubgoalOf, it means "A is a subgoal of B"
            // So B (the 'to' field) is the parent, and A (the 'from' field) is the child
            // If we're looking for parents of intent_id, we need edges where intent_id is the 'from' field
            if edge.from == *intent_id && edge.edge_type == EdgeType::IsSubgoalOf {
                // This is a parent relationship
                edges.push(edge.clone());
                
                // Get the parent intent (the 'to' field)
                if let Some(parent_intent) = self.storage.get_intent(&edge.to).await? {
                    intents.push(parent_intent);
                    
                    // Recursively collect ancestors using Box::pin
                    Box::pin(self.collect_ancestor_subgraph_recursive(&edge.to, intents, edges, visited)).await?;
                }
            }
        }
        
        Ok(())
    }
}

// Minimal Edge struct to resolve missing type errors
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[derive(PartialEq)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub edge_type: EdgeType,
    pub weight: Option<f64>,
    pub metadata: HashMap<String, String>,
}

impl Edge {
    pub fn new(from: String, to: String, edge_type: EdgeType) -> Self {
        Self { 
            from, 
            to, 
            edge_type,
            weight: None,
            metadata: HashMap::new(),
        }
    }

    pub fn with_weight(mut self, weight: f64) -> Self {
        self.weight = Some(weight);
        self
    }

    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    pub fn with_metadata_map(mut self, metadata: HashMap<String, String>) -> Self {
        self.metadata = metadata;
        self
    }
}

/// Backup data structure for serialization
#[derive(Debug, Serialize, Deserialize)]
struct StorageBackupData {
    intents: HashMap<IntentId, StorableIntent>,
    edges: Vec<Edge>,
    version: String,
    timestamp: u64,
}

/// Subgraph backup data structure for partial graph storage
#[derive(Debug, Serialize, Deserialize)]
struct SubgraphBackupData {
    intents: HashMap<IntentId, StorableIntent>,
    edges: Vec<Edge>,
    root_intent_id: IntentId, // Reference point for the subgraph
    version: String,
    timestamp: u64,
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

    #[test]
    fn test_weighted_edges() {
        let mut graph = IntentGraph::new().unwrap();
        
        let intent1 = StorableIntent::new("Parent goal".to_string());
        let intent2 = StorableIntent::new("Child goal".to_string());
        let intent3 = StorableIntent::new("Related goal".to_string());
        
        let intent1_id = intent1.intent_id.clone();
        let intent2_id = intent2.intent_id.clone();
        let intent3_id = intent3.intent_id.clone();
        
        graph.store_intent(intent1).unwrap();
        graph.store_intent(intent2).unwrap();
        graph.store_intent(intent3).unwrap();
        
        // Create weighted edges
        let mut metadata = HashMap::new();
        metadata.insert("reason".to_string(), "strong dependency".to_string());
        
        graph.create_weighted_edge(
            intent1_id.clone(),
            intent2_id.clone(),
            EdgeType::DependsOn,
            0.8,
            metadata.clone(),
        ).unwrap();
        
        graph.create_weighted_edge(
            intent1_id.clone(),
            intent3_id.clone(),
            EdgeType::RelatedTo,
            0.3,
            HashMap::new(),
        ).unwrap();
        
        // Test relationship strength
        let strength = graph.get_relationship_strength(&intent1_id, &intent2_id);
        assert_eq!(strength, 0.8);
        
        let strength = graph.get_relationship_strength(&intent1_id, &intent3_id);
        assert_eq!(strength, 0.3);
        
        // Test high weight relationships
        let high_weight = graph.get_high_weight_relationships(&intent1_id, 0.5);
        assert_eq!(high_weight.len(), 1);
        assert_eq!(high_weight[0].0.intent_id, intent2_id);
        assert_eq!(high_weight[0].1, 0.8);
    }

    #[test]
    fn test_hierarchical_relationships() {
        let mut graph = IntentGraph::new().unwrap();
        
        // Create a hierarchy: root -> parent -> child
        let root = StorableIntent::new("Root goal".to_string());
        let parent = StorableIntent::new("Parent goal".to_string());
        let child = StorableIntent::new("Child goal".to_string());
        
        let root_id = root.intent_id.clone();
        let parent_id = parent.intent_id.clone();
        let child_id = child.intent_id.clone();
        
        graph.store_intent(root).unwrap();
        graph.store_intent(parent).unwrap();
        graph.store_intent(child).unwrap();
        
        // Create hierarchical relationships
        graph.create_edge(parent_id.clone(), root_id.clone(), EdgeType::IsSubgoalOf).unwrap();
        graph.create_edge(child_id.clone(), parent_id.clone(), EdgeType::IsSubgoalOf).unwrap();
        
        // Debug: Check all edges
        let all_edges = graph.rt.block_on(async {
            graph.storage.get_edges().await.unwrap_or_else(|_| Vec::new())
        });
        println!("All edges: {:?}", all_edges);
        
        // Debug: Check edges for child
        let child_edges = graph.get_edges_for_intent(&child_id);
        println!("Child edges: {:?}", child_edges);
        
        // Debug: Check edges for parent
        let parent_edges = graph.get_edges_for_intent(&parent_id);
        println!("Parent edges: {:?}", parent_edges);
        
        // Test parent relationships
        let parents = graph.get_parent_intents(&child_id);
        println!("Parents of child: {:?}", parents.len());
        assert_eq!(parents.len(), 1);
        assert_eq!(parents[0].intent_id, parent_id);
        
        let parents = graph.get_parent_intents(&parent_id);
        println!("Parents of parent: {:?}", parents.len());
        assert_eq!(parents.len(), 1);
        assert_eq!(parents[0].intent_id, root_id);
        
        // Test child relationships
        let children = graph.get_child_intents(&root_id);
        println!("Children of root: {:?}", children.len());
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].intent_id, parent_id);
        
        let children = graph.get_child_intents(&parent_id);
        println!("Children of parent: {:?}", children.len());
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].intent_id, child_id);
        
        // Test full hierarchy
        let hierarchy = graph.get_intent_hierarchy(&parent_id);
        assert_eq!(hierarchy.len(), 3); // root, parent, child
        let hierarchy_ids: Vec<String> = hierarchy.iter().map(|i| i.intent_id.clone()).collect();
        assert!(hierarchy_ids.contains(&root_id));
        assert!(hierarchy_ids.contains(&parent_id));
        assert!(hierarchy_ids.contains(&child_id));
    }

    #[test]
    fn test_relationship_queries() {
        let mut graph = IntentGraph::new().unwrap();
        
        let intent1 = StorableIntent::new("Goal 1".to_string());
        let intent2 = StorableIntent::new("Goal 2".to_string());
        let intent3 = StorableIntent::new("Goal 3".to_string());
        let intent4 = StorableIntent::new("Goal 4".to_string());
        
        let intent1_id = intent1.intent_id.clone();
        let intent2_id = intent2.intent_id.clone();
        let intent3_id = intent3.intent_id.clone();
        let intent4_id = intent4.intent_id.clone();
        
        graph.store_intent(intent1).unwrap();
        graph.store_intent(intent2).unwrap();
        graph.store_intent(intent3).unwrap();
        graph.store_intent(intent4).unwrap();
        
        // Create various relationships
        graph.create_edge(intent1_id.clone(), intent2_id.clone(), EdgeType::DependsOn).unwrap();
        graph.create_edge(intent1_id.clone(), intent3_id.clone(), EdgeType::ConflictsWith).unwrap();
        graph.create_edge(intent1_id.clone(), intent4_id.clone(), EdgeType::Enables).unwrap();
        
        // Test relationship type queries
        let depends_on = graph.find_intents_by_relationship(&intent1_id, EdgeType::DependsOn);
        assert_eq!(depends_on.len(), 1);
        assert_eq!(depends_on[0].intent_id, intent2_id);
        
        let conflicts = graph.find_intents_by_relationship(&intent1_id, EdgeType::ConflictsWith);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].intent_id, intent3_id);
        
        let enables = graph.find_intents_by_relationship(&intent1_id, EdgeType::Enables);
        assert_eq!(enables.len(), 1);
        assert_eq!(enables[0].intent_id, intent4_id);
    }

    #[test]
    fn test_strongly_connected_intents() {
        let mut graph = IntentGraph::new().unwrap();
        
        let intent1 = StorableIntent::new("Goal 1".to_string());
        let intent2 = StorableIntent::new("Goal 2".to_string());
        let intent3 = StorableIntent::new("Goal 3".to_string());
        
        let intent1_id = intent1.intent_id.clone();
        let intent2_id = intent2.intent_id.clone();
        let intent3_id = intent3.intent_id.clone();
        
        graph.store_intent(intent1).unwrap();
        graph.store_intent(intent2).unwrap();
        graph.store_intent(intent3).unwrap();
        
        // Create bidirectional relationship between intent1 and intent2
        graph.create_edge(intent1_id.clone(), intent2_id.clone(), EdgeType::RelatedTo).unwrap();
        graph.create_edge(intent2_id.clone(), intent1_id.clone(), EdgeType::RelatedTo).unwrap();
        
        // Create one-way relationship to intent3
        graph.create_edge(intent1_id.clone(), intent3_id.clone(), EdgeType::DependsOn).unwrap();
        
        // Debug output
        println!("Intent1 edges: {:?}", graph.get_edges_for_intent(&intent1_id));
        println!("Intent2 edges: {:?}", graph.get_edges_for_intent(&intent2_id));
        println!("Intent3 edges: {:?}", graph.get_edges_for_intent(&intent3_id));
        
        // Test strongly connected intents
        let connected = graph.get_strongly_connected_intents(&intent1_id);
        println!("Strongly connected to intent1: {:?}", connected.len());
        assert_eq!(connected.len(), 1);
        assert_eq!(connected[0].intent_id, intent2_id);
        
        let connected = graph.get_strongly_connected_intents(&intent2_id);
        println!("Strongly connected to intent2: {:?}", connected.len());
        assert_eq!(connected.len(), 1);
        assert_eq!(connected[0].intent_id, intent1_id);
        
        // Debug intent3 specifically
        println!("Intent3 ID: {:?}", intent3_id);
        let intent3_edges = graph.get_edges_for_intent(&intent3_id);
        println!("Intent3 edges: {:?}", intent3_edges);
        
        // Check each edge for intent3
        for edge in &intent3_edges {
            let other_id = if edge.from == intent3_id { &edge.to } else { &edge.from };
            println!("Checking edge from {:?} to {:?} with type {:?}", edge.from, edge.to, edge.edge_type);
            
            let reverse_edges = graph.get_edges_for_intent(other_id);
            println!("Reverse edges for {:?}: {:?}", other_id, reverse_edges);
            
            let has_reverse = reverse_edges.iter().any(|e| {
                e.from == *other_id && 
                e.to == intent3_id && 
                e.edge_type == edge.edge_type
            });
            println!("Has reverse: {}", has_reverse);
        }
        
        let connected = graph.get_strongly_connected_intents(&intent3_id);
        println!("Strongly connected to intent3: {:?}", connected.len());
        assert_eq!(connected.len(), 0); // No bidirectional relationship
    }

    #[test]
    fn test_edge_metadata() {
        let mut graph = IntentGraph::new().unwrap();
        
        let intent1 = StorableIntent::new("Goal 1".to_string());
        let intent2 = StorableIntent::new("Goal 2".to_string());
        
        let intent1_id = intent1.intent_id.clone();
        let intent2_id = intent2.intent_id.clone();
        
        graph.store_intent(intent1).unwrap();
        graph.store_intent(intent2).unwrap();
        
        // Create edge with metadata
        let mut metadata = HashMap::new();
        metadata.insert("reason".to_string(), "resource conflict".to_string());
        metadata.insert("severity".to_string(), "high".to_string());
        
        graph.create_weighted_edge(
            intent1_id.clone(),
            intent2_id.clone(),
            EdgeType::ConflictsWith,
            0.9,
            metadata,
        ).unwrap();
        
        // Verify edge was created with metadata
        let edges = graph.get_edges_for_intent(&intent1_id);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].edge_type, EdgeType::ConflictsWith);
        assert_eq!(edges[0].weight, Some(0.9));
        assert_eq!(edges[0].metadata.get("reason"), Some(&"resource conflict".to_string()));
        assert_eq!(edges[0].metadata.get("severity"), Some(&"high".to_string()));
    }

    #[test]
    fn test_debug_edge_queries() {
        let mut graph = IntentGraph::new().unwrap();
        
        let intent1 = StorableIntent::new("Goal 1".to_string());
        let intent2 = StorableIntent::new("Goal 2".to_string());
        
        let intent1_id = intent1.intent_id.clone();
        let intent2_id = intent2.intent_id.clone();
        
        graph.store_intent(intent1).unwrap();
        graph.store_intent(intent2).unwrap();
        
        // Create edge: intent1 -> intent2 (intent1 depends on intent2)
        graph.create_edge(intent1_id.clone(), intent2_id.clone(), EdgeType::DependsOn).unwrap();
        
        // Debug output
        let all_edges = graph.get_edges_for_intent(&intent1_id);
        println!("All edges: {:?}", all_edges);
        
        // Test parent-child relationships
        let parents = graph.get_parent_intents(&intent1_id);
        println!("Intent1 parents: {:?}", parents.len());
        assert_eq!(parents.len(), 1); // intent1 has intent2 as parent
        
        let children = graph.get_child_intents(&intent2_id);
        println!("Intent2 children: {:?}", children.len());
        assert_eq!(children.len(), 1); // intent2 has intent1 as child
    }

    #[test]
    fn test_subgraph_storage_from_root() {
        let mut graph = IntentGraph::new().unwrap();
        
        // Create a hierarchical structure: root -> parent -> child
        let root = StorableIntent::new("Root goal".to_string());
        let parent = StorableIntent::new("Parent goal".to_string());
        let child = StorableIntent::new("Child goal".to_string());
        
        let root_id = root.intent_id.clone();
        let parent_id = parent.intent_id.clone();
        let child_id = child.intent_id.clone();
        
        graph.store_intent(root).unwrap();
        graph.store_intent(parent).unwrap();
        graph.store_intent(child).unwrap();
        
        // Create hierarchical relationships
        graph.create_edge(parent_id.clone(), root_id.clone(), EdgeType::IsSubgoalOf).unwrap();
        graph.create_edge(child_id.clone(), parent_id.clone(), EdgeType::IsSubgoalOf).unwrap();
        
        // Store subgraph from root
        let temp_dir = tempfile::tempdir().unwrap();
        let subgraph_path = temp_dir.path().join("subgraph_from_root.json");
        
        graph.store_subgraph_from_root(&root_id, &subgraph_path).unwrap();
        assert!(subgraph_path.exists());
        
        // Create new graph and restore subgraph
        let mut new_graph = IntentGraph::new().unwrap();
        new_graph.restore_subgraph(&subgraph_path).unwrap();
        
        // Verify all intents are restored
        assert!(new_graph.get_intent(&root_id).is_some());
        assert!(new_graph.get_intent(&parent_id).is_some());
        assert!(new_graph.get_intent(&child_id).is_some());
        
        // Verify relationships are restored
        let root_parents = new_graph.get_parent_intents(&root_id);
        assert_eq!(root_parents.len(), 0); // Root has no parents
        
        let parent_parents = new_graph.get_parent_intents(&parent_id);
        assert_eq!(parent_parents.len(), 1); // Parent has root as parent
        
        let child_parents = new_graph.get_parent_intents(&child_id);
        assert_eq!(child_parents.len(), 1); // Child has parent as parent
    }

    #[test]
    fn test_subgraph_storage_from_child() {
        let mut graph = IntentGraph::new().unwrap();
        
        // Create a hierarchical structure: root -> parent -> child
        let root = StorableIntent::new("Root goal".to_string());
        let parent = StorableIntent::new("Parent goal".to_string());
        let child = StorableIntent::new("Child goal".to_string());
        
        let root_id = root.intent_id.clone();
        let parent_id = parent.intent_id.clone();
        let child_id = child.intent_id.clone();
        
        graph.store_intent(root).unwrap();
        graph.store_intent(parent).unwrap();
        graph.store_intent(child).unwrap();
        
        // Create hierarchical relationships
        graph.create_edge(parent_id.clone(), root_id.clone(), EdgeType::IsSubgoalOf).unwrap();
        graph.create_edge(child_id.clone(), parent_id.clone(), EdgeType::IsSubgoalOf).unwrap();
        
        // Store subgraph from child (should include child and all ancestors)
        let temp_dir = tempfile::tempdir().unwrap();
        let subgraph_path = temp_dir.path().join("subgraph_from_child.json");
        
        graph.store_subgraph_from_child(&child_id, &subgraph_path).unwrap();
        assert!(subgraph_path.exists());
        
        // Create new graph and restore subgraph
        let mut new_graph = IntentGraph::new().unwrap();
        new_graph.restore_subgraph(&subgraph_path).unwrap();
        
        // Verify all intents are restored
        assert!(new_graph.get_intent(&root_id).is_some());
        assert!(new_graph.get_intent(&parent_id).is_some());
        assert!(new_graph.get_intent(&child_id).is_some());
        
        // Verify relationships are restored
        let child_parents = new_graph.get_parent_intents(&child_id);
        assert_eq!(child_parents.len(), 1); // Child has parent as parent
        
        let parent_parents = new_graph.get_parent_intents(&parent_id);
        assert_eq!(parent_parents.len(), 1); // Parent has root as parent
    }

    #[test]
    fn test_complex_subgraph_with_multiple_relationships() {
        let mut graph = IntentGraph::new().unwrap();
        
        // Create a complex graph structure
        let root = StorableIntent::new("Root goal".to_string());
        let parent1 = StorableIntent::new("Parent 1".to_string());
        let parent2 = StorableIntent::new("Parent 2".to_string());
        let child1 = StorableIntent::new("Child 1".to_string());
        let child2 = StorableIntent::new("Child 2".to_string());
        let grandchild = StorableIntent::new("Grandchild".to_string());
        
        let root_id = root.intent_id.clone();
        let parent1_id = parent1.intent_id.clone();
        let parent2_id = parent2.intent_id.clone();
        let child1_id = child1.intent_id.clone();
        let child2_id = child2.intent_id.clone();
        let grandchild_id = grandchild.intent_id.clone();
        
        // Store all intents
        graph.store_intent(root).unwrap();
        graph.store_intent(parent1).unwrap();
        graph.store_intent(parent2).unwrap();
        graph.store_intent(child1).unwrap();
        graph.store_intent(child2).unwrap();
        graph.store_intent(grandchild).unwrap();
        
        // Create complex relationships
        graph.create_edge(parent1_id.clone(), root_id.clone(), EdgeType::IsSubgoalOf).unwrap();
        graph.create_edge(parent2_id.clone(), root_id.clone(), EdgeType::IsSubgoalOf).unwrap();
        graph.create_edge(child1_id.clone(), parent1_id.clone(), EdgeType::IsSubgoalOf).unwrap();
        graph.create_edge(child2_id.clone(), parent1_id.clone(), EdgeType::IsSubgoalOf).unwrap();
        graph.create_edge(grandchild_id.clone(), child1_id.clone(), EdgeType::IsSubgoalOf).unwrap();
        
        // Add some related intents (not in hierarchy)
        graph.create_edge(child1_id.clone(), child2_id.clone(), EdgeType::RelatedTo).unwrap();
        
        // Store subgraph from root
        let temp_dir = tempfile::tempdir().unwrap();
        let subgraph_path = temp_dir.path().join("complex_subgraph.json");
        
        graph.store_subgraph_from_root(&root_id, &subgraph_path).unwrap();
        
        // Create new graph and restore
        let mut new_graph = IntentGraph::new().unwrap();
        new_graph.restore_subgraph(&subgraph_path).unwrap();
        
        // Get the actual intent IDs from the restored graph by matching the goals
        let restored_root = new_graph.find_relevant_intents("Root goal").into_iter().next().unwrap();
        let restored_parent1 = new_graph.find_relevant_intents("Parent 1").into_iter().next().unwrap();
        let restored_parent2 = new_graph.find_relevant_intents("Parent 2").into_iter().next().unwrap();
        let restored_child1 = new_graph.find_relevant_intents("Child 1").into_iter().next().unwrap();
        let restored_child2 = new_graph.find_relevant_intents("Child 2").into_iter().next().unwrap();
        let restored_grandchild = new_graph.find_relevant_intents("Grandchild").into_iter().next().unwrap();
        
        // Verify all intents are restored
        assert!(new_graph.get_intent(&restored_root.intent_id).is_some());
        assert!(new_graph.get_intent(&restored_parent1.intent_id).is_some());
        assert!(new_graph.get_intent(&restored_parent2.intent_id).is_some());
        assert!(new_graph.get_intent(&restored_child1.intent_id).is_some());
        assert!(new_graph.get_intent(&restored_child2.intent_id).is_some());
        assert!(new_graph.get_intent(&restored_grandchild.intent_id).is_some());
        
        // Verify hierarchy is preserved
        let root_children = new_graph.get_child_intents(&restored_root.intent_id);
        assert_eq!(root_children.len(), 2); // root has 2 children (parent1, parent2)
        
        let parent1_children = new_graph.get_child_intents(&restored_parent1.intent_id);
        assert_eq!(parent1_children.len(), 2); // parent1 has 2 children (child1, child2)
        
        let child1_children = new_graph.get_child_intents(&restored_child1.intent_id);
        assert_eq!(child1_children.len(), 1); // child1 has 1 child (grandchild)
        
        // Verify related intents are preserved
        let related_to_child1 = new_graph.find_intents_by_relationship(&restored_child1.intent_id, EdgeType::RelatedTo);
        assert_eq!(related_to_child1.len(), 1); // child1 is related to child2
    }
}
