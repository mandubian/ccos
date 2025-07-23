//! Intent Graph Implementation
//!
//! This module implements the Living Intent Graph - a dynamic, multi-layered data structure
//! that stores and manages user intents with their relationships and lifecycle.

use super::types::{EdgeType, ExecutionResult, Intent, IntentId, IntentStatus};
use crate::runtime::error::RuntimeError;
use crate::runtime::values::Value;
use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

/// Storage backend for the Intent Graph
/// In a full implementation, this would use vector and graph databases
pub struct IntentGraphStorage {
    // In-memory storage for now - would be replaced with proper databases
    intents: HashMap<IntentId, Intent>,
    edges: Vec<Edge>,
    metadata: HashMap<IntentId, IntentMetadata>,
}

impl IntentGraphStorage {
    pub fn new() -> Self {
        Self {
            intents: HashMap::new(),
            edges: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    pub fn store_intent(&mut self, intent: Intent) -> Result<(), RuntimeError> {
        let intent_id = intent.intent_id.clone();
        let metadata = IntentMetadata::new(&intent);
        self.intents.insert(intent_id.clone(), intent);
        self.metadata.insert(intent_id, metadata);
        Ok(())
    }

    pub fn get_intent(&self, intent_id: &IntentId) -> Option<&Intent> {
        self.intents.get(intent_id)
    }

    pub fn get_intent_mut(&mut self, intent_id: &IntentId) -> Option<&mut Intent> {
        self.intents.get_mut(intent_id)
    }

    pub fn store_edge(&mut self, edge: Edge) -> Result<(), RuntimeError> {
        self.edges.push(edge);
        Ok(())
    }

    pub fn get_edges(&self) -> &[Edge] {
        &self.edges
    }

    pub fn get_related_intents(&self, intent_id: &IntentId) -> Vec<&Intent> {
        let mut related = Vec::new();

        for edge in &self.edges {
            if edge.from == *intent_id {
                if let Some(intent) = self.intents.get(&edge.to) {
                    related.push(intent);
                }
            } else if edge.to == *intent_id {
                if let Some(intent) = self.intents.get(&edge.from) {
                    related.push(intent);
                }
            }
        }

        related
    }

    pub fn get_dependent_intents(&self, intent_id: &IntentId) -> Vec<&Intent> {
        let mut dependent = Vec::new();

        for edge in &self.edges {
            if edge.to == *intent_id && edge.edge_type == EdgeType::DependsOn {
                if let Some(intent) = self.intents.get(&edge.from) {
                    dependent.push(intent);
                }
            }
        }

        dependent
    }

    pub fn get_subgoals(&self, intent_id: &IntentId) -> Vec<&Intent> {
        let mut subgoals = Vec::new();

        for edge in &self.edges {
            if edge.from == *intent_id && edge.edge_type == EdgeType::IsSubgoalOf {
                if let Some(intent) = self.intents.get(&edge.to) {
                    subgoals.push(intent);
                }
            }
        }

        subgoals
    }

    pub fn get_conflicting_intents(&self, intent_id: &IntentId) -> Vec<&Intent> {
        let mut conflicting = Vec::new();

        for edge in &self.edges {
            if (edge.from == *intent_id || edge.to == *intent_id)
                && edge.edge_type == EdgeType::ConflictsWith
            {
                let other_id = if edge.from == *intent_id {
                    &edge.to
                } else {
                    &edge.from
                };
                if let Some(intent) = self.intents.get(other_id) {
                    conflicting.push(intent);
                }
            }
        }

        conflicting
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
    pub fn new(intent: &Intent) -> Self {
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

    fn calculate_complexity(intent: &Intent) -> f64 {
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

        for (intent_id, intent) in &storage.intents {
            if intent.goal.to_lowercase().contains(&query.to_lowercase()) {
                relevant.push(intent_id.clone());
            }
        }

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
    ) -> Vec<Intent> {
        let mut context_intents = Vec::new();
        let mut loaded_ids = HashSet::new();

        // Load primary intents
        for intent_id in intent_ids {
            if let Some(intent) = storage.get_intent(intent_id) {
                context_intents.push(intent.clone());
                loaded_ids.insert(intent_id.clone());
            }
        }

        // Load related intents (parents, dependencies, etc.)
        for intent_id in intent_ids {
            if let Some(intent) = storage.get_intent(intent_id) {
                // parent_intent field does not exist; skip or use metadata if needed

                // Load dependent intents
                for dependent in storage.get_dependent_intents(intent_id) {
                    if !loaded_ids.contains(&dependent.intent_id) {
                        context_intents.push(dependent.clone());
                        loaded_ids.insert(dependent.intent_id.clone());
                    }
                }
            }
        }

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

    pub fn estimate_tokens(&self, intents: &[Intent]) -> usize {
        let mut total_tokens = 0;

        for intent in intents {
            // Rough token estimation
            total_tokens += intent.goal.len() / 4; // ~4 chars per token
            total_tokens += intent.constraints.len() * 10; // ~10 tokens per constraint
            total_tokens += intent.preferences.len() * 8; // ~8 tokens per preference
        }

        total_tokens
    }

    pub fn should_truncate(&self, intents: &[Intent]) -> bool {
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
    pub fn archive_completed_intents(
        &self,
        storage: &mut IntentGraphStorage,
    ) -> Result<(), RuntimeError> {
        let completed_ids: Vec<IntentId> = storage
            .intents
            .iter()
            .filter(|(_, intent)| intent.status == IntentStatus::Completed)
            .map(|(id, _)| id.clone())
            .collect();

        for intent_id in completed_ids {
            if let Some(intent) = storage.get_intent_mut(&intent_id) {
                intent.status = IntentStatus::Archived;
                intent.updated_at = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
            }
        }

        Ok(())
    }

    pub fn infer_edges(&self, storage: &mut IntentGraphStorage) -> Result<(), RuntimeError> {
        // Simple edge inference based on goal similarity
        // In a full implementation, this would use more sophisticated NLP

        let intent_ids: Vec<IntentId> = storage.intents.keys().cloned().collect();

        for i in 0..intent_ids.len() {
            for j in (i + 1)..intent_ids.len() {
                let intent_a = storage.get_intent(&intent_ids[i]).unwrap();
                let intent_b = storage.get_intent(&intent_ids[j]).unwrap();

                // Check for potential conflicts based on resource constraints
                if self.detect_resource_conflict(intent_a, intent_b) {
                    let edge = Edge::new(
                        intent_a.intent_id.clone(),
                        intent_b.intent_id.clone(),
                        EdgeType::ConflictsWith,
                    );
                    storage.store_edge(edge)?;
                }
            }
        }

        Ok(())
    }

    fn detect_resource_conflict(&self, intent_a: &Intent, intent_b: &Intent) -> bool {
        // Simple conflict detection based on cost constraints
        let cost_a = intent_a
            .constraints
            .get("max_cost")
            .and_then(|v| v.as_number())
            .unwrap_or(f64::INFINITY);
        let cost_b = intent_b
            .constraints
            .get("max_cost")
            .and_then(|v| v.as_number())
            .unwrap_or(f64::INFINITY);

        // If both have very low cost constraints, they might conflict
        cost_a < 10.0 && cost_b < 10.0
    }
}

/// Main Intent Graph implementation
pub struct IntentGraph {
    storage: IntentGraphStorage,
    virtualization: IntentGraphVirtualization,
    lifecycle: IntentLifecycleManager,
}

impl IntentGraph {
    pub fn new() -> Result<Self, RuntimeError> {
        Ok(Self {
            storage: IntentGraphStorage::new(),
            virtualization: IntentGraphVirtualization::new(),
            lifecycle: IntentLifecycleManager,
        })
    }

    /// Store a new intent in the graph
    pub fn store_intent(&mut self, intent: Intent) -> Result<(), RuntimeError> {
        self.storage.store_intent(intent)?;
        self.lifecycle.infer_edges(&mut self.storage)?;
        Ok(())
    }

    /// Get an intent by ID
    pub fn get_intent(&self, intent_id: &IntentId) -> Option<&Intent> {
        self.storage.get_intent(intent_id)
    }

    /// Update an intent with execution results
    pub fn update_intent(
        &mut self,
        intent: Intent,
        result: &ExecutionResult,
    ) -> Result<(), RuntimeError> {
        let intent_id = intent.intent_id.clone();
        let updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if let Some(existing_intent) = self.storage.get_intent_mut(&intent_id) {
            existing_intent.updated_at = updated_at;

            // Update status based on result
            existing_intent.status = if result.success {
                IntentStatus::Completed
            } else {
                IntentStatus::Failed
            };
        }

        // Update metadata separately to avoid double mutable borrow
        if let Some(metadata) = self.storage.metadata.get_mut(&intent_id) {
            metadata.last_accessed = updated_at;
            metadata.access_count += 1;
        }

        Ok(())
    }

    /// Find relevant intents for a query
    pub fn find_relevant_intents(&self, query: &str) -> Vec<Intent> {
        let relevant_ids = self
            .virtualization
            .find_relevant_intents(query, &self.storage);
        let mut relevant_intents = Vec::new();

        for intent_id in relevant_ids {
            if let Some(intent) = self.storage.get_intent(&intent_id) {
                relevant_intents.push(intent.clone());
            }
        }

        relevant_intents
    }

    /// Load context window for a set of intent IDs
    pub fn load_context_window(&self, intent_ids: &[IntentId]) -> Vec<Intent> {
        self.virtualization
            .load_context_window(intent_ids, &self.storage)
    }

    /// Get related intents for a given intent
    pub fn get_related_intents(&self, intent_id: &IntentId) -> Vec<Intent> {
        self.storage
            .get_related_intents(intent_id)
            .into_iter()
            .cloned()
            .collect()
    }

    /// Get dependent intents for a given intent
    pub fn get_dependent_intents(&self, intent_id: &IntentId) -> Vec<Intent> {
        self.storage
            .get_dependent_intents(intent_id)
            .into_iter()
            .cloned()
            .collect()
    }

    /// Get subgoals for a given intent
    pub fn get_subgoals(&self, intent_id: &IntentId) -> Vec<Intent> {
        self.storage
            .get_subgoals(intent_id)
            .into_iter()
            .cloned()
            .collect()
    }

    /// Get conflicting intents for a given intent
    pub fn get_conflicting_intents(&self, intent_id: &IntentId) -> Vec<Intent> {
        self.storage
            .get_conflicting_intents(intent_id)
            .into_iter()
            .cloned()
            .collect()
    }

    /// Archive completed intents
    pub fn archive_completed_intents(&mut self) -> Result<(), RuntimeError> {
        self.lifecycle.archive_completed_intents(&mut self.storage)
    }

    /// Get all active intents
    pub fn get_active_intents(&self) -> Vec<Intent> {
        self.storage
            .intents
            .values()
            .filter(|intent| intent.status == IntentStatus::Active)
            .cloned()
            .collect()
    }

    /// Get intent count by status
    pub fn get_intent_count_by_status(&self) -> HashMap<IntentStatus, usize> {
        let mut counts = HashMap::new();

        for intent in self.storage.intents.values() {
            *counts.entry(intent.status.clone()).or_insert(0) += 1;
        }

        counts
    }

    /// Create an edge between two intents
    pub fn create_edge(
        &mut self,
        from_intent: IntentId,
        to_intent: IntentId,
        edge_type: EdgeType,
    ) -> Result<(), RuntimeError> {
        let edge = Edge::new(from_intent, to_intent, edge_type);
        self.storage.store_edge(edge)?;
        Ok(())
    }
}

// Minimal Edge struct to resolve missing type errors
#[derive(Clone, Debug)]
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

    #[test]
    fn test_intent_graph_creation() {
        let graph = IntentGraph::new();
        assert!(graph.is_ok());
    }

    #[test]
    fn test_store_and_retrieve_intent() {
        let mut graph = IntentGraph::new().unwrap();
        let intent = Intent::new("Test goal".to_string());
        let intent_id = intent.intent_id.clone();

        assert!(graph.store_intent(intent).is_ok());
        assert!(graph.get_intent(&intent_id).is_some());
    }

    #[test]
    fn test_find_relevant_intents() {
        let mut graph = IntentGraph::new().unwrap();

        let intent1 = Intent::new("Analyze sales data".to_string());
        let intent2 = Intent::new("Generate report".to_string());
        let intent3 = Intent::new("Send email".to_string());

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
        let intent = Intent::new("Test goal".to_string());
        let intent_id = intent.intent_id.clone();

        graph.store_intent(intent).unwrap();

        // Initially active
        assert_eq!(
            graph.get_intent(&intent_id).unwrap().status,
            IntentStatus::Active
        );

        // Update with successful result
        let result = ExecutionResult {
            success: true,
            value: Value::Nil,
            metadata: HashMap::new(),
        };

        // Create update intent with the same ID
        let mut update_intent = Intent::new("Test goal".to_string());
        update_intent.intent_id = intent_id.clone();
        graph.update_intent(update_intent, &result).unwrap();

        // Should be completed
        assert_eq!(
            graph.get_intent(&intent_id).unwrap().status,
            IntentStatus::Completed
        );
    }
}
