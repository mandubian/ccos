//! RTFS Task Delegation Protocol (M5 bootstrap)
//! Defines minimal TaskRequest / TaskResult structures for future orchestrator dispatch.
use serde::{Serialize, Deserialize};
use crate::runtime::values::Value;

/// A high-level delegated task request produced when Arbiter selects an external agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskRequest {
    /// Stable identifier for the originating intent
    pub intent_id: String,
    /// Target agent identifier
    pub agent_id: String,
    /// Natural language goal (echoed for transparency)
    pub goal: String,
    /// Optional structured parameters (future extension)
    pub params: std::collections::HashMap<String, Value>,
}

impl TaskRequest {
    pub fn new(intent_id: String, agent_id: String, goal: String) -> Self { Self { intent_id, agent_id, goal, params: std::collections::HashMap::new() } }
}

/// Result returned by delegated agent execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskResult {
    pub intent_id: String,
    pub agent_id: String,
    pub success: bool,
    pub output: Option<Value>,
    pub error: Option<String>,
}

impl TaskResult { pub fn success(intent_id: String, agent_id: String, output: Option<Value>) -> Self { Self { intent_id, agent_id, success: true, output, error: None } } }
