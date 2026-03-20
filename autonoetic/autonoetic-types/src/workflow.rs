//! Durable workflow orchestration records (gateway-owned persistence).
//!
//! These types back the workflow layer described in `autonoetic/plan_workflow_orchestration.md`.
//! They intentionally avoid session-path parsing semantics — callers supply explicit
//! `root_session_id` and `workflow_id` at persistence boundaries.

use serde::{Deserialize, Serialize};

/// Lifecycle of a user-facing workflow run (one per root task / root session).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunStatus {
    Active,
    WaitingChildren,
    BlockedApproval,
    Resumable,
    Completed,
    Failed,
    Cancelled,
}

impl Default for WorkflowRunStatus {
    fn default() -> Self {
        Self::Active
    }
}

/// Lifecycle of a delegated child execution unit.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskRunStatus {
    Pending,
    Runnable,
    Running,
    AwaitingApproval,
    Paused,
    Succeeded,
    Failed,
    Cancelled,
}

impl Default for TaskRunStatus {
    fn default() -> Self {
        Self::Pending
    }
}

/// One durable workflow run keyed by `workflow_id`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowRun {
    pub workflow_id: String,
    pub root_session_id: String,
    /// Front-door / lead agent when known; empty if not yet recorded.
    #[serde(default)]
    pub lead_agent_id: String,
    #[serde(default)]
    pub status: WorkflowRunStatus,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub active_task_ids: Vec<String>,
    #[serde(default)]
    pub blocked_task_ids: Vec<String>,
    #[serde(default)]
    pub pending_approval_ids: Vec<String>,
}

/// One delegated task (typically one `agent.spawn` child path).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskRun {
    pub task_id: String,
    pub workflow_id: String,
    /// Target specialist `agent_id`.
    pub agent_id: String,
    /// Child delegation session id (e.g. `root/parent-abc`).
    pub session_id: String,
    /// Session id of the delegating agent when spawned from a parent.
    #[serde(default)]
    pub parent_session_id: String,
    #[serde(default)]
    pub status: TaskRunStatus,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub source_agent_id: Option<String>,
    /// Short summary (length-capped by the gateway), not a full transcript.
    #[serde(default)]
    pub result_summary: Option<String>,
}

/// Append-only workflow event (mirrors plan `WorkflowEvent` concept).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowEventRecord {
    pub event_id: String,
    pub workflow_id: String,
    #[serde(default)]
    pub task_id: Option<String>,
    pub event_type: String,
    #[serde(default)]
    pub payload: serde_json::Value,
    pub occurred_at: String,
}
