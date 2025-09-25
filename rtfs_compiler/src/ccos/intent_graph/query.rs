use std::collections::{HashMap, HashSet};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use super::{storage::Edge, IntentGraph};
use crate::ccos::intent_storage::IntentFilter;
use crate::ccos::types::{EdgeType, IntentId, IntentStatus, StorableIntent};
use crate::runtime::RuntimeError;

#[derive(Debug)]
pub struct IntentGraphQueryAPI {
    // Store reference to graph instead of owning it
    graph_ref: std::sync::Arc<IntentGraph>,
}

/// Comprehensive query structure for advanced Intent Graph queries
#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IntentSortCriteria {
    CreatedDate(SortOrder),
    UpdatedDate(SortOrder),
    GoalAlphabetical(SortOrder),
    ConnectionCount(SortOrder),
    RelevanceScore(SortOrder),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SortOrder {
    Ascending,
    Descending,
}

/// Edge/Relationship query structure
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphVisualizationData {
    /// All nodes (intents) in the graph
    pub nodes: Vec<VisualizationNode>,
    /// All edges (relationships) in the graph
    pub edges: Vec<VisualizationEdge>,
    /// Metadata about the graph
    pub metadata: GraphMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualizationPosition {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutHints {
    /// Suggested layout algorithm
    pub suggested_layout: String,
    /// Whether graph has hierarchical structure
    pub is_hierarchical: bool,
    /// Suggested clustering
    pub clusters: Vec<IntentCluster>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
        Self {
            graph_ref: std::sync::Arc::new(graph),
        }
    }

    /// Execute an advanced intent query
    pub fn query_intents(&self, query: IntentQuery) -> Result<IntentQueryResult, RuntimeError> {
        let start_time = Instant::now();

        // Start with all intents
        let all_intents = self.graph_ref.rt.block_on(async {
            self.graph_ref
                .storage
                .list_intents(IntentFilter::default())
                .await
        })?;

        let mut filtered_intents = all_intents;

        // Apply status filter
        if let Some(status_filter) = &query.status_filter {
            filtered_intents.retain(|intent| status_filter.contains(&intent.status));
        }

        // Apply goal text filter
        if let Some(goal_text) = &query.goal_contains {
            let goal_text_lower = goal_text.to_lowercase();
            filtered_intents.retain(|intent| intent.goal.to_lowercase().contains(&goal_text_lower));
        }

        // Apply metadata filter
        if let Some(metadata_filter) = &query.metadata_filter {
            filtered_intents.retain(|intent| {
                metadata_filter.iter().all(|(key, value)| {
                    intent
                        .metadata
                        .get(key)
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
                    edges
                        .iter()
                        .any(|edge| &edge.edge_type == edge_type && &edge.from == &intent.intent_id)
                })
            });
        }

        // Apply connection filter
        if let Some(connected_to) = &query.connected_to {
            filtered_intents.retain(|intent| {
                let edges = self.graph_ref.get_edges_for_intent(&intent.intent_id);
                connected_to.iter().any(|target_id| {
                    edges
                        .iter()
                        .any(|edge| &edge.from == target_id || &edge.to == target_id)
                })
            });
        }

        // Apply semantic search if specified
        if let Some(semantic_query) = &query.semantic_query {
            // For now, do a simple text-based search
            // In a full implementation, this would use semantic embeddings
            let semantic_lower = semantic_query.to_lowercase();
            filtered_intents.retain(|intent| {
                intent.goal.to_lowercase().contains(&semantic_lower)
                    || intent
                        .metadata
                        .values()
                        .any(|v| v.to_string().to_lowercase().contains(&semantic_lower))
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
        let start_time = Instant::now();

        // Get all edges
        let all_edges = self
            .graph_ref
            .rt
            .block_on(async { self.graph_ref.storage.get_edges().await })?;

        let mut filtered_edges = all_edges;

        // Apply edge type filter
        if let Some(edge_types) = &query.edge_types {
            filtered_edges.retain(|edge| edge_types.contains(&edge.edge_type));
        }

        // Apply weight filters
        if let Some(min_weight) = query.min_weight {
            filtered_edges.retain(|edge| edge.weight.map_or(false, |w| w >= min_weight));
        }

        if let Some(max_weight) = query.max_weight {
            filtered_edges.retain(|edge| edge.weight.map_or(true, |w| w <= max_weight));
        }

        // Apply metadata filter
        if let Some(metadata_filter) = &query.metadata_filter {
            filtered_edges.retain(|edge| {
                metadata_filter.iter().all(|(key, value)| {
                    if let Some(metadata) = &edge.metadata {
                        metadata
                            .get(key)
                            .map(|v| v.contains(value))
                            .unwrap_or(false)
                    } else {
                        false
                    }
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
            filtered_edges
                .retain(|edge| &edge.from == involves_intent || &edge.to == involves_intent);
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
                self.graph_ref
                    .storage
                    .list_intents(IntentFilter::default())
                    .await
            })?
        };

        // Get filtered edges
        let edges = if let Some(filter) = edge_filter {
            self.query_edges(filter)?.edges
        } else {
            self.graph_ref
                .rt
                .block_on(async { self.graph_ref.storage.get_edges().await })?
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
    fn intent_to_visualization_node(
        &self,
        intent: &StorableIntent,
        all_edges: &[Edge],
    ) -> VisualizationNode {
        // Calculate connection count for size
        let connection_count = all_edges
            .iter()
            .filter(|edge| edge.from == intent.intent_id || edge.to == intent.intent_id)
            .count();

        // Determine size based on connections (min 10, max 100)
        let size = 10.0 + (connection_count as f64 * 5.0).min(90.0);

        // Determine color based on status
        let color = match intent.status {
            IntentStatus::Executing => "#FFC107".to_string(), // Amber for in-flight
            IntentStatus::Active => "#4CAF50".to_string(),    // Green
            IntentStatus::Completed => "#2196F3".to_string(), // Blue
            IntentStatus::Failed => "#F44336".to_string(),    // Red
            IntentStatus::Archived => "#9E9E9E".to_string(),  // Gray
            IntentStatus::Suspended => "#FF9800".to_string(), // Orange
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
        if let Some(edge_metadata) = &edge.metadata {
            for (key, value) in edge_metadata {
                metadata.insert(format!("meta_{}", key), value.clone());
            }
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
            *status_distribution
                .entry(intent.status.clone())
                .or_insert(0) += 1;
        }

        // Edge type distribution
        let mut edge_type_distribution = HashMap::new();
        for edge in edges {
            *edge_type_distribution
                .entry(edge.edge_type.clone())
                .or_insert(0) += 1;
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
    fn generate_simple_clusters(
        &self,
        intents: &[StorableIntent],
        edges: &[Edge],
    ) -> Vec<IntentCluster> {
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

                // Generate cluster theme based on common keywords in goals
                let cluster_goals: Vec<_> = cluster_intents
                    .iter()
                    .filter_map(|id| intents.iter().find(|i| &i.intent_id == id).map(|i| &i.goal))
                    .collect();

                let theme = if cluster_goals.is_empty() {
                    "Unknown".to_string()
                } else {
                    // Simple theme extraction - take first word of first goal
                    cluster_goals[0]
                        .split_whitespace()
                        .next()
                        .unwrap_or("Cluster")
                        .to_string()
                };

                let cluster_id = format!("cluster_{}", clusters.len());

                clusters.push(IntentCluster {
                    cluster_id,
                    intent_ids: cluster_intents,
                    center_intent,
                    theme,
                });
            }
        }

        clusters
    }

    /// Quick search for intents by text
    pub fn quick_search(
        &self,
        query: &str,
        limit: Option<usize>,
    ) -> Result<Vec<StorableIntent>, RuntimeError> {
        let intent_query = IntentQuery {
            goal_contains: Some(query.to_string()),
            limit,
            sort_by: Some(IntentSortCriteria::RelevanceScore(SortOrder::Descending)),
            ..Default::default()
        };

        Ok(self.query_intents(intent_query)?.intents)
    }

    /// Get related intents for a given intent ID
    pub fn get_related_intents(
        &self,
        intent_id: &IntentId,
        max_depth: usize,
    ) -> Result<Vec<StorableIntent>, RuntimeError> {
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
