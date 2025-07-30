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
#[derive(Debug)]
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
#[derive(Debug)]
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
#[derive(Debug)]
pub struct SemanticSearchEngine;

impl SemanticSearchEngine {
    pub fn new() -> Self {
        Self
    }
}

/// Graph traversal engine (placeholder for now)
#[derive(Debug)]
pub struct GraphTraversalEngine;

impl GraphTraversalEngine {
    pub fn new() -> Self {
        Self
    }
}

/// Lifecycle management for intents
#[derive(Debug)]
pub struct IntentLifecycleManager;

impl IntentLifecycleManager {
    /// Archive completed intents (existing functionality)
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
            self.transition_intent_status(
                storage,
                None, // causal_chain - will be added when IntentGraph has access
                &mut intent,
                IntentStatus::Archived,
                "Auto-archived completed intent".to_string(),
                None, // triggering_plan_id - will be enhanced later
            ).await?;
        }

        Ok(())
    }

    /// Transition an intent to a new status with audit trail
    pub async fn transition_intent_status(
        &self,
        storage: &mut IntentGraphStorage,
        causal_chain: Option<&mut crate::ccos::causal_chain::CausalChain>,
        intent: &mut StorableIntent,
        new_status: IntentStatus,
        reason: String,
        triggering_plan_id: Option<&str>,
    ) -> Result<(), RuntimeError> {
        let old_status = intent.status.clone();
        
        // Validate the transition
        self.validate_status_transition(&old_status, &new_status)?;
        
        // Update the intent
        intent.status = new_status.clone();
        intent.updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        // Add audit trail to metadata
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        // Count existing transitions to ensure unique keys
        let transition_count = intent.metadata
            .keys()
            .filter(|key| key.starts_with("status_transition_"))
            .count();
        
        let audit_entry = format!(
            "{}: {} -> {} (reason: {})",
            timestamp,
            self.status_to_string(&old_status),
            self.status_to_string(&new_status),
            reason
        );
        
        let audit_key = format!("status_transition_{}_{}", timestamp, transition_count);
        intent.metadata.insert(audit_key, audit_entry);
        
        // Store the updated intent
        storage.update_intent(intent).await?;
        
        // Log to Causal Chain if available
        if let Some(chain) = causal_chain {
            let plan_id = triggering_plan_id.unwrap_or("intent-lifecycle-manager");
            chain.log_intent_status_change(
                &plan_id.to_string(),
                &intent.intent_id,
                self.status_to_string(&old_status),
                self.status_to_string(&new_status),
                &reason,
                None, // triggering_action_id - could be enhanced later
            )?;
        }
        
        Ok(())
    }

    /// Complete an intent (transition to Completed status)
    pub async fn complete_intent(
        &self,
        storage: &mut IntentGraphStorage,
        intent_id: &IntentId,
        result: &ExecutionResult,
    ) -> Result<(), RuntimeError> {
        let mut intent = storage.get_intent(intent_id).await?
            .ok_or_else(|| RuntimeError::StorageError(format!("Intent {} not found", intent_id)))?;
        
        let reason = if result.success {
            "Intent completed successfully".to_string()
        } else {
            format!("Intent completed with errors: {:?}", result.value)
        };
        
        self.transition_intent_status(
            storage,
            None, // causal_chain - will be added when IntentGraph has access
            &mut intent,
            IntentStatus::Completed,
            reason,
            None, // triggering_plan_id - will be enhanced later
        ).await?;
        
        Ok(())
    }

    /// Fail an intent (transition to Failed status)
    pub async fn fail_intent(
        &self,
        storage: &mut IntentGraphStorage,
        intent_id: &IntentId,
        error_message: String,
    ) -> Result<(), RuntimeError> {
        let mut intent = storage.get_intent(intent_id).await?
            .ok_or_else(|| RuntimeError::StorageError(format!("Intent {} not found", intent_id)))?;
        
        self.transition_intent_status(
            storage,
            None, // causal_chain - will be added when IntentGraph has access
            &mut intent,
            IntentStatus::Failed,
            format!("Intent failed: {}", error_message),
            None, // triggering_plan_id - will be enhanced later
        ).await?;
        
        Ok(())
    }

    /// Suspend an intent (transition to Suspended status)
    pub async fn suspend_intent(
        &self,
        storage: &mut IntentGraphStorage,
        intent_id: &IntentId,
        reason: String,
    ) -> Result<(), RuntimeError> {
        let mut intent = storage.get_intent(intent_id).await?
            .ok_or_else(|| RuntimeError::StorageError(format!("Intent {} not found", intent_id)))?;
        
        self.transition_intent_status(
            storage,
            None, // causal_chain - will be added when IntentGraph has access
            &mut intent,
            IntentStatus::Suspended,
            format!("Intent suspended: {}", reason),
            None, // triggering_plan_id - will be enhanced later
        ).await?;
        
        Ok(())
    }

    /// Resume a suspended intent (transition to Active status)
    pub async fn resume_intent(
        &self,
        storage: &mut IntentGraphStorage,
        intent_id: &IntentId,
        reason: String,
    ) -> Result<(), RuntimeError> {
        let mut intent = storage.get_intent(intent_id).await?
            .ok_or_else(|| RuntimeError::StorageError(format!("Intent {} not found", intent_id)))?;
        
        self.transition_intent_status(
            storage,
            None, // causal_chain - will be added when IntentGraph has access
            &mut intent,
            IntentStatus::Active,
            format!("Intent resumed: {}", reason),
            None, // triggering_plan_id - will be enhanced later
        ).await?;
        
        Ok(())
    }

    /// Archive an intent (transition to Archived status)
    pub async fn archive_intent(
        &self,
        storage: &mut IntentGraphStorage,
        intent_id: &IntentId,
        reason: String,
    ) -> Result<(), RuntimeError> {
        let mut intent = storage.get_intent(intent_id).await?
            .ok_or_else(|| RuntimeError::StorageError(format!("Intent {} not found", intent_id)))?;
        
        self.transition_intent_status(
            storage,
            None, // causal_chain - will be added when IntentGraph has access
            &mut intent,
            IntentStatus::Archived,
            format!("Intent archived: {}", reason),
            None, // triggering_plan_id - will be enhanced later
        ).await?;
        
        Ok(())
    }

    /// Reactivate an archived intent (transition to Active status)
    pub async fn reactivate_intent(
        &self,
        storage: &mut IntentGraphStorage,
        intent_id: &IntentId,
        reason: String,
    ) -> Result<(), RuntimeError> {
        let mut intent = storage.get_intent(intent_id).await?
            .ok_or_else(|| RuntimeError::StorageError(format!("Intent {} not found", intent_id)))?;
        
        self.transition_intent_status(
            storage,
            None, // causal_chain - will be added when IntentGraph has access
            &mut intent,
            IntentStatus::Active,
            format!("Intent reactivated: {}", reason),
            None, // triggering_plan_id - will be enhanced later
        ).await?;
        
        Ok(())
    }

    /// Get intents by status
    pub async fn get_intents_by_status(
        &self,
        storage: &IntentGraphStorage,
        status: IntentStatus,
    ) -> Result<Vec<StorableIntent>, RuntimeError> {
        let filter = IntentFilter {
            status: Some(status),
            ..Default::default()
        };
        
        storage.list_intents(filter).await
    }

    /// Get intent status transition history
    pub async fn get_status_history(
        &self,
        storage: &IntentGraphStorage,
        intent_id: &IntentId,
    ) -> Result<Vec<String>, RuntimeError> {
        let intent = storage.get_intent(intent_id).await?
            .ok_or_else(|| RuntimeError::StorageError(format!("Intent {} not found", intent_id)))?;
        
        let mut history = Vec::new();
        
        // Extract status transition entries from metadata
        for (key, value) in &intent.metadata {
            if key.starts_with("status_transition_") {
                history.push(value.clone());
            }
        }
        
        // Sort by timestamp (extracted from key)
        history.sort_by(|a, b| {
            let timestamp_a = a.split(':').next().unwrap_or("0").parse::<u64>().unwrap_or(0);
            let timestamp_b = b.split(':').next().unwrap_or("0").parse::<u64>().unwrap_or(0);
            timestamp_a.cmp(&timestamp_b)
        });
        
        Ok(history)
    }

    /// Validate if a status transition is allowed
    fn validate_status_transition(
        &self,
        from: &IntentStatus,
        to: &IntentStatus,
    ) -> Result<(), RuntimeError> {
        match (from, to) {
            // Active can transition to any other status
            (IntentStatus::Active, _) => Ok(()),
            
            // Completed can only transition to Archived
            (IntentStatus::Completed, IntentStatus::Archived) => Ok(()),
            (IntentStatus::Completed, _) => Err(RuntimeError::Generic(
                format!("Cannot transition from Completed to {:?}", to)
            )),
            
            // Failed can transition to Active (retry) or Archived
            (IntentStatus::Failed, IntentStatus::Active) => Ok(()),
            (IntentStatus::Failed, IntentStatus::Archived) => Ok(()),
            (IntentStatus::Failed, _) => Err(RuntimeError::Generic(
                format!("Cannot transition from Failed to {:?}", to)
            )),
            
            // Suspended can transition to Active (resume) or Archived
            (IntentStatus::Suspended, IntentStatus::Active) => Ok(()),
            (IntentStatus::Suspended, IntentStatus::Archived) => Ok(()),
            (IntentStatus::Suspended, _) => Err(RuntimeError::Generic(
                format!("Cannot transition from Suspended to {:?}", to)
            )),
            
            // Archived can transition to Active (reactivate)
            (IntentStatus::Archived, IntentStatus::Active) => Ok(()),
            (IntentStatus::Archived, _) => Err(RuntimeError::Generic(
                format!("Cannot transition from Archived to {:?}", to)
            )),
        }
    }

    /// Convert status to string for audit trail
    fn status_to_string(&self, status: &IntentStatus) -> &'static str {
        match status {
            IntentStatus::Active => "Active",
            IntentStatus::Completed => "Completed",
            IntentStatus::Failed => "Failed",
            IntentStatus::Archived => "Archived",
            IntentStatus::Suspended => "Suspended",
        }
    }

    /// Get intents that are ready for processing (Active status)
    pub async fn get_ready_intents(
        &self,
        storage: &IntentGraphStorage,
    ) -> Result<Vec<StorableIntent>, RuntimeError> {
        self.get_intents_by_status(storage, IntentStatus::Active).await
    }

    /// Get intents that need attention (Failed or Suspended status)
    pub async fn get_intents_needing_attention(
        &self,
        storage: &IntentGraphStorage,
    ) -> Result<Vec<StorableIntent>, RuntimeError> {
        let failed = self.get_intents_by_status(storage, IntentStatus::Failed).await?;
        let suspended = self.get_intents_by_status(storage, IntentStatus::Suspended).await?;
        
        let mut needing_attention = failed;
        needing_attention.extend(suspended);
        
        Ok(needing_attention)
    }

    /// Get intents that can be archived (Completed for more than specified days)
    pub async fn get_intents_ready_for_archival(
        &self,
        storage: &IntentGraphStorage,
        days_threshold: u64,
    ) -> Result<Vec<StorableIntent>, RuntimeError> {
        let completed_intents = self.get_intents_by_status(storage, IntentStatus::Completed).await?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let threshold_seconds = days_threshold * 24 * 60 * 60;
        
        let ready_for_archival = completed_intents
            .into_iter()
            .filter(|intent| {
                let time_since_completion = now.saturating_sub(intent.updated_at);
                time_since_completion >= threshold_seconds
            })
            .collect();
        
        Ok(ready_for_archival)
    }

    /// Bulk transition intents by status
    pub async fn bulk_transition_intents(
        &self,
        storage: &mut IntentGraphStorage,
        intent_ids: &[IntentId],
        new_status: IntentStatus,
        reason: String,
    ) -> Result<Vec<IntentId>, RuntimeError> {
        let mut successful_transitions = Vec::new();
        let mut errors = Vec::new();
        
        for intent_id in intent_ids {
            match self.transition_intent_by_id(storage, intent_id, new_status.clone(), reason.clone()).await {
                Ok(()) => successful_transitions.push(intent_id.clone()),
                Err(e) => errors.push((intent_id.clone(), e)),
            }
        }
        
        if !errors.is_empty() {
            let error_summary = errors
                .iter()
                .map(|(id, e)| format!("{}: {}", id, e))
                .collect::<Vec<_>>()
                .join(", ");
            
            return Err(RuntimeError::Generic(
                format!("Some transitions failed: {}", error_summary)
            ));
        }
        
        Ok(successful_transitions)
    }

    /// Helper method to transition intent by ID
    async fn transition_intent_by_id(
        &self,
        storage: &mut IntentGraphStorage,
        intent_id: &IntentId,
        new_status: IntentStatus,
        reason: String,
    ) -> Result<(), RuntimeError> {
        let mut intent = storage.get_intent(intent_id).await?
            .ok_or_else(|| RuntimeError::StorageError(format!("Intent {} not found", intent_id)))?;
        
        self.transition_intent_status(
            storage,
            None, // causal_chain - will be added when IntentGraph has access
            &mut intent,
            new_status,
            reason,
            None, // triggering_plan_id - will be enhanced later
        ).await
    }

    /// Infer edges between intents (existing functionality)
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

    #[test]
    fn test_intent_lifecycle_management() {
        let mut graph = IntentGraph::new().unwrap();
        
        // Create test intent
        let intent = StorableIntent::new("Test lifecycle intent".to_string());
        let intent_id = intent.intent_id.clone();
        
        graph.store_intent(intent).unwrap();
        
        // Initially should be Active
        let retrieved = graph.get_intent(&intent_id).unwrap();
        assert_eq!(retrieved.status, IntentStatus::Active);
        
        // Test suspend
        graph.suspend_intent(&intent_id, "Waiting for resources".to_string()).unwrap();
        let suspended = graph.get_intent(&intent_id).unwrap();
        assert_eq!(suspended.status, IntentStatus::Suspended);
        
        // Test resume
        graph.resume_intent(&intent_id, "Resources available".to_string()).unwrap();
        let resumed = graph.get_intent(&intent_id).unwrap();
        assert_eq!(resumed.status, IntentStatus::Active);
        
        // Test fail
        graph.fail_intent(&intent_id, "Network timeout".to_string()).unwrap();
        let failed = graph.get_intent(&intent_id).unwrap();
        assert_eq!(failed.status, IntentStatus::Failed);
        
        // Test retry (failed -> active)
        graph.resume_intent(&intent_id, "Retrying after failure".to_string()).unwrap();
        let retried = graph.get_intent(&intent_id).unwrap();
        assert_eq!(retried.status, IntentStatus::Active);
        
        // Test complete
        let result = ExecutionResult {
            success: true,
            value: Value::String("Success".to_string()),
            metadata: HashMap::new(),
        };
        graph.complete_intent(&intent_id, &result).unwrap();
        let completed = graph.get_intent(&intent_id).unwrap();
        assert_eq!(completed.status, IntentStatus::Completed);
        
        // Test archive
        graph.archive_intent(&intent_id, "No longer needed".to_string()).unwrap();
        let archived = graph.get_intent(&intent_id).unwrap();
        assert_eq!(archived.status, IntentStatus::Archived);
        
        // Test reactivate
        graph.reactivate_intent(&intent_id, "Need to work on this again".to_string()).unwrap();
        let reactivated = graph.get_intent(&intent_id).unwrap();
        assert_eq!(reactivated.status, IntentStatus::Active);
    }

    #[test]
    fn test_status_transition_validation() {
        let mut graph = IntentGraph::new().unwrap();
        
        // Create test intent
        let intent = StorableIntent::new("Test validation intent".to_string());
        let intent_id = intent.intent_id.clone();
        
        graph.store_intent(intent).unwrap();
        
        // Test invalid transitions
        let execution_result = ExecutionResult {
            success: true,
            value: Value::String("Success".to_string()),
            metadata: HashMap::new(),
        };
        graph.complete_intent(&intent_id, &execution_result).unwrap();
        
        // Completed -> Active should fail
        let result = graph.resume_intent(&intent_id, "Invalid transition".to_string());
        assert!(result.is_err());
        
        // Completed -> Failed should fail
        let result = graph.fail_intent(&intent_id, "Invalid transition".to_string());
        assert!(result.is_err());
        
        // Completed -> Suspended should fail
        let result = graph.suspend_intent(&intent_id, "Invalid transition".to_string());
        assert!(result.is_err());
        
        // Completed -> Completed should fail (same status)
        let completion_result = graph.complete_intent(&intent_id, &execution_result);
        assert!(completion_result.is_err());
        
        // Only Completed -> Archived should work
        let result = graph.archive_intent(&intent_id, "Valid transition".to_string());
        assert!(result.is_ok());
    }

    #[test]
    fn test_status_history_audit_trail() {
        let mut graph = IntentGraph::new().unwrap();
        
        // Create test intent
        let intent = StorableIntent::new("Test audit intent".to_string());
        let intent_id = intent.intent_id.clone();
        
        graph.store_intent(intent).unwrap();
        
        // Perform several status transitions
        graph.suspend_intent(&intent_id, "Waiting for approval".to_string()).unwrap();
        graph.resume_intent(&intent_id, "Approved".to_string()).unwrap();
        graph.fail_intent(&intent_id, "Database error".to_string()).unwrap();
        graph.resume_intent(&intent_id, "Retrying".to_string()).unwrap();
        
        let result = ExecutionResult {
            success: true,
            value: Value::String("Success".to_string()),
            metadata: HashMap::new(),
        };
        graph.complete_intent(&intent_id, &result).unwrap();
        graph.archive_intent(&intent_id, "Project completed".to_string()).unwrap();
        
        // Get status history
        let history = graph.get_status_history(&intent_id);
        assert_eq!(history.len(), 6); // 6 transitions: Active->Suspended->Active->Failed->Active->Completed->Archived
        
        // Verify history entries contain expected information
        assert!(history.iter().any(|entry| entry.contains("Active -> Suspended")));
        assert!(history.iter().any(|entry| entry.contains("Suspended -> Active")));
        assert!(history.iter().any(|entry| entry.contains("Active -> Failed")));
        assert!(history.iter().any(|entry| entry.contains("Failed -> Active")));
        assert!(history.iter().any(|entry| entry.contains("Active -> Completed")));
        assert!(history.iter().any(|entry| entry.contains("Completed -> Archived")));
        
        // Verify reasons are included
        assert!(history.iter().any(|entry| entry.contains("Waiting for approval")));
        assert!(history.iter().any(|entry| entry.contains("Database error")));
        assert!(history.iter().any(|entry| entry.contains("Project completed")));
    }

    #[test]
    fn test_get_intents_by_status() {
        let mut graph = IntentGraph::new().unwrap();
        
        // Create intents with different statuses
        let mut active_intent = StorableIntent::new("Active intent".to_string());
        active_intent.status = IntentStatus::Active;
        
        let mut completed_intent = StorableIntent::new("Completed intent".to_string());
        completed_intent.status = IntentStatus::Completed;
        
        let mut failed_intent = StorableIntent::new("Failed intent".to_string());
        failed_intent.status = IntentStatus::Failed;
        
        let mut suspended_intent = StorableIntent::new("Suspended intent".to_string());
        suspended_intent.status = IntentStatus::Suspended;
        
        let mut archived_intent = StorableIntent::new("Archived intent".to_string());
        archived_intent.status = IntentStatus::Archived;
        
        graph.store_intent(active_intent).unwrap();
        graph.store_intent(completed_intent).unwrap();
        graph.store_intent(failed_intent).unwrap();
        graph.store_intent(suspended_intent).unwrap();
        graph.store_intent(archived_intent).unwrap();
        
        // Test getting intents by status
        let active_intents = graph.get_intents_by_status(IntentStatus::Active);
        assert_eq!(active_intents.len(), 1);
        assert_eq!(active_intents[0].goal, "Active intent");
        
        let completed_intents = graph.get_intents_by_status(IntentStatus::Completed);
        assert_eq!(completed_intents.len(), 1);
        assert_eq!(completed_intents[0].goal, "Completed intent");
        
        let failed_intents = graph.get_intents_by_status(IntentStatus::Failed);
        assert_eq!(failed_intents.len(), 1);
        assert_eq!(failed_intents[0].goal, "Failed intent");
        
        let suspended_intents = graph.get_intents_by_status(IntentStatus::Suspended);
        assert_eq!(suspended_intents.len(), 1);
        assert_eq!(suspended_intents[0].goal, "Suspended intent");
        
        let archived_intents = graph.get_intents_by_status(IntentStatus::Archived);
        assert_eq!(archived_intents.len(), 1);
        assert_eq!(archived_intents[0].goal, "Archived intent");
    }

    #[test]
    fn test_get_ready_intents() {
        let mut graph = IntentGraph::new().unwrap();
        
        // Create intents with different statuses
        let mut active_intent1 = StorableIntent::new("Active intent 1".to_string());
        active_intent1.status = IntentStatus::Active;
        
        let mut active_intent2 = StorableIntent::new("Active intent 2".to_string());
        active_intent2.status = IntentStatus::Active;
        
        let mut completed_intent = StorableIntent::new("Completed intent".to_string());
        completed_intent.status = IntentStatus::Completed;
        
        let mut failed_intent = StorableIntent::new("Failed intent".to_string());
        failed_intent.status = IntentStatus::Failed;
        
        graph.store_intent(active_intent1).unwrap();
        graph.store_intent(active_intent2).unwrap();
        graph.store_intent(completed_intent).unwrap();
        graph.store_intent(failed_intent).unwrap();
        
        // Test getting ready intents (Active status)
        let ready_intents = graph.get_ready_intents();
        assert_eq!(ready_intents.len(), 2);
        let goals: Vec<String> = ready_intents.iter().map(|i| i.goal.clone()).collect();
        assert!(goals.contains(&"Active intent 1".to_string()));
        assert!(goals.contains(&"Active intent 2".to_string()));
    }

    #[test]
    fn test_get_intents_needing_attention() {
        let mut graph = IntentGraph::new().unwrap();
        
        // Create intents with different statuses
        let mut active_intent = StorableIntent::new("Active intent".to_string());
        active_intent.status = IntentStatus::Active;
        
        let mut failed_intent1 = StorableIntent::new("Failed intent 1".to_string());
        failed_intent1.status = IntentStatus::Failed;
        
        let mut failed_intent2 = StorableIntent::new("Failed intent 2".to_string());
        failed_intent2.status = IntentStatus::Failed;
        
        let mut suspended_intent = StorableIntent::new("Suspended intent".to_string());
        suspended_intent.status = IntentStatus::Suspended;
        
        let mut completed_intent = StorableIntent::new("Completed intent".to_string());
        completed_intent.status = IntentStatus::Completed;
        
        graph.store_intent(active_intent).unwrap();
        graph.store_intent(failed_intent1).unwrap();
        graph.store_intent(failed_intent2).unwrap();
        graph.store_intent(suspended_intent).unwrap();
        graph.store_intent(completed_intent).unwrap();
        
        // Test getting intents needing attention (Failed or Suspended)
        let needing_attention = graph.get_intents_needing_attention();
        assert_eq!(needing_attention.len(), 3); // 2 failed + 1 suspended
        let goals: Vec<String> = needing_attention.iter().map(|i| i.goal.clone()).collect();
        assert!(goals.contains(&"Failed intent 1".to_string()));
        assert!(goals.contains(&"Failed intent 2".to_string()));
        assert!(goals.contains(&"Suspended intent".to_string()));
    }

    #[test]
    fn test_bulk_transition_intents() {
        let mut graph = IntentGraph::new().unwrap();
        
        // Create multiple intents
        let intent1 = StorableIntent::new("Intent 1".to_string());
        let intent2 = StorableIntent::new("Intent 2".to_string());
        let intent3 = StorableIntent::new("Intent 3".to_string());
        
        let intent1_id = intent1.intent_id.clone();
        let intent2_id = intent2.intent_id.clone();
        let intent3_id = intent3.intent_id.clone();
        
        graph.store_intent(intent1).unwrap();
        graph.store_intent(intent2).unwrap();
        graph.store_intent(intent3).unwrap();
        
        // Bulk suspend all intents
        let intent_ids = vec![intent1_id.clone(), intent2_id.clone(), intent3_id.clone()];
        let result = graph.bulk_transition_intents(
            &intent_ids,
            IntentStatus::Suspended,
            "System maintenance".to_string(),
        );
        assert!(result.is_ok());
        
        let successful = result.unwrap();
        assert_eq!(successful.len(), 3);
        assert!(successful.contains(&intent1_id));
        assert!(successful.contains(&intent2_id));
        assert!(successful.contains(&intent3_id));
        
        // Verify all intents are suspended
        let suspended_intents = graph.get_intents_by_status(IntentStatus::Suspended);
        assert_eq!(suspended_intents.len(), 3);
        
        // Bulk resume all intents
        let result = graph.bulk_transition_intents(
            &intent_ids,
            IntentStatus::Active,
            "Maintenance complete".to_string(),
        );
        assert!(result.is_ok());
        
        // Verify all intents are active
        let active_intents = graph.get_intents_by_status(IntentStatus::Active);
        assert_eq!(active_intents.len(), 3);
    }

    #[test]
    fn test_get_intents_ready_for_archival() {
        let mut graph = IntentGraph::new().unwrap();
        
        // Create completed intents with different timestamps
        let mut old_completed = StorableIntent::new("Old completed intent".to_string());
        old_completed.status = IntentStatus::Completed;
        old_completed.updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() - (30 * 24 * 60 * 60); // 30 days ago
        
        let mut recent_completed = StorableIntent::new("Recent completed intent".to_string());
        recent_completed.status = IntentStatus::Completed;
        recent_completed.updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() - (5 * 24 * 60 * 60); // 5 days ago
        
        let mut active_intent = StorableIntent::new("Active intent".to_string());
        active_intent.status = IntentStatus::Active;
        
        graph.store_intent(old_completed).unwrap();
        graph.store_intent(recent_completed).unwrap();
        graph.store_intent(active_intent).unwrap();
        
        // Test getting intents ready for archival (older than 7 days)
        let ready_for_archival = graph.get_intents_ready_for_archival(7);
        assert_eq!(ready_for_archival.len(), 1);
        assert_eq!(ready_for_archival[0].goal, "Old completed intent");
        
        // Test with 1 day threshold (should include recent completed)
        let ready_for_archival = graph.get_intents_ready_for_archival(1);
        assert_eq!(ready_for_archival.len(), 2); // Both completed intents
    }

    #[test]
    fn test_causal_chain_integration() {
        let mut graph = IntentGraph::new().unwrap();
        let mut causal_chain = crate::ccos::causal_chain::CausalChain::new().unwrap();
        
        // Create test intent
        let intent = StorableIntent::new("Test intent for causal chain integration".to_string());
        let intent_id = intent.intent_id.clone();
        
        graph.store_intent(intent).unwrap();
        
        // Test transition with causal chain logging
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut intent = graph.storage.get_intent(&intent_id).await.unwrap().unwrap();
            
            // Perform a status transition with causal chain logging
            graph.lifecycle.transition_intent_status(
                &mut graph.storage,
                Some(&mut causal_chain),
                &mut intent,
                IntentStatus::Suspended,
                "Testing causal chain integration".to_string(),
                Some("test-plan-123"),
            ).await.unwrap();
            
            // Verify intent metadata contains audit trail
            let updated_intent = graph.storage.get_intent(&intent_id).await.unwrap().unwrap();
            let has_audit_entry = updated_intent.metadata
                .keys()
                .any(|key| key.starts_with("status_transition_"));
            assert!(has_audit_entry, "Intent should have audit trail in metadata");
            
            // Verify causal chain contains the action
            let actions_for_intent = causal_chain.get_actions_for_intent(&intent_id);
            assert!(!actions_for_intent.is_empty(), "Causal chain should contain actions for intent");
            
            // Find the status change action
            let status_change_action = actions_for_intent.iter()
                .find(|action| action.action_type == crate::ccos::types::ActionType::IntentStatusChanged);
            assert!(status_change_action.is_some(), "Should have status change action in causal chain");
            
            let action = status_change_action.unwrap();
            assert_eq!(action.intent_id, intent_id);
            assert_eq!(action.plan_id, "test-plan-123");
            
            // Verify metadata contains transition details
            assert!(action.metadata.contains_key("old_status"));
            assert!(action.metadata.contains_key("new_status"));
            assert!(action.metadata.contains_key("reason"));
            assert_eq!(action.metadata.get("old_status").unwrap(), &crate::runtime::Value::String("Active".to_string()));
            assert_eq!(action.metadata.get("new_status").unwrap(), &crate::runtime::Value::String("Suspended".to_string()));
            assert_eq!(action.metadata.get("reason").unwrap(), &crate::runtime::Value::String("Testing causal chain integration".to_string()));
        });
    }

    #[test]
    fn test_dual_audit_trail_consistency() {
        let mut graph = IntentGraph::new().unwrap();
        let mut causal_chain = crate::ccos::causal_chain::CausalChain::new().unwrap();
        
        // Create test intent
        let intent = StorableIntent::new("Test dual audit trail".to_string());
        let intent_id = intent.intent_id.clone();
        
        graph.store_intent(intent).unwrap();
        
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            // Perform multiple transitions
            let transitions = vec![
                (IntentStatus::Suspended, "First transition"),
                (IntentStatus::Active, "Resume after approval"),
                (IntentStatus::Failed, "Encountered error"),
                (IntentStatus::Active, "Retry after fix"),
                (IntentStatus::Completed, "Successfully completed"),
                (IntentStatus::Archived, "Project finished"),
            ];
            
            for (new_status, reason) in transitions {
                let mut intent = graph.storage.get_intent(&intent_id).await.unwrap().unwrap();
                
                graph.lifecycle.transition_intent_status(
                    &mut graph.storage,
                    Some(&mut causal_chain),
                    &mut intent,
                    new_status,
                    reason.to_string(),
                    Some("test-plan-456"),
                ).await.unwrap();
            }
            
            // Verify consistency between metadata and causal chain
            let final_intent = graph.storage.get_intent(&intent_id).await.unwrap().unwrap();
            let metadata_transitions: Vec<_> = final_intent.metadata
                .keys()
                .filter(|key| key.starts_with("status_transition_"))
                .collect();
            
            let causal_chain_actions = causal_chain.get_actions_for_intent(&intent_id);
            let status_change_actions: Vec<_> = causal_chain_actions.iter()
                .filter(|action| action.action_type == crate::ccos::types::ActionType::IntentStatusChanged)
                .collect();
            
            // Should have same number of transitions in both audit trails
            assert_eq!(metadata_transitions.len(), status_change_actions.len(), 
                      "Metadata and causal chain should have same number of transitions");
            
            // Verify all transitions are recorded in both places
            assert_eq!(metadata_transitions.len(), 6, "Should have 6 transitions in metadata");
            assert_eq!(status_change_actions.len(), 6, "Should have 6 transitions in causal chain");
            
            // Verify final status consistency
            assert_eq!(final_intent.status, IntentStatus::Archived);
            
            // Verify causal chain actions have proper metadata
            for action in &status_change_actions {
                assert!(action.metadata.contains_key("old_status"));
                assert!(action.metadata.contains_key("new_status"));
                assert!(action.metadata.contains_key("reason"));
                assert!(action.metadata.contains_key("signature"), "All actions should be cryptographically signed");
            }
        });
    }

    /// Test comprehensive query API functionality
    #[test]
    fn test_intent_query_api() {
        let mut graph = IntentGraph::new().unwrap();
        
        // Create test intents with various statuses and metadata
        let mut intent1 = StorableIntent::new("Deploy web service".to_string());
        intent1.status = IntentStatus::Active;
        intent1.metadata.insert("priority".to_string(), "high".to_string());
        
        let mut intent2 = StorableIntent::new("Setup database".to_string());
        intent2.status = IntentStatus::Completed;
        intent2.metadata.insert("priority".to_string(), "medium".to_string());
        
        let mut intent3 = StorableIntent::new("Configure monitoring".to_string());
        intent3.status = IntentStatus::Failed;
        intent3.metadata.insert("priority".to_string(), "low".to_string());
        
        let intent1_id = intent1.intent_id.clone();
        let intent2_id = intent2.intent_id.clone();
        let intent3_id = intent3.intent_id.clone();
        
        graph.store_intent(intent1).unwrap();
        graph.store_intent(intent2).unwrap();
        graph.store_intent(intent3).unwrap();
        
        // Create some relationships
        graph.create_edge(intent1_id.clone(), intent2_id.clone(), EdgeType::DependsOn).unwrap();
        graph.create_edge(intent1_id.clone(), intent3_id.clone(), EdgeType::RelatedTo).unwrap();
        
        // Create query API after graph setup
        let query_api = IntentGraphQueryAPI::from_graph(graph);
        
        // Test status filter
        let query = IntentQuery {
            status_filter: Some(vec![IntentStatus::Active]),
            ..Default::default()
        };
        let result = query_api.query_intents(query).unwrap();
        assert_eq!(result.intents.len(), 1);
        assert_eq!(result.intents[0].goal, "Deploy web service");
        
        // Test goal text filter
        let query = IntentQuery {
            goal_contains: Some("web".to_string()),
            ..Default::default()
        };
        let result = query_api.query_intents(query).unwrap();
        assert_eq!(result.intents.len(), 1);
        assert_eq!(result.intents[0].goal, "Deploy web service");
        
        // Test metadata filter
        let mut metadata_filter = HashMap::new();
        metadata_filter.insert("priority".to_string(), "high".to_string());
        let query = IntentQuery {
            metadata_filter: Some(metadata_filter),
            ..Default::default()
        };
        let result = query_api.query_intents(query).unwrap();
        assert_eq!(result.intents.len(), 1);
        assert_eq!(result.intents[0].goal, "Deploy web service");
        
        // Test relationship type filter
        let query = IntentQuery {
            has_relationship_types: Some(vec![EdgeType::DependsOn]),
            ..Default::default()
        };
        let result = query_api.query_intents(query).unwrap();
        assert_eq!(result.intents.len(), 1); // Only intent1 has DependsOn relationship
        
        // Test limit
        let query = IntentQuery {
            limit: Some(2),
            ..Default::default()
        };
        let result = query_api.query_intents(query).unwrap();
        assert_eq!(result.intents.len(), 2);
        assert_eq!(result.total_count, 3); // Total before limit
        assert!(result.truncated);
        
        // Test sorting by goal alphabetical
        let query = IntentQuery {
            sort_by: Some(IntentSortCriteria::GoalAlphabetical(SortOrder::Ascending)),
            ..Default::default()
        };
        let result = query_api.query_intents(query).unwrap();
        assert!(result.intents[0].goal < result.intents[1].goal);
        assert!(result.intents[1].goal < result.intents[2].goal);
    }

    #[test]
    fn test_edge_query_api() {
        let mut graph = IntentGraph::new().unwrap();
        
        // Create test intents
        let intent1 = StorableIntent::new("Intent 1".to_string());
        let intent2 = StorableIntent::new("Intent 2".to_string());
        let intent3 = StorableIntent::new("Intent 3".to_string());
        
        let intent1_id = intent1.intent_id.clone();
        let intent2_id = intent2.intent_id.clone();
        let intent3_id = intent3.intent_id.clone();
        
        graph.store_intent(intent1).unwrap();
        graph.store_intent(intent2).unwrap();
        graph.store_intent(intent3).unwrap();
        
        // Create edges with different types and weights
        let mut metadata1 = HashMap::new();
        metadata1.insert("reason".to_string(), "infrastructure".to_string());
        graph.create_weighted_edge(intent1_id.clone(), intent2_id.clone(), EdgeType::DependsOn, 0.8, metadata1).unwrap();
        
        let mut metadata2 = HashMap::new();
        metadata2.insert("reason".to_string(), "monitoring".to_string());
        graph.create_weighted_edge(intent1_id.clone(), intent3_id.clone(), EdgeType::RelatedTo, 0.3, metadata2).unwrap();
        
        graph.create_edge(intent2_id.clone(), intent3_id.clone(), EdgeType::ConflictsWith).unwrap();
        
        // Create query API after graph setup
        let query_api = IntentGraphQueryAPI::from_graph(graph);
        
        // Test edge type filter
        let query = EdgeQuery {
            edge_types: Some(vec![EdgeType::DependsOn]),
            ..Default::default()
        };
        let result = query_api.query_edges(query).unwrap();
        assert_eq!(result.edges.len(), 1);
        assert_eq!(result.edges[0].edge_type, EdgeType::DependsOn);
        
        // Test weight filter
        let query = EdgeQuery {
            min_weight: Some(0.5),
            ..Default::default()
        };
        let result = query_api.query_edges(query).unwrap();
        assert_eq!(result.edges.len(), 1);
        assert_eq!(result.edges[0].weight, Some(0.8));
        
        // Test metadata filter
        let mut metadata_filter = HashMap::new();
        metadata_filter.insert("reason".to_string(), "infrastructure".to_string());
        let query = EdgeQuery {
            metadata_filter: Some(metadata_filter),
            ..Default::default()
        };
        let result = query_api.query_edges(query).unwrap();
        assert_eq!(result.edges.len(), 1);
        
        // Test from_intent filter
        let query = EdgeQuery {
            from_intent: Some(intent1_id.clone()),
            ..Default::default()
        };
        let result = query_api.query_edges(query).unwrap();
        assert_eq!(result.edges.len(), 2); // Two edges from intent1
        
        // Test involves_intent filter
        let query = EdgeQuery {
            involves_intent: Some(intent3_id.clone()),
            ..Default::default()
        };
        let result = query_api.query_edges(query).unwrap();
        assert_eq!(result.edges.len(), 2); // Two edges involving intent3
    }

    #[test]
    fn test_visualization_data_export() {
        let mut graph = IntentGraph::new().unwrap();
        
        // Create test data
        let mut intent1 = StorableIntent::new("Web Service Deployment".to_string());
        intent1.status = IntentStatus::Active;
        let mut intent2 = StorableIntent::new("Database Setup".to_string());
        intent2.status = IntentStatus::Completed;
        
        let intent1_id = intent1.intent_id.clone();
        let intent2_id = intent2.intent_id.clone();
        
        graph.store_intent(intent1).unwrap();
        graph.store_intent(intent2).unwrap();
        
        // Create relationship
        graph.create_weighted_edge(intent1_id.clone(), intent2_id.clone(), EdgeType::DependsOn, 0.8, HashMap::new()).unwrap();
        
        // Create query API after graph setup
        let query_api = IntentGraphQueryAPI::from_graph(graph);
        
        // Test visualization data export
        let viz_data = query_api.export_visualization_data().unwrap();
        
        // Verify nodes
        assert_eq!(viz_data.nodes.len(), 2);
        let web_service_node = viz_data.nodes.iter().find(|n| n.id == intent1_id).unwrap();
        assert_eq!(web_service_node.label, "Web Service Deployment");
        assert_eq!(web_service_node.status, IntentStatus::Active);
        assert_eq!(web_service_node.color, "#4CAF50"); // Green for Active
        
        let database_node = viz_data.nodes.iter().find(|n| n.id == intent2_id).unwrap();
        assert_eq!(database_node.status, IntentStatus::Completed);
        assert_eq!(database_node.color, "#2196F3"); // Blue for Completed
        
        // Verify edges
        assert_eq!(viz_data.edges.len(), 1);
        let edge = &viz_data.edges[0];
        assert_eq!(edge.from, intent1_id);
        assert_eq!(edge.to, intent2_id);
        assert_eq!(edge.edge_type, EdgeType::DependsOn);
        assert_eq!(edge.label, "depends on");
        assert_eq!(edge.color, "#FF5722"); // Orange for DependsOn
        
        // Verify metadata
        assert_eq!(viz_data.metadata.node_count, 2);
        assert_eq!(viz_data.metadata.edge_count, 1);
        assert_eq!(viz_data.metadata.statistics.status_distribution.get(&IntentStatus::Active), Some(&1));
        assert_eq!(viz_data.metadata.statistics.status_distribution.get(&IntentStatus::Completed), Some(&1));
    }

    #[test]
    fn test_export_formats() {
        let mut graph = IntentGraph::new().unwrap();
        
        // Create minimal test data
        let intent1 = StorableIntent::new("Test Intent 1".to_string());
        let intent2 = StorableIntent::new("Test Intent 2".to_string());
        
        let intent1_id = intent1.intent_id.clone();
        let intent2_id = intent2.intent_id.clone();
        
        graph.store_intent(intent1).unwrap();
        graph.store_intent(intent2).unwrap();
        graph.create_edge(intent1_id.clone(), intent2_id.clone(), EdgeType::DependsOn).unwrap();
        
        // Create query API after graph setup
        let query_api = IntentGraphQueryAPI::from_graph(graph);
        
        // Test JSON export
        let json_data = query_api.export_graph_data(ExportFormat::Json).unwrap();
        assert!(json_data.contains("nodes"));
        assert!(json_data.contains("edges"));
        assert!(json_data.contains("metadata"));
        
        // Test Graphviz export
        let dot_data = query_api.export_graph_data(ExportFormat::Graphviz).unwrap();
        assert!(dot_data.starts_with("digraph IntentGraph"));
        assert!(dot_data.contains(&intent1_id));
        assert!(dot_data.contains(&intent2_id));
        assert!(dot_data.contains("->"));
        
        // Test Mermaid export
        let mermaid_data = query_api.export_graph_data(ExportFormat::Mermaid).unwrap();
        assert!(mermaid_data.starts_with("graph TD"));
        assert!(mermaid_data.contains("-->"));
        
        // Test Cytoscape export
        let cytoscape_data = query_api.export_graph_data(ExportFormat::Cytoscape).unwrap();
        assert!(cytoscape_data.contains("nodes"));
        assert!(cytoscape_data.contains("edges"));
        
        // Test D3 Force export
        let d3_data = query_api.export_graph_data(ExportFormat::D3Force).unwrap();
        assert!(d3_data.contains("nodes"));
        assert!(d3_data.contains("links"));
    }

    #[test]
    fn test_graph_statistics() {
        let mut graph = IntentGraph::new().unwrap();
        
        // Create diverse test data
        let mut intent1 = StorableIntent::new("Active Intent".to_string());
        intent1.status = IntentStatus::Active;
        let mut intent2 = StorableIntent::new("Completed Intent".to_string());
        intent2.status = IntentStatus::Completed;
        let mut intent3 = StorableIntent::new("Failed Intent".to_string());
        intent3.status = IntentStatus::Failed;
        let intent4 = StorableIntent::new("Isolated Intent".to_string()); // No connections
        
        let intent1_id = intent1.intent_id.clone();
        let intent2_id = intent2.intent_id.clone();
        let intent3_id = intent3.intent_id.clone();
        let intent4_id = intent4.intent_id.clone();
        
        graph.store_intent(intent1).unwrap();
        graph.store_intent(intent2).unwrap();
        graph.store_intent(intent3).unwrap();
        graph.store_intent(intent4).unwrap();
        
        // Create connections (leaving intent4 isolated)
        graph.create_edge(intent1_id.clone(), intent2_id.clone(), EdgeType::DependsOn).unwrap();
        graph.create_edge(intent1_id.clone(), intent3_id.clone(), EdgeType::RelatedTo).unwrap();
        graph.create_edge(intent2_id.clone(), intent3_id.clone(), EdgeType::ConflictsWith).unwrap();
        
        // Create query API after graph setup
        let query_api = IntentGraphQueryAPI::from_graph(graph);
        
        // Test statistics
        let stats = query_api.get_graph_statistics().unwrap();
        
        // Status distribution
        assert_eq!(stats.status_distribution.get(&IntentStatus::Active), Some(&2)); // intent1 and intent4 are both Active
        assert_eq!(stats.status_distribution.get(&IntentStatus::Completed), Some(&1));
        assert_eq!(stats.status_distribution.get(&IntentStatus::Failed), Some(&1));
        assert_eq!(stats.status_distribution.get(&IntentStatus::Archived), None); // No archived intents, so not in HashMap
        
        // Edge type distribution
        assert_eq!(stats.edge_type_distribution.get(&EdgeType::DependsOn), Some(&1));
        assert_eq!(stats.edge_type_distribution.get(&EdgeType::RelatedTo), Some(&1));
        assert_eq!(stats.edge_type_distribution.get(&EdgeType::ConflictsWith), Some(&1));
        
        // Isolated intents
        assert_eq!(stats.isolated_intents.len(), 1);
        assert!(stats.isolated_intents.contains(&intent4_id));
        
        // Highly connected intents
        assert!(!stats.highly_connected_intents.is_empty());
        let most_connected = &stats.highly_connected_intents[0];
        assert_eq!(most_connected.1, 2); // Most connected intent should have 2 connections
        // Verify that intent1_id is indeed the most connected one
        assert!(stats.highly_connected_intents.iter().any(|(id, count)| id == &intent1_id && *count == 2));
    }

    #[test]
    fn test_debug_info_and_health_score() {
        let mut graph = IntentGraph::new().unwrap();
        
        // Create test scenario with some problems
        let mut intent1 = StorableIntent::new("Working Intent".to_string());
        intent1.status = IntentStatus::Active;
        let mut intent2 = StorableIntent::new("Failed Intent".to_string());
        intent2.status = IntentStatus::Failed;
        let intent3 = StorableIntent::new("Isolated Intent".to_string());
        
        let intent1_id = intent1.intent_id.clone();
        let intent2_id = intent2.intent_id.clone();
        let intent3_id = intent3.intent_id.clone();
        
        graph.store_intent(intent1).unwrap();
        graph.store_intent(intent2).unwrap();
        graph.store_intent(intent3).unwrap();
        
        // Only connect intent1 and intent2
        graph.create_edge(intent1_id.clone(), intent2_id.clone(), EdgeType::DependsOn).unwrap();
        
        // Create query API after graph setup
        let query_api = IntentGraphQueryAPI::from_graph(graph);
        
        // Get debug info
        let debug_info = query_api.get_debug_info().unwrap();
        
        // Health score should be less than perfect due to failed intent and isolation
        assert!(debug_info.health_score < 1.0);
        assert!(debug_info.health_score > 0.0);
        
        // Should have recommendations
        assert!(!debug_info.recommendations.is_empty());
        let recommendations_text = debug_info.recommendations.join(" ");
        assert!(recommendations_text.contains("isolated") || recommendations_text.contains("failed"));
        
        // Should detect orphaned intents
        assert_eq!(debug_info.orphaned_intents.len(), 1);
        assert!(debug_info.orphaned_intents.contains(&intent3_id));
        
        // Cycles should be empty for this simple graph
        assert!(debug_info.cycles.is_empty());
    }

    #[test]
    fn test_cycle_detection() {
        let mut graph = IntentGraph::new().unwrap();
        
        // Create intentional cycle: A -> B -> C -> A
        let intent_a = StorableIntent::new("Intent A".to_string());
        let intent_b = StorableIntent::new("Intent B".to_string());
        let intent_c = StorableIntent::new("Intent C".to_string());
        
        let intent_a_id = intent_a.intent_id.clone();
        let intent_b_id = intent_b.intent_id.clone();
        let intent_c_id = intent_c.intent_id.clone();
        
        graph.store_intent(intent_a).unwrap();
        graph.store_intent(intent_b).unwrap();
        graph.store_intent(intent_c).unwrap();
        
        // Create cycle
        graph.create_edge(intent_a_id.clone(), intent_b_id.clone(), EdgeType::DependsOn).unwrap();
        
        graph.create_edge(intent_b_id.clone(), intent_c_id.clone(), EdgeType::DependsOn).unwrap();
        graph.create_edge(intent_c_id.clone(), intent_a_id.clone(), EdgeType::DependsOn).unwrap();
        
        // Create query API after graph setup
        let query_api = IntentGraphQueryAPI::from_graph(graph);
        
        let debug_info = query_api.get_debug_info().unwrap();
        
        // Should detect the cycle
        assert!(!debug_info.cycles.is_empty());
        let cycle = &debug_info.cycles[0];
        assert!(cycle.len() >= 3); // Should contain at least the 3 intents
        
        // Health score should be affected by the cycle
        assert!(debug_info.health_score < 1.0);
    }

    #[test]
    fn test_quick_search() {
        let mut graph = IntentGraph::new().unwrap();
        
        // Create searchable intents
        let intent1 = StorableIntent::new("Deploy web service with monitoring".to_string());
        let intent2 = StorableIntent::new("Setup database cluster".to_string());
        let intent3 = StorableIntent::new("Configure web proxy".to_string());
        
        graph.store_intent(intent1).unwrap();
        graph.store_intent(intent2).unwrap();
        graph.store_intent(intent3).unwrap();
        
        // Create query API after graph setup
        let query_api = IntentGraphQueryAPI::from_graph(graph);
        
        // Test quick search
        let results = query_api.quick_search("web", None).unwrap();
        assert_eq!(results.len(), 2); // Should find "web service" and "web proxy"
        
        let goals: Vec<String> = results.iter().map(|i| i.goal.clone()).collect();
        assert!(goals.iter().any(|g| g.contains("service")));
        assert!(goals.iter().any(|g| g.contains("proxy")));
        
        // Test with limit
        let results = query_api.quick_search("web", Some(1)).unwrap();
        assert_eq!(results.len(), 1);
        
        // Test search that should find nothing
        let results = query_api.quick_search("nonexistent", None).unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_related_intents() {
        let mut graph = IntentGraph::new().unwrap();
        
        // Create a network of related intents
        let intent1 = StorableIntent::new("Root Intent".to_string());
        let intent2 = StorableIntent::new("Direct Child".to_string());
        let intent3 = StorableIntent::new("Grandchild".to_string());
        let intent4 = StorableIntent::new("Sibling".to_string());
        let intent5 = StorableIntent::new("Isolated Intent".to_string());
        
        let intent1_id = intent1.intent_id.clone();
        let intent2_id = intent2.intent_id.clone();
        let intent3_id = intent3.intent_id.clone();
        let intent4_id = intent4.intent_id.clone();
        let intent5_id = intent5.intent_id.clone();
        
        graph.store_intent(intent1).unwrap();
        graph.store_intent(intent2).unwrap();
        graph.store_intent(intent3).unwrap();
        graph.store_intent(intent4).unwrap();
        graph.store_intent(intent5).unwrap();
        
        // Create relationships
        graph.create_edge(intent1_id.clone(), intent2_id.clone(), EdgeType::DependsOn).unwrap();
        graph.create_edge(intent2_id.clone(), intent3_id.clone(), EdgeType::DependsOn).unwrap();
        graph.create_edge(intent1_id.clone(), intent4_id.clone(), EdgeType::RelatedTo).unwrap();
        // intent5 remains isolated
        
        // Create query API after graph setup
        let query_api = IntentGraphQueryAPI::from_graph(graph);
        
        // Test related intents with depth 1
        let related = query_api.get_related_intents(&intent1_id, 1).unwrap();
        assert_eq!(related.len(), 2); // Should find intent2 and intent4
        let related_ids: Vec<String> = related.iter().map(|i| i.intent_id.clone()).collect();
        assert!(related_ids.contains(&intent2_id));
        assert!(related_ids.contains(&intent4_id));
        assert!(!related_ids.contains(&intent3_id)); // Too far (depth 2)
        
        // Test related intents with depth 2
        let related = query_api.get_related_intents(&intent1_id, 2).unwrap();
        assert_eq!(related.len(), 3); // Should find intent2, intent3, and intent4
        let related_ids: Vec<String> = related.iter().map(|i| i.intent_id.clone()).collect();
        assert!(related_ids.contains(&intent2_id));
        assert!(related_ids.contains(&intent3_id));
        assert!(related_ids.contains(&intent4_id));
        assert!(!related_ids.contains(&intent5_id)); // Still isolated
        
        // Test from isolated intent
        let related = query_api.get_related_intents(&intent5_id, 3).unwrap();
        assert_eq!(related.len(), 0); // No connections
    }

    #[test]
    fn test_filtered_visualization_export() {
        let mut graph = IntentGraph::new().unwrap();
        
        // Create test data with different statuses
        let mut intent1 = StorableIntent::new("Active Task".to_string());
        intent1.status = IntentStatus::Active;
        let mut intent2 = StorableIntent::new("Completed Task".to_string());
        intent2.status = IntentStatus::Completed;
        let mut intent3 = StorableIntent::new("Failed Task".to_string());
        intent3.status = IntentStatus::Failed;
        
        let intent1_id = intent1.intent_id.clone();
        let intent2_id = intent2.intent_id.clone();
        let intent3_id = intent3.intent_id.clone();
        
        graph.store_intent(intent1).unwrap();
        graph.store_intent(intent2).unwrap();
        graph.store_intent(intent3).unwrap();
        
        // Create relationships
        graph.create_edge(intent1_id.clone(), intent2_id.clone(), EdgeType::DependsOn).unwrap();
        graph.create_edge(intent1_id.clone(), intent3_id.clone(), EdgeType::ConflictsWith).unwrap();
        
        // Create query API after graph setup
        let query_api = IntentGraphQueryAPI::from_graph(graph);
        
        // Test filtered export - only active intents
        let intent_filter = IntentQuery {
            status_filter: Some(vec![IntentStatus::Active]),
            ..Default::default()
        };
        
        let viz_data = query_api.export_filtered_visualization_data(
            Some(intent_filter),
            None
        ).unwrap();
        
        // Should only have active intents in nodes
        assert_eq!(viz_data.nodes.len(), 1);
        assert_eq!(viz_data.nodes[0].id, intent1_id);
        assert_eq!(viz_data.nodes[0].status, IntentStatus::Active);
        
        // Test edge filtering - only ConflictsWith relationships
        let edge_filter = EdgeQuery {
            edge_types: Some(vec![EdgeType::ConflictsWith]),
            ..Default::default()
        };
        
        let viz_data = query_api.export_filtered_visualization_data(
            None,
            Some(edge_filter)
        ).unwrap();
        
        // Should have all nodes but only ConflictsWith edges
        assert_eq!(viz_data.nodes.len(), 3);
        assert_eq!(viz_data.edges.len(), 1);
        assert_eq!(viz_data.edges[0].edge_type, EdgeType::ConflictsWith);
    }
}

