use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::ccos::types::{
    EdgeType, IntentId, IntentStatus, StorableIntent, ExecutionResult,
};
use crate::ccos::intent_storage::IntentFilter;
use crate::runtime::RuntimeError;
use super::{
    config::IntentGraphConfig,
    storage::{IntentGraphStorage, Edge},
    virtualization::{IntentGraphVirtualization, VirtualizationConfig, VirtualizedIntentGraph, VirtualizedSearchResult, VirtualizationStats},
    processing::{IntentLifecycleManager},
};

/// Main Intent Graph implementation with persistent storage
pub struct IntentGraph {
    pub storage: IntentGraphStorage,
    pub virtualization: IntentGraphVirtualization,
    pub lifecycle: IntentLifecycleManager,
    pub rt: tokio::runtime::Handle,
}

impl std::fmt::Debug for IntentGraph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IntentGraph")
            .field("storage", &self.storage)
            .field("virtualization", &self.virtualization)
            .field("lifecycle", &self.lifecycle)
            .field("rt", &"tokio::runtime::Handle")
            .finish()
    }
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

        // Create storage synchronously using the runtime.
        // If we are already inside a Tokio runtime, avoid blocking the worker thread directly.
        let storage = if tokio::runtime::Handle::try_current().is_ok() {
            // Use a lightweight futures executor which is safe even on current-thread runtimes
            futures::executor::block_on(async { IntentGraphStorage::new(config).await })
        } else {
            rt.block_on(async { IntentGraphStorage::new(config).await })
        };

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
        // If we're already inside a Tokio runtime, avoid block_in_place which requires multi-thread flavor.
        // Instead, use a lightweight futures executor to drive the future to completion.
        if tokio::runtime::Handle::try_current().is_ok() {
            futures::executor::block_on(async {
                self.storage.store_intent(intent).await?;
                self.lifecycle.infer_edges(&mut self.storage).await?;
                Ok(())
            })
        } else {
            self.rt.block_on(async {
                self.storage.store_intent(intent).await?;
                self.lifecycle.infer_edges(&mut self.storage).await?;
                Ok(())
            })
        }
    }

    /// Get an intent by ID
    pub fn get_intent(&self, intent_id: &IntentId) -> Option<StorableIntent> {
        if tokio::runtime::Handle::try_current().is_ok() {
            futures::executor::block_on(async { self.storage.get_intent(intent_id).await.unwrap_or(None) })
        } else {
            self.rt.block_on(async { self.storage.get_intent(intent_id).await.unwrap_or(None) })
        }
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

        if tokio::runtime::Handle::try_current().is_ok() {
            futures::executor::block_on(async { self.storage.update_intent(&intent).await })
        } else {
            self.rt.block_on(async { self.storage.update_intent(&intent).await })
        }
    }

    /// Find relevant intents for a query
    pub fn find_relevant_intents(&self, query: &str) -> Vec<StorableIntent> {
        if tokio::runtime::Handle::try_current().is_ok() {
            futures::executor::block_on(async {
                let filter = IntentFilter {
                    goal_contains: Some(query.to_string()),
                    ..Default::default()
                };
                self.storage.list_intents(filter).await.unwrap_or_default()
            })
        } else {
            self.rt.block_on(async {
                let filter = IntentFilter {
                    goal_contains: Some(query.to_string()),
                    ..Default::default()
                };
                self.storage.list_intents(filter).await.unwrap_or_default()
            })
        }
    }

    /// Load context window for a set of intent IDs
    pub fn load_context_window(&self, intent_ids: &[IntentId]) -> Vec<StorableIntent> {
        if tokio::runtime::Handle::try_current().is_ok() {
            futures::executor::block_on(async {
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
        } else {
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
    }

    /// Create a virtualized view of the intent graph
    pub async fn create_virtualized_view(
        &self,
        focal_intents: &[IntentId],
        config: &VirtualizationConfig,
    ) -> Result<VirtualizedIntentGraph, RuntimeError> {
        self.virtualization.create_virtualized_view(focal_intents, &self.storage, config).await
    }

    /// Load context window with virtualization optimizations
    pub async fn load_virtualized_context_window(
        &self,
        intent_ids: &[IntentId],
        config: &VirtualizationConfig,
    ) -> Result<Vec<StorableIntent>, RuntimeError> {
        self.virtualization.load_context_window(intent_ids, &self.storage, config).await
    }

    /// Perform semantic search with virtualization
    pub async fn search_with_virtualization(
        &self,
        query: &str,
        config: &VirtualizationConfig,
    ) -> Result<VirtualizedSearchResult, RuntimeError> {
        self.virtualization.search_with_virtualization(query, &self.storage, config).await
    }

    /// Enhanced semantic search using the virtualization layer
    pub fn enhanced_search(
        &self,
        query: &str,
        limit: Option<usize>,
    ) -> Result<Vec<StorableIntent>, RuntimeError> {
        let intent_ids = self.virtualization.search_intents(query, &self.storage, limit.unwrap_or(50))?;
        let mut results = Vec::new();
        
        for intent_id in intent_ids {
            if let Some(intent) = self.storage.get_intent_sync(&intent_id) {
                results.push(intent);
            }
        }
        
        Ok(results)
    }

    /// Find similar intents to a given intent
    pub fn find_similar_intents(
        &self,
        target_intent: &StorableIntent,
        limit: usize,
    ) -> Result<Vec<StorableIntent>, RuntimeError> {
        let similar_ids = self.virtualization.find_similar_intents(
            target_intent, 
            &self.storage, 
            limit
        )?;
        
        let mut results = Vec::new();
        for intent_id in similar_ids {
            if let Some(intent) = self.storage.get_intent_sync(&intent_id) {
                results.push(intent);
            }
        }
        
        Ok(results)
    }

    /// Get graph statistics for monitoring and optimization
    pub fn get_virtualization_stats(&self) -> Result<VirtualizationStats, RuntimeError> {
        let all_intents = self.storage.get_all_intents_sync();
        let total_intents = all_intents.len();
        
        let mut status_distribution = HashMap::new();
        let mut connectivity_scores = Vec::new();
        
        for intent in &all_intents {
            *status_distribution.entry(intent.status.clone()).or_insert(0) += 1;
            
            let connections = self.storage.get_connected_intents_sync(&intent.intent_id).len();
            connectivity_scores.push(connections);
        }
        
        let avg_connectivity = if !connectivity_scores.is_empty() {
            connectivity_scores.iter().sum::<usize>() as f64 / connectivity_scores.len() as f64
        } else {
            0.0
        };
        
        let isolated_intents = connectivity_scores.iter().filter(|&&x| x == 0).count();
        let highly_connected = connectivity_scores.iter().filter(|&&x| x > 5).count();
        
        Ok(VirtualizationStats {
            total_intents,
            status_distribution,
            avg_connectivity,
            isolated_intents,
            highly_connected_intents: highly_connected,
            memory_usage_estimate: total_intents * 1024, // Rough estimate
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
            .with_metadata(metadata);
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

    /// Archive completed intents
    pub fn archive_completed_intents(&mut self) -> Result<(), RuntimeError> {
        self.rt.block_on(async {
            self.lifecycle.archive_completed_intents(&mut self.storage).await
        })
    }

    /// Complete an intent with execution result
    pub fn complete_intent(&mut self, intent_id: &IntentId, result: &ExecutionResult) -> Result<(), RuntimeError> {
        self.rt.block_on(async {
            self.lifecycle.complete_intent(&mut self.storage, intent_id, result).await
        })
    }

    /// Fail an intent with error message
    pub fn fail_intent(&mut self, intent_id: &IntentId, error_message: String) -> Result<(), RuntimeError> {
        self.rt.block_on(async {
            self.lifecycle.fail_intent(&mut self.storage, intent_id, error_message).await
        })
    }

    /// Suspend an intent with reason
    pub fn suspend_intent(&mut self, intent_id: &IntentId, reason: String) -> Result<(), RuntimeError> {
        self.rt.block_on(async {
            self.lifecycle.suspend_intent(&mut self.storage, intent_id, reason).await
        })
    }

    /// Resume a suspended intent
    pub fn resume_intent(&mut self, intent_id: &IntentId, reason: String) -> Result<(), RuntimeError> {
        self.rt.block_on(async {
            self.lifecycle.resume_intent(&mut self.storage, intent_id, reason).await
        })
    }

    /// Archive an intent with reason
    pub fn archive_intent(&mut self, intent_id: &IntentId, reason: String) -> Result<(), RuntimeError> {
        self.rt.block_on(async {
            self.lifecycle.archive_intent(&mut self.storage, intent_id, reason).await
        })
    }

    /// Reactivate an archived intent
    pub fn reactivate_intent(&mut self, intent_id: &IntentId, reason: String) -> Result<(), RuntimeError> {
        self.rt.block_on(async {
            self.lifecycle.reactivate_intent(&mut self.storage, intent_id, reason).await
        })
    }

    /// Get intents by status
    pub fn get_intents_by_status(&self, status: IntentStatus) -> Vec<StorableIntent> {
        self.rt.block_on(async {
            self.lifecycle.get_intents_by_status(&self.storage, status).await.unwrap_or_default()
        })
    }

    /// Get intent status transition history
    pub fn get_status_history(&self, intent_id: &IntentId) -> Vec<String> {
        self.rt.block_on(async {
            self.lifecycle.get_status_history(&self.storage, intent_id).await.unwrap_or_default()
        })
    }

    /// Get intents that are ready for processing (Active status)
    pub fn get_ready_intents(&self) -> Vec<StorableIntent> {
        self.rt.block_on(async {
            self.lifecycle.get_ready_intents(&self.storage).await.unwrap_or_default()
        })
    }

    /// Get intents that need attention (Failed or Suspended status)
    pub fn get_intents_needing_attention(&self) -> Vec<StorableIntent> {
        self.rt.block_on(async {
            self.lifecycle.get_intents_needing_attention(&self.storage).await.unwrap_or_default()
        })
    }

    /// Get intents that can be archived (Completed for more than specified days)
    pub fn get_intents_ready_for_archival(&self, days_threshold: u64) -> Vec<StorableIntent> {
        self.rt.block_on(async {
            self.lifecycle.get_intents_ready_for_archival(&self.storage, days_threshold).await.unwrap_or_default()
        })
    }

    /// Bulk transition intents by status
    pub fn bulk_transition_intents(
        &mut self,
        intent_ids: &[IntentId],
        new_status: IntentStatus,
        reason: String,
    ) -> Result<Vec<IntentId>, RuntimeError> {
        self.rt.block_on(async {
            self.lifecycle.bulk_transition_intents(&mut self.storage, intent_ids, new_status, reason).await
        })
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
