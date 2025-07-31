//! Virtualization layer for Intent Graph

use super::super::types::{EdgeType, StorableIntent, IntentId, IntentStatus};
use super::storage::IntentGraphStorage;
use super::search::{SemanticSearchEngine, GraphTraversalEngine};
use super::processing::{IntentSummarizer, IntentPruningEngine};
use crate::runtime::error::RuntimeError;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Serialize, Deserialize};

/// Virtualization layer for context horizon management and large graph optimization
#[derive(Debug)]
pub struct IntentGraphVirtualization {
    context_manager: ContextWindowManager,
    semantic_search: SemanticSearchEngine,
    graph_traversal: GraphTraversalEngine,
    summarizer: IntentSummarizer,
    pruning_engine: IntentPruningEngine,
}

impl IntentGraphVirtualization {
    pub fn new() -> Self {
        Self {
            context_manager: ContextWindowManager::new(),
            semantic_search: SemanticSearchEngine::new(),
            graph_traversal: GraphTraversalEngine::new(),
            summarizer: IntentSummarizer::new(1000), // Max 1000 chars per summary
            pruning_engine: IntentPruningEngine::new(0.3, 30), // 30% importance threshold, 30 days age
        }
    }

    /// Find relevant intents using enhanced semantic search
    pub fn find_relevant_intents(
        &self,
        query: &str,
        storage: &IntentGraphStorage,
        limit: Option<usize>,
    ) -> Result<Vec<IntentId>, RuntimeError> {
        self.semantic_search.search_intents(query, storage, limit)
    }

    /// Generate a virtualized view of the graph for context windows
    pub async fn create_virtualized_view(
        &self,
        focal_intents: &[IntentId],
        storage: &IntentGraphStorage,
        config: &VirtualizationConfig,
    ) -> Result<VirtualizedIntentGraph, RuntimeError> {
        let mut virtual_graph = VirtualizedIntentGraph::new();
        
        // Step 1: Collect relevant intents within specified radius
        let relevant_intents = self.graph_traversal.collect_neighborhood(
            focal_intents, 
            storage, 
            config.traversal_depth
        )?;
        
        // Step 2: Apply pruning if needed to respect max_intents limit
        let pruned_intents = if relevant_intents.len() > config.max_intents {
            self.pruning_engine.prune_intents(&relevant_intents, storage, config)?
        } else {
            // Still respect max_intents even if we don't need sophisticated pruning
            relevant_intents.into_iter().take(config.max_intents).collect()
        };
        
        // Step 3: Create summaries for clusters if requested
        if config.enable_summarization {
            let clusters = self.graph_traversal.identify_clusters(&pruned_intents, storage)?;
            for cluster in clusters {
                if cluster.len() > config.summarization_threshold {
                    let summary_text = self.summarizer.create_cluster_summary(&cluster, storage).await?;
                    let summary = IntentSummary {
                        summary_id: format!("cluster_{}", virtual_graph.summaries.len()),
                        description: summary_text,
                        key_goals: Vec::new(), // TODO: extract key goals from cluster
                        dominant_status: IntentStatus::Active, // TODO: compute dominant status
                        intent_ids: cluster.clone(),
                        cluster_size: cluster.len(),
                        relevance_score: 0.8, // TODO: compute relevance
                        created_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
                    };
                    virtual_graph.add_summary(summary);
                } else {
                    // Keep individual intents for small clusters
                    for intent_id in cluster {
                        if let Some(intent) = storage.get_intent_sync(&intent_id) {
                            virtual_graph.add_intent(intent);
                        }
                    }
                }
            }
        } else {
            // Add all pruned intents individually
            for intent_id in pruned_intents {
                if let Some(intent) = storage.get_intent_sync(&intent_id) {
                    virtual_graph.add_intent(intent);
                }
            }
        }
        
        // Step 4: Include edges between included intents/summaries
        virtual_graph.compute_virtual_edges(storage)?;
        
        Ok(virtual_graph)
    }

