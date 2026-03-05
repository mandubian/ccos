//! Task Board Entry — shared inter-agent task queue.

use serde::{Deserialize, Serialize};

/// Status of a task board entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Claimed,
    Completed,
    Failed,
}

/// A single entry in the task board.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskBoardEntry {
    pub task_id: String,
    pub creator_id: String,
    pub title: String,
    pub description: String,
    pub status: TaskStatus,
    pub assignee_id: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub capabilities_required: Vec<String>,
    pub result: Option<serde_json::Value>,
}
