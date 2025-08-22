//! Search and traversal components for Intent Graph

use super::super::types::{StorableIntent, IntentId, IntentStatus};
use super::storage::IntentGraphStorage;
use crate::runtime::error::RuntimeError;
use std::collections::{HashMap, HashSet};

/// Enhanced semantic search engine with keyword and pattern matching
#[derive(Debug)]
pub struct SemanticSearchEngine {
    /// Cache for search results
    search_cache: HashMap<String, Vec<IntentId>>,
}

impl SemanticSearchEngine {
    pub fn new() -> Self {
        Self {
            search_cache: HashMap::new(),
        }
    }

    /// Search intents using enhanced semantic matching
    pub fn search_intents(
        &self,
        query: &str,
        storage: &IntentGraphStorage,
        limit: Option<usize>,
    ) -> Result<Vec<IntentId>, RuntimeError> {
        let query_lower = query.to_lowercase();
        let mut scored_intents = Vec::new();

        // Get all intents for scoring
        let all_intents = storage.get_all_intents_sync();
        
        for intent in all_intents {
            let score = self.calculate_relevance_score(&query_lower, &intent);
            if score > 0.0 {
                scored_intents.push((intent.intent_id.clone(), score));
            }
        }

        // Sort by relevance score (descending)
        scored_intents.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Apply limit if specified
        let results: Vec<IntentId> = if let Some(limit) = limit {
            scored_intents.into_iter().take(limit).map(|(id, _)| id).collect()
        } else {
            scored_intents.into_iter().map(|(id, _)| id).collect()
        };

        Ok(results)
    }

    /// Calculate relevance score for an intent against a query
    fn calculate_relevance_score(&self, query: &str, intent: &StorableIntent) -> f64 {
        let mut score = 0.0;
        let goal_lower = intent.goal.to_lowercase();

        // Exact phrase match in goal (highest weight)
        if goal_lower.contains(query) {
            score += 1.0;
        }

        // Individual word matches in goal
        let query_words: Vec<&str> = query.split_whitespace().collect();
        let goal_words: Vec<&str> = goal_lower.split_whitespace().collect();
        
        for query_word in &query_words {
            if goal_words.iter().any(|w| w.contains(query_word)) {
                score += 0.3;
            }
        }

        // Match in constraints and preferences (lower weight)
        for (key, _value) in &intent.constraints {
            if key.to_lowercase().contains(query) {
                score += 0.2;
            }
        }

        for (key, _value) in &intent.preferences {
            if key.to_lowercase().contains(query) {
                score += 0.1;
            }
        }

        // Boost active intents
        match intent.status {
            IntentStatus::Executing => score *= 1.3,
            IntentStatus::Active => score *= 1.2,
            IntentStatus::Failed => score *= 1.1,
            IntentStatus::Suspended => score *= 0.8,
            IntentStatus::Archived => score *= 0.5,
            _ => {}
        }

        score
    }

    /// Find similar intents based on goal similarity
    pub fn find_similar_intents(
        &self,
        target_intent: &StorableIntent,
        storage: &IntentGraphStorage,
        limit: usize,
    ) -> Result<Vec<IntentId>, RuntimeError> {
        let all_intents = storage.get_all_intents_sync();
        let mut similarities = Vec::new();

        for intent in all_intents {
            if intent.intent_id == target_intent.intent_id {
                continue; // Skip self
            }
            
            let similarity = self.calculate_intent_similarity(target_intent, &intent);
            if similarity > 0.1 { // Minimum similarity threshold
                similarities.push((intent.intent_id.clone(), similarity));
            }
        }

        // Sort by similarity (descending)
        similarities.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        Ok(similarities.into_iter().take(limit).map(|(id, _)| id).collect())
    }

    /// Calculate similarity between two intents
    fn calculate_intent_similarity(&self, intent_a: &StorableIntent, intent_b: &StorableIntent) -> f64 {
        let goal_a = intent_a.goal.to_lowercase();
        let goal_b = intent_b.goal.to_lowercase();

        // Simple word overlap similarity
        let words_a: HashSet<&str> = goal_a.split_whitespace().collect();
        let words_b: HashSet<&str> = goal_b.split_whitespace().collect();

        let intersection = words_a.intersection(&words_b).count();
        let union = words_a.union(&words_b).count();

        if union == 0 {
            0.0
        } else {
            intersection as f64 / union as f64
        }
    }
}

