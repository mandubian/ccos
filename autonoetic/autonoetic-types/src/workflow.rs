//! Durable workflow orchestration records (gateway-owned persistence).
//!
//! These types back the workflow layer described in `autonoetic/plan_workflow_orchestration.md`.
//! They intentionally avoid session-path parsing semantics — callers supply explicit
//! `root_session_id` and `workflow_id` at persistence boundaries.

use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

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
    /// Task IDs currently queued for async execution (not yet running).
    #[serde(default)]
    pub queued_task_ids: Vec<String>,
    /// Join policy for this workflow's planner resume.
    #[serde(default)]
    pub join_policy: JoinPolicy,
    /// Task IDs that must complete before the planner resumes (join condition).
    #[serde(default)]
    pub join_task_ids: Vec<String>,
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
    /// Join group this task belongs to (tasks in the same group are awaited together).
    #[serde(default)]
    pub join_group: Option<String>,
    /// Original kickoff message for the child agent. Preserved across approval boundaries.
    #[serde(default)]
    pub message: Option<String>,
    /// Original metadata passed through to the child. Preserved across approval boundaries.
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

/// Join policy for a group of tasks.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JoinPolicy {
    /// All tasks in the group must complete before the planner resumes.
    AllOf,
    /// Any one task completing satisfies the join.
    AnyOf,
    /// First task that succeeds satisfies the join; failures are ignored.
    FirstSuccess,
    /// Manual: planner must explicitly call workflow.wait or resume.
    Manual,
}

impl Default for JoinPolicy {
    fn default() -> Self {
        Self::AllOf
    }
}

/// A queued task awaiting async execution by the scheduler.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueuedTaskRun {
    pub task_id: String,
    pub workflow_id: String,
    pub agent_id: String,
    /// Kickoff message for the child agent.
    pub message: String,
    /// Delegation path used as session_id for the child.
    pub child_session_id: String,
    /// Source (parent) session.
    pub parent_session_id: String,
    /// Agent that initiated the spawn.
    pub source_agent_id: String,
    /// Optional metadata passed through to the child.
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    /// Join group for this task (planner resumes when join condition is met).
    #[serde(default)]
    pub join_group: Option<String>,
    /// Whether this task blocks the planner from continuing.
    #[serde(default = "default_true")]
    pub blocks_planner: bool,
    pub enqueued_at: String,
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
    pub agent_id: Option<String>,
    #[serde(default)]
    pub payload: serde_json::Value,
    pub occurred_at: String,
}

// ---------------------------------------------------------------------------
// Durable checkpoints (Phase 3)
// ---------------------------------------------------------------------------

/// Durable planner-level checkpoint.
/// Stores the orchestrator's delegation state at the end of a turn so it can
/// resume deterministically after join satisfaction or gateway restart.
/// Explicitly separate from `SessionContext` (prompt continuity) and
/// `SessionSnapshot` (branch/fork).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowCheckpoint {
    pub workflow_id: String,
    /// Monotonically increasing version per workflow.
    pub version: u32,
    /// Natural-language description of what the planner was doing.
    pub planner_intent: String,
    /// Task IDs the planner delegated and is waiting for.
    pub pending_task_ids: Vec<String>,
    /// The join policy governing resume.
    pub join_policy: JoinPolicy,
    /// Arbitrary planner context (JSON): delegation instructions, expected
    /// result shape, intermediate decisions, etc.
    #[serde(default)]
    pub context: serde_json::Value,
    pub created_at: String,
}

/// Durable task-level checkpoint.
/// Stores a child task's execution progress between sandbox execs or approval
/// boundaries so it can resume without replaying from scratch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskCheckpoint {
    pub workflow_id: String,
    pub task_id: String,
    /// Monotonically increasing version per task.
    pub version: u32,
    /// Label for the current execution step (e.g., "setup", "run_tests", "build").
    pub step: String,
    /// Arbitrary task state (JSON): last script output, file hashes,
    /// accumulated data, etc.
    #[serde(default)]
    pub state: serde_json::Value,
    pub created_at: String,
}