    /// Search for intents using semantic search
    pub fn search_intents(
        &self,
        query: &str,
        storage: &IntentGraphStorage,
        limit: usize,
    ) -> Result<Vec<IntentId>, RuntimeError> {
        self.semantic_search.search_intents(query, storage, Some(limit))
    }

    /// Find similar intents
    pub fn find_similar_intents(
        &self,
        target_intent: &StorableIntent,
        storage: &IntentGraphStorage,
        limit: usize,
    ) -> Result<Vec<IntentId>, RuntimeError> {
        self.semantic_search.find_similar_intents(target_intent, storage, limit)
    }

    /// Load optimized context window with virtualization
    pub async fn load_context_window(
        &self,
        intent_ids: &[IntentId],
        storage: &IntentGraphStorage,
        config: &VirtualizationConfig,
    ) -> Result<Vec<StorableIntent>, RuntimeError> {
        let virtual_graph = self.create_virtualized_view(intent_ids, storage, config).await?;
        Ok(virtual_graph.to_intent_list())
    }

    /// Search and retrieve intents with virtualization
    pub async fn search_with_virtualization(
        &self,
        query: &str,
        storage: &IntentGraphStorage,
        config: &VirtualizationConfig,
    ) -> Result<VirtualizedSearchResult, RuntimeError> {
        // Perform semantic search
        let search_results = self.semantic_search.search_intents(query, storage, Some(config.max_search_results))?;
        
        // Create virtualized view around search results
        let virtual_graph = self.create_virtualized_view(&search_results, storage, config).await?;
        
        Ok(VirtualizedSearchResult {
            query: query.to_string(),
            virtual_graph,
            total_matches: search_results.len(),
            execution_time_ms: 0, // TODO: Add timing
        })
    }
}

/// Configuration for intent graph virtualization
#[derive(Debug, Clone)]
pub struct VirtualizationConfig {
    /// Maximum number of intents to include in virtual view
    pub max_intents: usize,
    /// Maximum traversal depth from focal points
    pub traversal_depth: usize,
    /// Enable intent summarization for large clusters
    pub enable_summarization: bool,
    /// Minimum cluster size before summarization
    pub summarization_threshold: usize,
    /// Maximum search results to return
    pub max_search_results: usize,
    /// Token budget for context window
    pub max_tokens: usize,
    /// Relevance score threshold for pruning
    pub relevance_threshold: f64,
    /// Priority weights for different intent statuses
    pub status_weights: HashMap<IntentStatus, f64>,
}

impl Default for VirtualizationConfig {
    fn default() -> Self {
        let mut status_weights = HashMap::new();
        status_weights.insert(IntentStatus::Active, 1.0);
        status_weights.insert(IntentStatus::Completed, 0.3);
        status_weights.insert(IntentStatus::Failed, 0.5);
        status_weights.insert(IntentStatus::Suspended, 0.2);
        status_weights.insert(IntentStatus::Archived, 0.1);

        Self {
            max_intents: 100,
            traversal_depth: 2,
            enable_summarization: true,
            summarization_threshold: 5,
            max_search_results: 50,
            max_tokens: 8000,
            relevance_threshold: 0.3,
            status_weights,
        }
    }
}

/// Virtualized representation of an intent subgraph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualizedIntentGraph {
    /// Individual intents in the virtual view
    pub intents: Vec<StorableIntent>,
    /// Summarized intent clusters
    pub summaries: Vec<IntentSummary>,
    /// Edges between intents and summaries
    pub virtual_edges: Vec<VirtualEdge>,
    /// Metadata about the virtualization
    pub metadata: VirtualizationMetadata,
}

impl VirtualizedIntentGraph {
    pub fn new() -> Self {
        Self {
            intents: Vec::new(),
            summaries: Vec::new(),
            virtual_edges: Vec::new(),
            metadata: VirtualizationMetadata::default(),
        }
    }

    pub fn add_intent(&mut self, intent: StorableIntent) {
        self.intents.push(intent);
    }

