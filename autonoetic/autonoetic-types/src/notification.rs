use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NotificationRecord {
    pub notification_id: String,             // "ntf-xxxxxxxx"
    pub notification_type: NotificationType, // ApprovalResolved | WorkflowJoinSatisfied
    pub request_id: Option<String>,          // approval request_id
    pub payload: Value,                      // structured JSON payload
    pub target_session_id: String,           // explicit target
    pub target_agent_id: Option<String>,     // explicit target
    pub workflow_id: Option<String>,
    pub task_id: Option<String>,
    pub status: NotificationStatus,          // Pending | ActionExecuted | Delivered | Consumed
    pub created_at: String,
    pub action_completed_at: Option<String>,
    pub delivered_at: Option<String>,
    pub consumed_at: Option<String>,
    pub attempt_count: u32,
    pub last_attempt_at: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NotificationStatus {
    Pending,        // decision recorded, action not yet executed
    ActionExecuted, // gateway auto-executed the approved action
    Delivered,      // notification visible to consumers
    Consumed,       // consumer acknowledged
    Failed,         // action execution permanently failed
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NotificationType {
    ApprovalResolved,
    WorkflowJoinSatisfied,
}

impl NotificationRecord {
    pub fn new(
        notification_id: String,
        notification_type: NotificationType,
        target_session_id: String,
        payload: Value,
    ) -> Self {
        Self {
            notification_id,
            notification_type,
            request_id: None,
            payload,
            target_session_id,
            target_agent_id: None,
            workflow_id: None,
            task_id: None,
            status: NotificationStatus::Pending,
            created_at: chrono::Utc::now().to_rfc3339(),
            action_completed_at: None,
            delivered_at: None,
            consumed_at: None,
            attempt_count: 0,
            last_attempt_at: None,
            error_message: None,
        }
    }
}
