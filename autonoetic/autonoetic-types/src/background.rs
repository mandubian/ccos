//! Background scheduling and reevaluation types.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundMode {
    Deterministic,
    Reasoning,
}

impl Default for BackgroundMode {
    fn default() -> Self {
        Self::Deterministic
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WakePredicates {
    #[serde(default = "default_true")]
    pub timer: bool,
    #[serde(default)]
    pub new_messages: bool,
    #[serde(default)]
    pub task_completions: bool,
    #[serde(default)]
    pub queued_work: bool,
    #[serde(default)]
    pub stale_goals: bool,
    #[serde(default)]
    pub retryable_failures: bool,
    #[serde(default)]
    pub approval_resolved: bool,
}

impl Default for WakePredicates {
    fn default() -> Self {
        Self {
            timer: true,
            new_messages: false,
            task_completions: false,
            queued_work: false,
            stale_goals: false,
            retryable_failures: false,
            approval_resolved: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct BackgroundPolicy {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub interval_secs: u64,
    #[serde(default)]
    pub mode: BackgroundMode,
    #[serde(default)]
    pub wake_predicates: WakePredicates,
    #[serde(default = "default_true")]
    pub validate_on_install: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduledActionDependencies {
    pub runtime: String,
    pub packages: Vec<String>,
}

/// Actions that can be stored in reevaluation state and executed by the background scheduler,
/// or used as the *subject* of an approval request (ApprovalRequest/ApprovalDecision).
///
/// **Schedulable vs approval-only:**
/// - `WriteFile` and `SandboxExec` are real runnable actions: the scheduler can execute them
///   (after approval when `requires_approval` is true). They may also appear in
///   `pending_scheduled_action` and in approval requests.
/// - `AgentInstall` is **not** something the scheduler runs. We cannot "schedule" the
///   installation of an agent. It exists only as the subject of an approval request: when
///   `agent.install` requires human approval, we create an approval with action=AgentInstall;
///   after the operator approves, the *caller* retries `agent.install` with
///   `install_approval_ref` and the install runs synchronously. The scheduler never executes
///   an AgentInstall; it is only a label for "this approval was for an install."
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScheduledAction {
    WriteFile {
        path: String,
        content: String,
        #[serde(default)]
        requires_approval: bool,
        #[serde(default)]
        evidence_ref: Option<String>,
    },
    SandboxExec {
        command: String,
        #[serde(default)]
        dependencies: Option<ScheduledActionDependencies>,
        #[serde(default)]
        requires_approval: bool,
        #[serde(default)]
        evidence_ref: Option<String>,
    },
    /// Approval subject only: "this approval request is for an agent install." Not executed by the scheduler; install is performed by the caller retrying `agent.install` with `install_approval_ref`.
    AgentInstall {
        agent_id: String,
        summary: String,
        requested_by_agent_id: String,
        install_fingerprint: String,
    },
}

impl ScheduledAction {
    /// True if this action is something the scheduler can execute (WriteFile, SandboxExec).
    /// False for AgentInstall, which is only an approval subject.
    pub fn is_executable_by_scheduler(&self) -> bool {
        !matches!(self, Self::AgentInstall { .. })
    }

    pub fn requires_approval(&self) -> bool {
        match self {
            Self::WriteFile {
                requires_approval, ..
            }
            | Self::SandboxExec {
                requires_approval, ..
            } => *requires_approval,
            Self::AgentInstall { .. } => true,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::WriteFile { .. } => "write_file",
            Self::SandboxExec { .. } => "sandbox_exec",
            Self::AgentInstall { .. } => "agent_install",
        }
    }

    pub fn evidence_ref(&self) -> Option<String> {
        match self {
            Self::WriteFile { evidence_ref, .. } => evidence_ref.clone(),
            Self::SandboxExec { evidence_ref, .. } => evidence_ref.clone(),
            Self::AgentInstall { .. } => None,
        }
    }

    pub fn with_evidence_ref(mut self, evidence_ref: Option<String>) -> Self {
        match &mut self {
            Self::WriteFile {
                evidence_ref: r, ..
            } => *r = evidence_ref,
            Self::SandboxExec {
                evidence_ref: r, ..
            } => *r = evidence_ref,
            Self::AgentInstall { .. } => {}
        }
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ReevaluationState {
    #[serde(default)]
    pub retry_not_before: Option<String>,
    #[serde(default)]
    pub stale_goal_at: Option<String>,
    #[serde(default)]
    pub last_outcome: Option<String>,
    #[serde(default)]
    pub pending_scheduled_action: Option<ScheduledAction>,
    #[serde(default)]
    pub open_approval_request_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WakeReason {
    Timer { due_bucket: String },
    NewMessage { event_id: String, message: String },
    TaskCompletion { task_id: String, status: String },
    QueuedWork { task_id: String, status: String },
    StaleGoal { marker_id: String },
    RetryableFailure { marker_id: String },
    ApprovalResolved { request_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct BackgroundState {
    #[serde(default)]
    pub agent_id: String,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub last_wake_at: Option<String>,
    #[serde(default)]
    pub last_wake_reason: Option<WakeReason>,
    #[serde(default)]
    pub last_result: Option<String>,
    #[serde(default)]
    pub next_due_at: Option<String>,
    #[serde(default)]
    pub active_session_ids: Vec<String>,
    #[serde(default)]
    pub pending_wake_fingerprints: Vec<String>,
    #[serde(default)]
    pub retry_not_before: Option<String>,
    #[serde(default)]
    pub approval_blocked: bool,
    #[serde(default)]
    pub pending_approval_request_ids: Vec<String>,
    #[serde(default)]
    pub processed_inbox_event_ids: Vec<String>,
    #[serde(default)]
    pub processed_task_keys: Vec<String>,
    #[serde(default)]
    pub processed_approval_request_ids: Vec<String>,
}

/// A request for human approval. The `action` describes what is being approved: either a
/// schedulable action (WriteFile, SandboxExec) that the scheduler will run after approval, or
/// an approval-only subject (AgentInstall) where the actual install is done by the caller
/// retrying with install_approval_ref.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovalRequest {
    pub request_id: String,
    pub agent_id: String,
    pub session_id: String,
    pub action: ScheduledAction,
    pub created_at: String,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub evidence_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Approved,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovalDecision {
    pub request_id: String,
    pub agent_id: String,
    pub session_id: String,
    pub action: ScheduledAction,
    pub status: ApprovalStatus,
    pub decided_at: String,
    pub decided_by: String,
    #[serde(default)]
    pub reason: Option<String>,
}

fn default_true() -> bool {
    true
}
