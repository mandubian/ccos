//! Boundary model for Working Memory/Context Horizon retrieval.
//!
//! This mirrors and concretizes SEP-009 boundary concepts for local use:
//! - TokenLimit, TimeLimit, MemoryLimit, SemanticLimit
//! - ReductionStrategy controlling per-section budgets and optional scoring
//!
//! Keep this file focused on small data structures and simple helpers.
//! Unit tests validate defaults and simple serialization.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Token/memory/semantic/time boundary types supported by Context Horizon.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum BoundaryType {
    TokenLimit,
    TimeLimit,
    MemoryLimit,
    SemanticLimit,
}

impl Default for BoundaryType {
    fn default() -> Self {
        BoundaryType::TokenLimit
    }
}

/// Generic boundary with a type and constraints key-value map.
/// Examples:
/// - TokenLimit: {"max_tokens": 8192}
/// - TimeLimit: {"from_ts": 1719772800, "to_ts": 1719859200}
/// - MemoryLimit: {"max_bytes": 1_000_000}
/// - SemanticLimit: {"min_relevance": 0.5}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Boundary {
    pub name: String,
    pub boundary_type: BoundaryType,
    pub constraints: HashMap<String, serde_json::Value>,
}

/// Reduction strategy influences ranking and per-section budgets.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReductionStrategy {
    pub enable_semantic_scoring: bool,
    /// Half-life in seconds for time-decay ranking (None disables decay).
    pub time_decay_half_life_s: Option<u64>,
    /// Per-section token budgets, e.g., {"intents": 4000, "wisdom": 2000, "plan": 2000}.
    pub per_section_budgets: HashMap<String, usize>,
}

impl Boundary {
    pub fn new(name: impl Into<String>, boundary_type: BoundaryType) -> Self {
        Self {
            name: name.into(),
            boundary_type,
            constraints: HashMap::new(),
        }
    }

    pub fn with_constraint(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.constraints.insert(key.into(), value);
        self
    }

    pub fn get_u64(&self, key: &str) -> Option<u64> {
        self.constraints.get(key).and_then(|v| v.as_u64())
    }

    pub fn get_usize(&self, key: &str) -> Option<usize> {
        self.constraints
            .get(key)
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
    }
}

impl ReductionStrategy {
    pub fn with_budget(mut self, section: impl Into<String>, tokens: usize) -> Self {
        self.per_section_budgets.insert(section.into(), tokens);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_boundary_builders() {
        let b = Boundary::new("token-limit", BoundaryType::TokenLimit)
            .with_constraint("max_tokens", serde_json::json!(8192));
        assert_eq!(b.name, "token-limit");
        assert_eq!(b.boundary_type, BoundaryType::TokenLimit);
        assert_eq!(b.get_usize("max_tokens"), Some(8192));
    }

    #[test]
    fn test_time_limit_accessors() {
        let b = Boundary::new("time-limit", BoundaryType::TimeLimit)
            .with_constraint("from_ts", serde_json::json!(100))
            .with_constraint("to_ts", serde_json::json!(200));
        assert_eq!(b.get_u64("from_ts"), Some(100));
        assert_eq!(b.get_u64("to_ts"), Some(200));
    }

    #[test]
    fn test_reduction_strategy_budgets() {
        let rs = ReductionStrategy {
            enable_semantic_scoring: true,
            time_decay_half_life_s: Some(3600),
            per_section_budgets: HashMap::new(),
        }
        .with_budget("intents", 4000)
        .with_budget("wisdom", 2000)
        .with_budget("plan", 2000);

        assert!(rs.enable_semantic_scoring);
        assert_eq!(rs.time_decay_half_life_s, Some(3600));
        assert_eq!(rs.per_section_budgets.get("wisdom").copied(), Some(2000));
    }
}
