//! Agent Memory
//!
//! Wraps WorkingMemory with agent-specific semantics and learned patterns.
//! Provides recall, learning, and execution history for individual agents.

use crate::working_memory::backend::{QueryParams, WorkingMemoryError};
use crate::working_memory::facade::WorkingMemory;
use crate::working_memory::types::{WorkingMemoryEntry, WorkingMemoryMeta};
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

/// A pattern learned from execution failures or successes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LearnedPattern {
    /// Unique identifier for this pattern
    pub pattern_id: String,
    /// Human-readable description of what was learned
    pub description: String,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f64,
    /// Failure IDs that contributed to learning this pattern
    pub source_failures: Vec<String>,
    /// Capability IDs this pattern applies to
    pub related_capabilities: Vec<String>,
    /// Error category this pattern addresses (e.g., "SchemaError")
    pub error_category: Option<String>,
    /// Suggested fix or action
    pub suggested_action: Option<String>,
    /// Unix timestamp when pattern was learned
    pub learned_at: u64,
}

impl LearnedPattern {
    /// Create a new learned pattern with current timestamp.
    pub fn new(pattern_id: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            pattern_id: pattern_id.into(),
            description: description.into(),
            confidence: 0.5,
            source_failures: vec![],
            related_capabilities: vec![],
            error_category: None,
            suggested_action: None,
            learned_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }

    /// Builder: set confidence
    pub fn with_confidence(mut self, confidence: f64) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }

    /// Builder: set error category
    pub fn with_error_category(mut self, category: impl Into<String>) -> Self {
        self.error_category = Some(category.into());
        self
    }

    /// Builder: set suggested action
    pub fn with_suggested_action(mut self, action: impl Into<String>) -> Self {
        self.suggested_action = Some(action.into());
        self
    }

    /// Add a source failure ID
    pub fn add_source_failure(&mut self, failure_id: impl Into<String>) {
        self.source_failures.push(failure_id.into());
    }

    /// Add a related capability
    pub fn add_related_capability(&mut self, capability_id: impl Into<String>) {
        self.related_capabilities.push(capability_id.into());
    }
}

/// Agent-specific memory wrapping WorkingMemory with learned patterns.
pub struct AgentMemory {
    agent_id: String,
    working_memory: Arc<Mutex<WorkingMemory>>,
    learned_patterns: Vec<LearnedPattern>,
}

impl AgentMemory {
    /// Create a new AgentMemory for a specific agent.
    pub fn new(agent_id: impl Into<String>, working_memory: Arc<Mutex<WorkingMemory>>) -> Self {
        Self {
            agent_id: agent_id.into(),
            working_memory,
            learned_patterns: vec![],
        }
    }

    /// Get the agent ID.
    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    /// Store an entry in working memory with agent tagging.
    pub fn store(
        &self,
        title: impl Into<String>,
        content: impl Into<String>,
        additional_tags: &[&str],
    ) -> Result<String, WorkingMemoryError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let entry_id = format!("agent:{}:{}", self.agent_id, uuid::Uuid::new_v4());

        let mut tags: HashSet<String> = additional_tags.iter().map(|s| s.to_string()).collect();
        tags.insert(format!("agent:{}", self.agent_id));
        tags.insert("agent-memory".to_string());

        let meta = WorkingMemoryMeta {
            provider: Some(format!("agent:{}", self.agent_id)),
            ..Default::default()
        };

        let entry = WorkingMemoryEntry::new_with_estimate(
            entry_id.clone(),
            title,
            content,
            tags.into_iter(),
            now,
            meta,
        );

        let mut wm = self
            .working_memory
            .lock()
            .map_err(|_| WorkingMemoryError::Other("Failed to lock working memory".to_string()))?;
        wm.append(entry)?;

        Ok(entry_id)
    }

    /// Recall relevant entries for this agent based on context/tags.
    pub fn recall_relevant(
        &self,
        context_tags: &[&str],
        limit: Option<usize>,
    ) -> Result<Vec<WorkingMemoryEntry>, WorkingMemoryError> {
        let mut tags: HashSet<String> = context_tags.iter().map(|s| s.to_string()).collect();
        // Always filter by this agent
        tags.insert(format!("agent:{}", self.agent_id));

        let params = QueryParams::with_tags(tags).with_limit(limit);

        let wm = self
            .working_memory
            .lock()
            .map_err(|_| WorkingMemoryError::Other("Failed to lock working memory".to_string()))?;
        let result = wm.query(&params)?;
        Ok(result.entries)
    }

    /// Get execution history for this agent (most recent first).
    pub fn get_execution_history(
        &self,
        limit: usize,
    ) -> Result<Vec<WorkingMemoryEntry>, WorkingMemoryError> {
        let tags: HashSet<String> = [
            format!("agent:{}", self.agent_id),
            "causal-chain".to_string(),
        ]
        .into_iter()
        .collect();

        let params = QueryParams::with_tags(tags).with_limit(Some(limit));

        let wm = self
            .working_memory
            .lock()
            .map_err(|_| WorkingMemoryError::Other("Failed to lock working memory".to_string()))?;
        let result = wm.query(&params)?;
        Ok(result.entries)
    }

    /// Store a learned pattern and index it in working memory.
    pub fn store_learned_pattern(&mut self, pattern: LearnedPattern) {
        // Index into WorkingMemory for recall using interior mutability
        let tags = vec!["learned-pattern", "learning"];
        let _ = self.store(
            format!("Pattern: {}", pattern.description),
            serde_json::to_string(&pattern).unwrap_or_default(),
            &tags,
        );

        // Avoid duplicates by pattern_id
        self.learned_patterns
            .retain(|p| p.pattern_id != pattern.pattern_id);
        self.learned_patterns.push(pattern);
    }

    /// Get all learned patterns.
    pub fn get_learned_patterns(&self) -> &[LearnedPattern] {
        &self.learned_patterns
    }

    /// Find patterns relevant to an error category.
    pub fn find_patterns_for_error(&self, error_category: &str) -> Vec<&LearnedPattern> {
        self.learned_patterns
            .iter()
            .filter(|p| p.error_category.as_deref() == Some(error_category))
            .collect()
    }

    /// Find patterns related to a capability.
    pub fn find_patterns_for_capability(&self, capability_id: &str) -> Vec<&LearnedPattern> {
        self.learned_patterns
            .iter()
            .filter(|p| p.related_capabilities.contains(&capability_id.to_string()))
            .collect()
    }

    /// Get high-confidence patterns (confidence >= threshold).
    pub fn get_high_confidence_patterns(&self, threshold: f64) -> Vec<&LearnedPattern> {
        self.learned_patterns
            .iter()
            .filter(|p| p.confidence >= threshold)
            .collect()
    }

    /// Helper to serialize learned patterns.
    fn serialize_patterns(&self) -> Result<String, Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(&self.learned_patterns)?;
        Ok(json)
    }

    /// Helper to load learned patterns from JSON string.
    fn load_patterns(&mut self, json_str: &str) -> Result<(), Box<dyn std::error::Error>> {
        let patterns: Vec<LearnedPattern> = serde_json::from_str(json_str)?;
        for pattern in patterns {
            self.store_learned_pattern(pattern);
        }
        Ok(())
    }

    /// Save learned patterns to a JSON file.
    pub fn save_to_disk(&self, path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = self.serialize_patterns()?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load learned patterns from a JSON file.
    pub fn load_from_disk(
        &mut self,
        path: &std::path::Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if path.exists() {
            let json = std::fs::read_to_string(path)?;
            self.load_patterns(&json)?;
        }
        Ok(())
    }
}