    pub fn add_summary(&mut self, summary: IntentSummary) {
        self.summaries.push(summary);
    }

    pub fn compute_virtual_edges(&mut self, storage: &IntentGraphStorage) -> Result<(), RuntimeError> {
        // Compute edges between virtual entities (intents and summaries)
        self.virtual_edges.clear();
        
        // Add edges between intents
        for i in 0..self.intents.len() {
            for j in (i + 1)..self.intents.len() {
                let intent_a = &self.intents[i];
                let intent_b = &self.intents[j];
                
                // Check if there's an edge in the original graph
                if storage.has_edge_sync(&intent_a.intent_id, &intent_b.intent_id) {
                    self.virtual_edges.push(VirtualEdge {
                        from: VirtualNodeId::Intent(intent_a.intent_id.clone()),
                        to: VirtualNodeId::Intent(intent_b.intent_id.clone()),
                        edge_type: EdgeType::DependsOn, // Simplified
                        weight: 1.0,
                    });
                }
            }
        }
        
        // Add edges from intents to summaries
        for intent in &self.intents {
            for summary in &self.summaries {
                if summary.contains_intent(&intent.intent_id) {
                    self.virtual_edges.push(VirtualEdge {
                        from: VirtualNodeId::Intent(intent.intent_id.clone()),
                        to: VirtualNodeId::Summary(summary.summary_id.clone()),
                        edge_type: EdgeType::RelatedTo,
                        weight: 0.8,
                    });
                }
            }
        }
        
        Ok(())
    }

    pub fn to_intent_list(&self) -> Vec<StorableIntent> {
        let mut result = self.intents.clone();
        
        // Convert summaries to synthetic intents for compatibility
        for summary in &self.summaries {
            result.push(summary.to_synthetic_intent());
        }
        
        result
    }

    pub fn total_node_count(&self) -> usize {
        self.intents.len() + self.summaries.len()
    }

    // ...existing code...
}

/// Summary of a cluster of related intents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentSummary {
    pub summary_id: String,
    pub description: String,
    pub key_goals: Vec<String>,
    pub dominant_status: IntentStatus,
    pub intent_ids: Vec<IntentId>,
    pub cluster_size: usize,
    pub relevance_score: f64,
    pub created_at: u64,
}

impl IntentSummary {
    pub fn contains_intent(&self, intent_id: &IntentId) -> bool {
        self.intent_ids.contains(intent_id)
    }

    pub fn to_synthetic_intent(&self) -> StorableIntent {
        let mut synthetic = StorableIntent::new(self.description.clone());
        synthetic.intent_id = self.summary_id.clone();
        synthetic.status = self.dominant_status.clone();
        synthetic.created_at = self.created_at;
        synthetic
    }
}

/// Virtual edge connecting intents and/or summaries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualEdge {
    pub from: VirtualNodeId,
    pub to: VirtualNodeId,
    pub edge_type: EdgeType,
    pub weight: f64,
}

/// Identifier for virtual graph nodes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VirtualNodeId {
    Intent(IntentId),
    Summary(String),
}

/// Metadata about virtualization process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualizationMetadata {
    pub original_intent_count: usize,
    pub virtualized_intent_count: usize,
    pub summary_count: usize,
    pub compression_ratio: f64,
    pub created_at: u64,
}

impl Default for VirtualizationMetadata {
    fn default() -> Self {
        Self {
            original_intent_count: 0,
            virtualized_intent_count: 0,
            summary_count: 0,
            compression_ratio: 1.0,
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }
}

/// Result of virtualized search operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualizedSearchResult {
    pub query: String,
    pub virtual_graph: VirtualizedIntentGraph,
    pub total_matches: usize,
    pub execution_time_ms: u64,
}

/// Statistics about intent graph virtualization performance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualizationStats {
    pub total_intents: usize,
    pub status_distribution: HashMap<IntentStatus, usize>,
    pub avg_connectivity: f64,
    pub isolated_intents: usize,
    pub highly_connected_intents: usize,
    pub memory_usage_estimate: usize,
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