/// Graph traversal engine for neighborhood and cluster analysis
#[derive(Debug)]
pub struct GraphTraversalEngine;

impl GraphTraversalEngine {
    pub fn new() -> Self {
        Self
    }

    /// Collect all intents within specified depth from focal points
    pub fn collect_neighborhood(
        &self,
        focal_intents: &[IntentId],
        storage: &IntentGraphStorage,
        max_depth: usize,
    ) -> Result<Vec<IntentId>, RuntimeError> {
        let mut visited = HashSet::new();
        let mut result = Vec::new();
        let mut current_layer = focal_intents.to_vec();

        for _depth in 0..=max_depth {
            let mut next_layer = Vec::new();

            for intent_id in &current_layer {
                if visited.contains(intent_id) {
                    continue;
                }

                visited.insert(intent_id.clone());
                result.push(intent_id.clone());

                // Get connected intents
                let connected = storage.get_connected_intents_sync(intent_id);
                for connected_id in connected {
                    if !visited.contains(&connected_id) {
                        next_layer.push(connected_id);
                    }
                }
            }

            current_layer = next_layer;
            if current_layer.is_empty() {
                break;
            }
        }

        Ok(result)
    }

    /// Identify clusters of related intents
    pub fn identify_clusters(
        &self,
        intent_ids: &[IntentId],
        storage: &IntentGraphStorage,
    ) -> Result<Vec<Vec<IntentId>>, RuntimeError> {
        let mut clusters = Vec::new();
        let mut visited = HashSet::new();

        for intent_id in intent_ids {
            if visited.contains(intent_id) {
                continue;
            }

            // Perform DFS to find connected component
            let cluster = self.dfs_cluster(intent_id, storage, &mut visited)?;
            if !cluster.is_empty() {
                clusters.push(cluster);
            }
        }

        Ok(clusters)
    }

    /// Depth-first search to find a cluster of connected intents
    fn dfs_cluster(
        &self,
        start_id: &IntentId,
        storage: &IntentGraphStorage,
        visited: &mut HashSet<IntentId>,
    ) -> Result<Vec<IntentId>, RuntimeError> {
        let mut cluster = Vec::new();
        let mut stack = vec![start_id.clone()];

        while let Some(current_id) = stack.pop() {
            if visited.contains(&current_id) {
                continue;
            }

            visited.insert(current_id.clone());
            cluster.push(current_id.clone());

            // Add connected intents to stack
            let connected = storage.get_connected_intents_sync(&current_id);
            for connected_id in connected {
                if !visited.contains(&connected_id) {
                    stack.push(connected_id);
                }
            }
        }

        Ok(cluster)
    }

    /// Find shortest path between two intents
    pub fn find_path(
        &self,
        from: &IntentId,
        to: &IntentId,
        storage: &IntentGraphStorage,
        max_depth: usize,
    ) -> Result<Option<Vec<IntentId>>, RuntimeError> {
        let mut queue = std::collections::VecDeque::new();
        let mut visited = HashSet::new();
        let mut parent: HashMap<IntentId, IntentId> = HashMap::new();

        queue.push_back((from.clone(), 0));
        visited.insert(from.clone());

        while let Some((current_id, depth)) = queue.pop_front() {
            if current_id == *to {
                // Reconstruct path
                let mut path = Vec::new();
                let mut current = to.clone();
                
                while let Some(p) = parent.get(&current) {
                    path.push(current.clone());
                    current = p.clone();
                }
                path.push(from.clone());
                path.reverse();
                return Ok(Some(path));
            }

            if depth >= max_depth {
                continue;
            }

            let connected = storage.get_connected_intents_sync(&current_id);
            for next_id in connected {
                if !visited.contains(&next_id) {
                    visited.insert(next_id.clone());
                    parent.insert(next_id.clone(), current_id.clone());
                    queue.push_back((next_id, depth + 1));
                }
            }
        }

        Ok(None)
    }
}