#[derive(Debug)]
pub struct IntentGraphQueryAPI {
    // Store reference to graph instead of owning it
    graph_ref: std::sync::Arc<IntentGraph>,
}

/// Comprehensive query structure for advanced Intent Graph queries
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IntentQuery {
    /// Filter by intent status(es)
    pub status_filter: Option<Vec<IntentStatus>>,
    /// Filter by intent goals containing text
    pub goal_contains: Option<String>,
    /// Filter by metadata key-value pairs
    pub metadata_filter: Option<HashMap<String, String>>,
    /// Filter by creation date range
    pub created_after: Option<u64>,
    pub created_before: Option<u64>,
    /// Filter by update date range
    pub updated_after: Option<u64>,
    pub updated_before: Option<u64>,
    /// Filter by relationship types
    pub has_relationship_types: Option<Vec<EdgeType>>,
    /// Filter by connection to specific intent IDs
    pub connected_to: Option<Vec<IntentId>>,
    /// Semantic search query
    pub semantic_query: Option<String>,
    /// Maximum number of results
    pub limit: Option<usize>,
    /// Sort criteria
    pub sort_by: Option<IntentSortCriteria>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum IntentSortCriteria {
    CreatedDate(SortOrder),
    UpdatedDate(SortOrder),
    GoalAlphabetical(SortOrder),
    ConnectionCount(SortOrder),
    RelevanceScore(SortOrder),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SortOrder {
    Ascending,
    Descending,
}

/// Edge/Relationship query structure
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EdgeQuery {
    /// Filter by edge types
    pub edge_types: Option<Vec<EdgeType>>,
    /// Filter by weight range
    pub min_weight: Option<f64>,
    pub max_weight: Option<f64>,
    /// Filter by metadata
    pub metadata_filter: Option<HashMap<String, String>>,
    /// Filter by source intent
    pub from_intent: Option<IntentId>,
    /// Filter by target intent
    pub to_intent: Option<IntentId>,
    /// Filter by any involvement of intent
    pub involves_intent: Option<IntentId>,
}

/// Visualization export data structures
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GraphVisualizationData {
    /// All nodes (intents) in the graph
    pub nodes: Vec<VisualizationNode>,
    /// All edges (relationships) in the graph
    pub edges: Vec<VisualizationEdge>,
    /// Metadata about the graph
    pub metadata: GraphMetadata,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VisualizationNode {
    /// Intent ID
    pub id: IntentId,
    /// Display label
    pub label: String,
    /// Node type/category
    pub node_type: String,
    /// Current status
    pub status: IntentStatus,
    /// Size hint for visualization (based on connections, importance, etc.)
    pub size: f64,
    /// Color hint for visualization
    pub color: String,
    /// Additional metadata for tooltips/details
    pub metadata: HashMap<String, String>,
    /// Position hints for layout algorithms
    pub position: Option<VisualizationPosition>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VisualizationEdge {
    /// Source intent ID
    pub from: IntentId,
    /// Target intent ID
    pub to: IntentId,
    /// Edge type
    pub edge_type: EdgeType,
    /// Visual weight/thickness hint
    pub weight: Option<f64>,
    /// Edge label
    pub label: String,
    /// Color hint for visualization
    pub color: String,
    /// Metadata for tooltips/details
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VisualizationPosition {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GraphMetadata {
    /// Total number of nodes
    pub node_count: usize,
    /// Total number of edges
    pub edge_count: usize,
    /// Graph statistics
    pub statistics: GraphStatistics,
    /// Layout hints
    pub layout_hints: LayoutHints,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GraphStatistics {
    /// Count by status
    pub status_distribution: HashMap<IntentStatus, usize>,
    /// Count by edge type
    pub edge_type_distribution: HashMap<EdgeType, usize>,
    /// Average connections per node
    pub avg_connections_per_node: f64,
    /// Most connected intents
    pub highly_connected_intents: Vec<(IntentId, usize)>,
    /// Isolated intents (no connections)
    pub isolated_intents: Vec<IntentId>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LayoutHints {
    /// Suggested layout algorithm
    pub suggested_layout: String,
    /// Whether graph has hierarchical structure
    pub is_hierarchical: bool,
    /// Suggested clustering
    pub clusters: Vec<IntentCluster>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IntentCluster {
    /// Cluster identifier
    pub cluster_id: String,
    /// Intents in this cluster
    pub intent_ids: Vec<IntentId>,
    /// Cluster center (most connected intent)
    pub center_intent: Option<IntentId>,
    /// Cluster theme/category
    pub theme: String,
}

/// Query result structures
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IntentQueryResult {
    /// Matching intents
    pub intents: Vec<StorableIntent>,
    /// Total count (may be higher than returned if limited)
    pub total_count: usize,
    /// Query execution time in milliseconds
    pub execution_time_ms: u64,
    /// Whether results were truncated
    pub truncated: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EdgeQueryResult {
    /// Matching edges
    pub edges: Vec<Edge>,
    /// Total count
    pub total_count: usize,
    /// Query execution time in milliseconds
    pub execution_time_ms: u64,
    /// Whether results were truncated
    pub truncated: bool,
}

impl IntentGraphQueryAPI {
    /// Create a new query API instance
    pub fn new(graph: std::sync::Arc<IntentGraph>) -> Self {
        Self { graph_ref: graph }
    }
    
    /// Create a new query API instance from an IntentGraph
    pub fn from_graph(graph: IntentGraph) -> Self {
        Self { graph_ref: std::sync::Arc::new(graph) }
    }

    /// Execute an advanced intent query
    pub fn query_intents(&self, query: IntentQuery) -> Result<IntentQueryResult, RuntimeError> {
        let start_time = std::time::Instant::now();
        
        // Start with all intents
        let all_intents = self.graph_ref.rt.block_on(async {
            self.graph_ref.storage.list_intents(IntentFilter::default()).await
        })?;

        let mut filtered_intents = all_intents;

        // Apply status filter
        if let Some(status_filter) = &query.status_filter {
            filtered_intents.retain(|intent| status_filter.contains(&intent.status));
        }

        // Apply goal text filter
        if let Some(goal_text) = &query.goal_contains {
            let goal_text_lower = goal_text.to_lowercase();
            filtered_intents.retain(|intent| 
                intent.goal.to_lowercase().contains(&goal_text_lower)
            );
        }

        // Apply metadata filter
        if let Some(metadata_filter) = &query.metadata_filter {
            filtered_intents.retain(|intent| {
                metadata_filter.iter().all(|(key, value)| {
                    intent.metadata.get(key)
                        .map(|v| v.to_string().contains(value))
                        .unwrap_or(false)
                })
            });
        }

        // Apply date filters
        if let Some(created_after) = query.created_after {
            filtered_intents.retain(|intent| intent.created_at >= created_after);
        }
        
        if let Some(created_before) = query.created_before {
            filtered_intents.retain(|intent| intent.created_at <= created_before);
        }

        if let Some(updated_after) = query.updated_after {
            filtered_intents.retain(|intent| intent.updated_at >= updated_after);
        }

        if let Some(updated_before) = query.updated_before {
            filtered_intents.retain(|intent| intent.updated_at <= updated_before);
        }

        // Apply relationship type filter
        if let Some(relationship_types) = &query.has_relationship_types {
            filtered_intents.retain(|intent| {
                let edges = self.graph_ref.get_edges_for_intent(&intent.intent_id);
                relationship_types.iter().any(|edge_type| {
                    edges.iter().any(|edge| 
                        &edge.edge_type == edge_type && &edge.from == &intent.intent_id
                    )
                })
            });
        }

        // Apply connection filter
        if let Some(connected_to) = &query.connected_to {
            filtered_intents.retain(|intent| {
                let edges = self.graph_ref.get_edges_for_intent(&intent.intent_id);
                connected_to.iter().any(|target_id| {
                    edges.iter().any(|edge| 
                        &edge.from == target_id || &edge.to == target_id
                    )
                })
            });
        }

        // Apply semantic search if specified
        if let Some(semantic_query) = &query.semantic_query {
            // For now, do a simple text-based search
            // In a full implementation, this would use semantic embeddings
            let semantic_lower = semantic_query.to_lowercase();
            filtered_intents.retain(|intent| {
                intent.goal.to_lowercase().contains(&semantic_lower) ||
                intent.metadata.values().any(|v| v.to_string().to_lowercase().contains(&semantic_lower))
            });
        }

        let total_count = filtered_intents.len();

        // Apply sorting
        if let Some(sort_criteria) = &query.sort_by {
            match sort_criteria {
                IntentSortCriteria::CreatedDate(order) => {
                    filtered_intents.sort_by(|a, b| match order {
                        SortOrder::Ascending => a.created_at.cmp(&b.created_at),
                        SortOrder::Descending => b.created_at.cmp(&a.created_at),
                    });
                }
                IntentSortCriteria::UpdatedDate(order) => {
                    filtered_intents.sort_by(|a, b| match order {
                        SortOrder::Ascending => a.updated_at.cmp(&b.updated_at),
                        SortOrder::Descending => b.updated_at.cmp(&a.updated_at),
                    });
                }
                IntentSortCriteria::GoalAlphabetical(order) => {
                    filtered_intents.sort_by(|a, b| match order {
                        SortOrder::Ascending => a.goal.cmp(&b.goal),
                        SortOrder::Descending => b.goal.cmp(&a.goal),
                    });
                }
                IntentSortCriteria::ConnectionCount(order) => {
                    filtered_intents.sort_by(|a, b| {
                        let count_a = self.graph_ref.get_edges_for_intent(&a.intent_id).len();
                        let count_b = self.graph_ref.get_edges_for_intent(&b.intent_id).len();
                        match order {
                            SortOrder::Ascending => count_a.cmp(&count_b),
                            SortOrder::Descending => count_b.cmp(&count_a),
                        }
                    });
                }
                IntentSortCriteria::RelevanceScore(_) => {
                    // For now, keep original order
                    // In a full implementation, this would use relevance scoring
                }
            }
        }

        // Apply limit
        let truncated = if let Some(limit) = query.limit {
            if filtered_intents.len() > limit {
                filtered_intents.truncate(limit);
                true
            } else {
                false
            }
        } else {
            false
        };

        let execution_time_ms = start_time.elapsed().as_millis() as u64;

        Ok(IntentQueryResult {
            intents: filtered_intents,
            total_count,
            execution_time_ms,
            truncated,
        })
    }

    /// Execute an edge query
    pub fn query_edges(&self, query: EdgeQuery) -> Result<EdgeQueryResult, RuntimeError> {
        let start_time = std::time::Instant::now();

        // Get all edges
        let all_edges = self.graph_ref.rt.block_on(async {
            self.graph_ref.storage.get_edges().await
        })?;

        let mut filtered_edges = all_edges;

        // Apply edge type filter
        if let Some(edge_types) = &query.edge_types {
            filtered_edges.retain(|edge| edge_types.contains(&edge.edge_type));
        }

        // Apply weight filters
        if let Some(min_weight) = query.min_weight {
            filtered_edges.retain(|edge| 
                edge.weight.map_or(false, |w| w >= min_weight)
            );
        }

        if let Some(max_weight) = query.max_weight {
            filtered_edges.retain(|edge| 
                edge.weight.map_or(true, |w| w <= max_weight)
            );
        }

        // Apply metadata filter
        if let Some(metadata_filter) = &query.metadata_filter {
            filtered_edges.retain(|edge| {
                metadata_filter.iter().all(|(key, value)| {
                    edge.metadata.get(key)
                        .map(|v| v.contains(value))
                        .unwrap_or(false)
                })
            });
        }

        // Apply from_intent filter
        if let Some(from_intent) = &query.from_intent {
            filtered_edges.retain(|edge| &edge.from == from_intent);
        }

        // Apply to_intent filter
        if let Some(to_intent) = &query.to_intent {
            filtered_edges.retain(|edge| &edge.to == to_intent);
        }

        // Apply involves_intent filter
        if let Some(involves_intent) = &query.involves_intent {
            filtered_edges.retain(|edge| 
                &edge.from == involves_intent || &edge.to == involves_intent
            );
        }

        let total_count = filtered_edges.len();
        let execution_time_ms = start_time.elapsed().as_millis() as u64;

        Ok(EdgeQueryResult {
            edges: filtered_edges,
            total_count,
            execution_time_ms,
            truncated: false, // No limit applied for edges for now
        })
    }

    /// Export visualization data for the entire graph
    pub fn export_visualization_data(&self) -> Result<GraphVisualizationData, RuntimeError> {
        self.export_filtered_visualization_data(None, None)
    }

    /// Export visualization data with optional filters
    pub fn export_filtered_visualization_data(
        &self,
        intent_filter: Option<IntentQuery>,
        edge_filter: Option<EdgeQuery>,
    ) -> Result<GraphVisualizationData, RuntimeError> {
        // Get filtered intents
        let intents = if let Some(filter) = intent_filter {
            self.query_intents(filter)?.intents
        } else {
            self.graph_ref.rt.block_on(async {
                self.graph_ref.storage.list_intents(IntentFilter::default()).await
            })?
        };

        // Get filtered edges
        let edges = if let Some(filter) = edge_filter {
            self.query_edges(filter)?.edges
        } else {
            self.graph_ref.rt.block_on(async {
                self.graph_ref.storage.get_edges().await
            })?
        };

        // Convert intents to visualization nodes
        let nodes = intents
            .iter()
            .map(|intent| self.intent_to_visualization_node(intent, &edges))
            .collect();

        // Convert edges to visualization edges
        let vis_edges = edges
            .iter()
            .map(|edge| self.edge_to_visualization_edge(edge))
            .collect();

        // Generate metadata
        let metadata = self.generate_graph_metadata(&intents, &edges);

        Ok(GraphVisualizationData {
            nodes,
            edges: vis_edges,
            metadata,
        })
    }

    /// Convert an intent to a visualization node
    fn intent_to_visualization_node(&self, intent: &StorableIntent, all_edges: &[Edge]) -> VisualizationNode {
        // Calculate connection count for size
        let connection_count = all_edges
            .iter()
            .filter(|edge| edge.from == intent.intent_id || edge.to == intent.intent_id)
            .count();

        // Determine size based on connections (min 10, max 100)
        let size = 10.0 + (connection_count as f64 * 5.0).min(90.0);

        // Determine color based on status
        let color = match intent.status {
            IntentStatus::Active => "#4CAF50".to_string(),     // Green
            IntentStatus::Completed => "#2196F3".to_string(),  // Blue
            IntentStatus::Failed => "#F44336".to_string(),     // Red
            IntentStatus::Archived => "#9E9E9E".to_string(),   // Gray
            IntentStatus::Suspended => "#FF9800".to_string(),  // Orange
        };

        // Generate display label (truncate if too long)
        let label = if intent.goal.len() > 50 {
            format!("{}...", &intent.goal[..47])
        } else {
            intent.goal.clone()
        };

        // Build metadata for tooltips
        let mut metadata = HashMap::new();
        metadata.insert("goal".to_string(), intent.goal.clone());
        metadata.insert("status".to_string(), format!("{:?}", intent.status));
        metadata.insert("created_at".to_string(), intent.created_at.to_string());
        metadata.insert("updated_at".to_string(), intent.updated_at.to_string());
        metadata.insert("connections".to_string(), connection_count.to_string());

        // Add custom metadata
        for (key, value) in &intent.metadata {
            metadata.insert(format!("meta_{}", key), value.to_string());
        }

        VisualizationNode {
            id: intent.intent_id.clone(),
            label,
            node_type: "intent".to_string(),
            status: intent.status.clone(),
            size,
            color,
            metadata,
            position: None, // Let visualization engine determine layout
        }
    }

    /// Convert an edge to a visualization edge
    fn edge_to_visualization_edge(&self, edge: &Edge) -> VisualizationEdge {
        // Determine color and label based on edge type
        let (color, label) = match edge.edge_type {
            EdgeType::DependsOn => ("#FF5722".to_string(), "depends on".to_string()),
            EdgeType::IsSubgoalOf => ("#3F51B5".to_string(), "subgoal of".to_string()),
            EdgeType::ConflictsWith => ("#E91E63".to_string(), "conflicts with".to_string()),
            EdgeType::Enables => ("#4CAF50".to_string(), "enables".to_string()),
            EdgeType::RelatedTo => ("#607D8B".to_string(), "related to".to_string()),
            EdgeType::TriggeredBy => ("#9C27B0".to_string(), "triggered by".to_string()),
            EdgeType::Blocks => ("#FF9800".to_string(), "blocks".to_string()),
        };

        // Build metadata
        let mut metadata = HashMap::new();
        metadata.insert("edge_type".to_string(), format!("{:?}", edge.edge_type));
        if let Some(weight) = edge.weight {
            metadata.insert("weight".to_string(), weight.to_string());
        }

        // Add custom metadata
        for (key, value) in &edge.metadata {
            metadata.insert(format!("meta_{}", key), value.clone());
        }

        VisualizationEdge {
            from: edge.from.clone(),
            to: edge.to.clone(),
            edge_type: edge.edge_type.clone(),
            weight: edge.weight,
            label,
            color,
            metadata,
        }
    }

    /// Generate graph metadata and statistics
    fn generate_graph_metadata(&self, intents: &[StorableIntent], edges: &[Edge]) -> GraphMetadata {
        // Status distribution
        let mut status_distribution = HashMap::new();
        for intent in intents {
            *status_distribution.entry(intent.status.clone()).or_insert(0) += 1;
        }

        // Edge type distribution
        let mut edge_type_distribution = HashMap::new();
        for edge in edges {
            *edge_type_distribution.entry(edge.edge_type.clone()).or_insert(0) += 1;
        }

        // Calculate connection counts
        let mut connection_counts: HashMap<IntentId, usize> = HashMap::new();
        for edge in edges {
            *connection_counts.entry(edge.from.clone()).or_insert(0) += 1;
            *connection_counts.entry(edge.to.clone()).or_insert(0) += 1;
        }

        // Average connections per node
        let avg_connections_per_node = if intents.is_empty() {
            0.0
        } else {
            connection_counts.values().sum::<usize>() as f64 / intents.len() as f64
        };

        // Highly connected intents (top 10)
        let mut highly_connected: Vec<_> = connection_counts.into_iter().collect();
        highly_connected.sort_by(|a, b| b.1.cmp(&a.1));
        highly_connected.truncate(10);

        // Isolated intents (no connections)
        let connected_intent_ids: HashSet<_> = edges
            .iter()
            .flat_map(|edge| vec![&edge.from, &edge.to])
            .collect();
        let isolated_intents: Vec<_> = intents
            .iter()
            .filter(|intent| !connected_intent_ids.contains(&intent.intent_id))
            .map(|intent| intent.intent_id.clone())
            .collect();

        // Detect if graph is hierarchical
        let is_hierarchical = edges
            .iter()
            .any(|edge| edge.edge_type == EdgeType::IsSubgoalOf);

        // Generate clusters (simple implementation based on edge types)
        let clusters = self.generate_simple_clusters(intents, edges);

        GraphMetadata {
            node_count: intents.len(),
            edge_count: edges.len(),
            statistics: GraphStatistics {
                status_distribution,
                edge_type_distribution,
                avg_connections_per_node,
                highly_connected_intents: highly_connected,
                isolated_intents,
            },
            layout_hints: LayoutHints {
                suggested_layout: if is_hierarchical {
                    "hierarchical".to_string()
                } else if intents.len() > 100 {
                    "force-directed".to_string()
                } else {
                    "circular".to_string()
                },
                is_hierarchical,
                clusters,
            },
        }
    }

    /// Generate simple clusters based on connectivity
    fn generate_simple_clusters(&self, intents: &[StorableIntent], edges: &[Edge]) -> Vec<IntentCluster> {
        // Simple clustering by connected components
        let mut clusters = Vec::new();
        let mut visited = HashSet::new();

        for intent in intents {
            if visited.contains(&intent.intent_id) {
                continue;
            }

            let mut cluster_intents = Vec::new();
            let mut stack = vec![intent.intent_id.clone()];

            while let Some(current_id) = stack.pop() {
                if visited.contains(&current_id) {
                    continue;
                }

                visited.insert(current_id.clone());
                cluster_intents.push(current_id.clone());

                // Find connected intents
                for edge in edges {
                    if edge.from == current_id && !visited.contains(&edge.to) {
                        stack.push(edge.to.clone());
                    }
                    if edge.to == current_id && !visited.contains(&edge.from) {
                        stack.push(edge.from.clone());
                    }
                }
            }

            if !cluster_intents.is_empty() {
                // Find center intent (most connected in cluster)
                let center_intent = cluster_intents
                    .iter()
                    .max_by_key(|intent_id| {
                        edges
                            .iter()
                            .filter(|edge| &edge.from == *intent_id || &edge.to == *intent_id)
                            .count()
                    })
                    .cloned();

                clusters.push(IntentCluster {
                    cluster_id: format!("cluster_{}", clusters.len()),
                    intent_ids: cluster_intents,
                    center_intent,
                    theme: "connected_group".to_string(),
                });
            }
        }

        clusters
    }

    /// Get aggregated statistics for the graph
    pub fn get_graph_statistics(&self) -> Result<GraphStatistics, RuntimeError> {
        let intents = self.graph_ref.rt.block_on(async {
            self.graph_ref.storage.list_intents(IntentFilter::default()).await
        })?;

        let edges = self.graph_ref.rt.block_on(async {
            self.graph_ref.storage.get_edges().await
        })?;

        Ok(self.generate_graph_metadata(&intents, &edges).statistics)
    }

    /// Get layout hints for visualization
    pub fn get_layout_hints(&self) -> Result<LayoutHints, RuntimeError> {
        let intents = self.graph_ref.rt.block_on(async {
            self.graph_ref.storage.list_intents(IntentFilter::default()).await
        })?;

        let edges = self.graph_ref.rt.block_on(async {
            self.graph_ref.storage.get_edges().await
        })?;

        Ok(self.generate_graph_metadata(&intents, &edges).layout_hints)
    }

    /// Export graph data in different formats for various visualization tools
    pub fn export_graph_data(&self, format: ExportFormat) -> Result<String, RuntimeError> {
        let visualization_data = self.export_visualization_data()?;
        
        match format {
            ExportFormat::Json => {
                serde_json::to_string_pretty(&visualization_data)
                    .map_err(|e| RuntimeError::new(&format!("Serialization error: {}", e)))
            }
            ExportFormat::Graphviz => {
                self.export_as_graphviz(&visualization_data)
            }
            ExportFormat::Cytoscape => {
                self.export_as_cytoscape(&visualization_data)
            }
            ExportFormat::D3Force => {
                self.export_as_d3_force(&visualization_data)
            }
            ExportFormat::Mermaid => {
                self.export_as_mermaid(&visualization_data)
            }
        }
    }

    /// Export as Graphviz DOT format
    fn export_as_graphviz(&self, data: &GraphVisualizationData) -> Result<String, RuntimeError> {
        let mut dot = String::new();
        dot.push_str("digraph IntentGraph {\n");
        dot.push_str("  rankdir=TB;\n");
        dot.push_str("  node [shape=box, style=rounded];\n");
        
        // Add nodes
        for node in &data.nodes {
            let escaped_label = node.label.replace("\"", "\\\"");
            dot.push_str(&format!(
                "  \"{}\" [label=\"{}\", fillcolor=\"{}\", style=filled];\n",
                node.id, escaped_label, node.color
            ));
        }
        
        // Add edges
        for edge in &data.edges {
            dot.push_str(&format!(
                "  \"{}\" -> \"{}\" [label=\"{}\", color=\"{}\"];\n",
                edge.from, edge.to, edge.label, edge.color
            ));
        }
        
        dot.push_str("}\n");
        Ok(dot)
    }

    /// Export as Cytoscape.js format
    fn export_as_cytoscape(&self, data: &GraphVisualizationData) -> Result<String, RuntimeError> {
        #[derive(serde::Serialize)]
        struct CytoscapeData {
            nodes: Vec<CytoscapeNode>,
            edges: Vec<CytoscapeEdge>,
        }

        #[derive(serde::Serialize)]
        struct CytoscapeNode {
            data: CytoscapeNodeData,
        }

        #[derive(serde::Serialize)]
        struct CytoscapeNodeData {
            id: String,
            label: String,
            #[serde(rename = "background-color")]
            background_color: String,
            width: f64,
            height: f64,
        }

        #[derive(serde::Serialize)]
        struct CytoscapeEdge {
            data: CytoscapeEdgeData,
        }

        #[derive(serde::Serialize)]
        struct CytoscapeEdgeData {
            id: String,
            source: String,
            target: String,
            label: String,
            #[serde(rename = "line-color")]
            line_color: String,
        }

        let cytoscape_nodes = data.nodes
            .iter()
            .map(|node| CytoscapeNode {
                data: CytoscapeNodeData {
                    id: node.id.clone(),
                    label: node.label.clone(),
                    background_color: node.color.clone(),
                    width: node.size,
                    height: node.size,
                }
            })
            .collect();

        let cytoscape_edges = data.edges
            .iter()
            .enumerate()
            .map(|(i, edge)| CytoscapeEdge {
                data: CytoscapeEdgeData {
                    id: format!("edge_{}", i),
                    source: edge.from.clone(),
                    target: edge.to.clone(),
                    label: edge.label.clone(),
                    line_color: edge.color.clone(),
                }
            })
            .collect();

        let cytoscape_data = CytoscapeData {
            nodes: cytoscape_nodes,
            edges: cytoscape_edges,
        };

        serde_json::to_string_pretty(&cytoscape_data)
            .map_err(|e| RuntimeError::new(&format!("Serialization error: {}", e)))
    }

    /// Export as D3.js force-directed graph format
    fn export_as_d3_force(&self, data: &GraphVisualizationData) -> Result<String, RuntimeError> {
        #[derive(serde::Serialize)]
        struct D3Data {
            nodes: Vec<D3Node>,
            links: Vec<D3Link>,
        }

        #[derive(serde::Serialize)]
        struct D3Node {
            id: String,
            label: String,
            group: String,
            color: String,
            size: f64,
        }

        #[derive(serde::Serialize)]
        struct D3Link {
            source: String,
            target: String,
            value: f64,
            label: String,
            color: String,
        }

        let d3_nodes = data.nodes
            .iter()
            .map(|node| D3Node {
                id: node.id.clone(),
                label: node.label.clone(),
                group: format!("{:?}", node.status),
                color: node.color.clone(),
                size: node.size,
            })
            .collect();

        let d3_links = data.edges
            .iter()
            .map(|edge| D3Link {
                source: edge.from.clone(),
                target: edge.to.clone(),
                value: edge.weight.unwrap_or(1.0),
                label: edge.label.clone(),
                color: edge.color.clone(),
            })
            .collect();

        let d3_data = D3Data {
            nodes: d3_nodes,
            links: d3_links,
        };

        serde_json::to_string_pretty(&d3_data)
            .map_err(|e| RuntimeError::new(&format!("Serialization error: {}", e)))
    }

    /// Export as Mermaid diagram format
    fn export_as_mermaid(&self, data: &GraphVisualizationData) -> Result<String, RuntimeError> {
        let mut mermaid = String::new();
        mermaid.push_str("graph TD\n");

        // Add nodes with styling
        for node in &data.nodes {
            let safe_id = node.id.replace("-", "_");
            let escaped_label = node.label.replace("\"", "&quot;");
            mermaid.push_str(&format!("  {}[\"{}\"]\n", safe_id, escaped_label));
        }

        // Add edges
        for edge in &data.edges {
            let safe_from = edge.from.replace("-", "_");
            let safe_to = edge.to.replace("-", "_");
            let arrow = match edge.edge_type {
                EdgeType::DependsOn => "-->",
                EdgeType::IsSubgoalOf => "-.->",
                EdgeType::ConflictsWith => "-.->",
                EdgeType::Enables => "==>",
                EdgeType::RelatedTo => "---",
                EdgeType::TriggeredBy => "==>",
                EdgeType::Blocks => "-.->",
            };
            mermaid.push_str(&format!("  {} {}|{}| {}\n", safe_from, arrow, edge.label, safe_to));
        }

        Ok(mermaid)
    }

    /// Get debugging information about the graph
    pub fn get_debug_info(&self) -> Result<DebugInfo, RuntimeError> {
        let statistics = self.get_graph_statistics()?;
        let layout_hints = self.get_layout_hints()?;

        // Additional debug information
        let intents = self.graph_ref.rt.block_on(async {
            self.graph_ref.storage.list_intents(IntentFilter::default()).await
        })?;

        let edges = self.graph_ref.rt.block_on(async {
            self.graph_ref.storage.get_edges().await
        })?;

        // Find cycles
        let cycles = self.detect_cycles(&intents, &edges);

        // Find orphaned intents
        let orphaned = statistics.isolated_intents.clone();

        // Calculate health metrics
        let health_score = self.calculate_graph_health(&statistics, &cycles);

        let recommendations = self.generate_recommendations(&statistics);
        
        Ok(DebugInfo {
            statistics,
            layout_hints,
            cycles,
            orphaned_intents: orphaned,
            health_score,
            recommendations,
        })
    }

    /// Detect cycles in the graph
    fn detect_cycles(&self, intents: &[StorableIntent], edges: &[Edge]) -> Vec<Vec<IntentId>> {
        let mut cycles = Vec::new();
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();

        for intent in intents {
            if !visited.contains(&intent.intent_id) {
                if let Some(cycle) = self.dfs_cycle_detection(
                    &intent.intent_id,
                    edges,
                    &mut visited,
                    &mut rec_stack,
                    &mut Vec::new(),
                ) {
                    cycles.push(cycle);
                }
            }
        }

        cycles
    }

    /// DFS-based cycle detection
    fn dfs_cycle_detection(
        &self,
        current: &IntentId,
        edges: &[Edge],
        visited: &mut HashSet<IntentId>,
        rec_stack: &mut HashSet<IntentId>,
        path: &mut Vec<IntentId>,
    ) -> Option<Vec<IntentId>> {
        visited.insert(current.clone());
        rec_stack.insert(current.clone());
        path.push(current.clone());

        for edge in edges {
            if &edge.from == current {
                if !visited.contains(&edge.to) {
                    if let Some(cycle) = self.dfs_cycle_detection(&edge.to, edges, visited, rec_stack, path) {
                        return Some(cycle);
                    }
                } else if rec_stack.contains(&edge.to) {
                    // Found a cycle
                    let cycle_start = path.iter().position(|id| id == &edge.to).unwrap();
                    return Some(path[cycle_start..].to_vec());
                }
            }
        }

        rec_stack.remove(current);
        path.pop();
        None
    }

    /// Calculate graph health score (0.0 to 1.0)
    fn calculate_graph_health(&self, statistics: &GraphStatistics, cycles: &[Vec<IntentId>]) -> f64 {
        let mut health_score = 1.0;

        // Penalize cycles (severe penalty)
        if !cycles.is_empty() {
            health_score -= 0.4; // Significant penalty for cycles
        }

        // Penalize too many isolated nodes
        let isolation_penalty = statistics.isolated_intents.len() as f64 * 0.1;
        health_score -= isolation_penalty.min(0.3);

        // Penalize very low connectivity
        if statistics.avg_connections_per_node < 1.0 {
            health_score -= 0.2;
        }

        // Penalize too many failed intents
        let failed_count = statistics.status_distribution
            .get(&IntentStatus::Failed)
            .unwrap_or(&0);
        let total_intents = statistics.status_distribution.values().sum::<usize>();
        if total_intents > 0 {
            let failure_rate = *failed_count as f64 / total_intents as f64;
            health_score -= failure_rate * 0.3;
        }

        health_score.max(0.0).min(1.0)
    }

    /// Generate recommendations for graph improvement
    fn generate_recommendations(&self, statistics: &GraphStatistics) -> Vec<String> {
        let mut recommendations = Vec::new();

        if !statistics.isolated_intents.is_empty() {
            recommendations.push(format!(
                "Consider connecting {} isolated intents to the main graph",
                statistics.isolated_intents.len()
            ));
        }

        if statistics.avg_connections_per_node < 1.0 {
            recommendations.push(
                "Graph connectivity is low. Consider adding more relationships between intents".to_string()
            );
        }

        let failed_count = statistics.status_distribution
            .get(&IntentStatus::Failed)
            .unwrap_or(&0);
        if *failed_count > 0 {
            recommendations.push(format!(
                "Review {} failed intents and consider archiving or reactivating them",
                failed_count
            ));
        }

        if statistics.highly_connected_intents.len() < 3 {
            recommendations.push(
                "Consider identifying key intents that could serve as hubs in the graph".to_string()
            );
        }

        recommendations
    }

    /// Quick search for intents by text
    pub fn quick_search(&self, query: &str, limit: Option<usize>) -> Result<Vec<StorableIntent>, RuntimeError> {
        let intent_query = IntentQuery {
            goal_contains: Some(query.to_string()),
            limit,
            sort_by: Some(IntentSortCriteria::RelevanceScore(SortOrder::Descending)),
            ..Default::default()
        };

        Ok(self.query_intents(intent_query)?.intents)
    }

    /// Get related intents for a given intent ID
    pub fn get_related_intents(&self, intent_id: &IntentId, max_depth: usize) -> Result<Vec<StorableIntent>, RuntimeError> {
        let mut related = HashSet::new();
        let mut to_explore = vec![(intent_id.clone(), 0)];
        let mut explored = HashSet::new();

        while let Some((current_id, depth)) = to_explore.pop() {
            if depth >= max_depth || explored.contains(&current_id) {
                continue;
            }

            explored.insert(current_id.clone());

            // Get all edges involving this intent
            let edges = self.graph_ref.get_edges_for_intent(&current_id);
            
            for edge in edges {
                let related_id = if edge.from == current_id {
                    &edge.to
                } else {
                    &edge.from
                };

                if !explored.contains(related_id) {
                    related.insert(related_id.clone());
                    to_explore.push((related_id.clone(), depth + 1));
                }
            }
        }

        // Convert to intents
        let mut result_intents = Vec::new();
        for intent_id in related {
            if let Some(intent) = self.graph_ref.get_intent(&intent_id) {
                result_intents.push(intent);
            }
        }

        Ok(result_intents)
    }
}

/// Export format enumeration
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ExportFormat {
    Json,
    Graphviz,
    Cytoscape,
    D3Force,
    Mermaid,
}

/// Debug information structure
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DebugInfo {
    pub statistics: GraphStatistics,
    pub layout_hints: LayoutHints,
    pub cycles: Vec<Vec<IntentId>>,
    pub orphaned_intents: Vec<IntentId>,
    pub health_score: f64,
    pub recommendations: Vec<String>,
}

impl Default for IntentQuery {
    fn default() -> Self {
        Self {
            status_filter: None,
            goal_contains: None,
            metadata_filter: None,
            created_after: None,
            created_before: None,
            updated_after: None,
            updated_before: None,
            has_relationship_types: None,
            connected_to: None,
            semantic_query: None,
            limit: None,
            sort_by: None,
        }
    }
}

impl Default for EdgeQuery {
    fn default() -> Self {
        Self {
            edge_types: None,
            min_weight: None,
            max_weight: None,
            metadata_filter: None,
            from_intent: None,
            to_intent: None,
            involves_intent: None,
        }
    }
}
