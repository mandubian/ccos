//! Wake-decision logic for the background scheduler.
//! Determines whether an agent should wake based on timers, messages, tasks, and approvals.

use crate::execution::{gateway_actor_id, init_gateway_causal_logger};
use crate::tracing::{EventScope, SessionId, TraceSession};
use autonoetic_types::background::{
    BackgroundPolicy, BackgroundState, ReevaluationState, WakeReason,
};
use autonoetic_types::config::GatewayConfig;
use autonoetic_types::task_board::TaskStatus;
use chrono::{DateTime, Utc};
use std::sync::Arc;

pub fn effective_interval_secs(
    config: &GatewayConfig,
    background: &BackgroundPolicy,
    cap_min_interval: u64,
) -> u64 {
    background
        .interval_secs
        .max(1)
        .max(config.background_min_interval_secs.max(1))
        .max(cap_min_interval.max(1))
}

pub fn should_wake(
    config: &GatewayConfig,
    agent_id: &str,
    session_id: &str,
    background: &BackgroundPolicy,
    state: &BackgroundState,
    reevaluation: &ReevaluationState,
    now: DateTime<Utc>,
) -> anyhow::Result<Option<WakeReason>> {
    let _ = session_id;

    for request_id in &reevaluation.open_approval_request_ids {
        if state.processed_approval_request_ids.contains(request_id) {
            continue;
        }
        if crate::scheduler::approved_approvals_dir(config)
            .join(format!("{request_id}.json"))
            .exists()
        {
            return Ok(Some(WakeReason::ApprovalResolved {
                request_id: request_id.clone(),
            }));
        }
    }

    if reevaluation.pending_scheduled_action.is_some()
        && reevaluation.open_approval_request_ids.is_empty()
        && !state.approval_blocked
    {
        let due_bucket = state
            .next_due_at
            .clone()
            .unwrap_or_else(|| now.to_rfc3339());
        return Ok(Some(WakeReason::Timer { due_bucket }));
    }

    if background.wake_predicates.new_messages {
        for event in crate::scheduler::store::load_inbox_events(config, agent_id)? {
            if !state.processed_inbox_event_ids.contains(&event.event_id) {
                return Ok(Some(WakeReason::NewMessage {
                    event_id: event.event_id,
                    message: event.message,
                }));
            }
        }
    }

    if background.wake_predicates.task_completions || background.wake_predicates.queued_work {
        for task in crate::scheduler::store::load_task_board_entries(config)? {
            if task.assignee_id.as_deref() != Some(agent_id) {
                continue;
            }
            let task_status = match &task.status {
                TaskStatus::Pending => "pending",
                TaskStatus::Claimed => "claimed",
                TaskStatus::Completed => "completed",
                TaskStatus::Failed => "failed",
            };
            let task_key = format!("{}:{}", task.task_id, task_status);
            if state.processed_task_keys.contains(&task_key) {
                continue;
            }
            match &task.status {
                TaskStatus::Completed if background.wake_predicates.task_completions => {
                    return Ok(Some(WakeReason::TaskCompletion {
                        task_id: task.task_id,
                        status: "completed".to_string(),
                    }));
                }
                TaskStatus::Pending | TaskStatus::Claimed
                    if background.wake_predicates.queued_work =>
                {
                    return Ok(Some(WakeReason::QueuedWork {
                        task_id: task.task_id,
                        status: format!("{:?}", task.status).to_ascii_lowercase(),
                    }));
                }
                _ => {}
            }
        }
    }

    if let Some(next_due) = &state.next_due_at {
        let due = parse_timestamp(next_due)?;
        if due <= now {
            return Ok(Some(WakeReason::Timer {
                due_bucket: next_due.clone(),
            }));
        }
    }

    Ok(None)
}

pub fn log_should_wake(
    config: &GatewayConfig,
    session_id: &str,
    agent_id: &str,
    reason: &Option<WakeReason>,
    effective_interval: u64,
) {
    let Some(_reason) = reason else {
        return;
    };
    if let Ok(causal_logger) = init_gateway_causal_logger(config) {
        let mut trace_session = TraceSession::create_with_session_id(
            SessionId::from_string(session_id.to_string()),
            Arc::new(causal_logger),
            gateway_actor_id(),
            EventScope::Session,
        );
        let _ = trace_session.log_completed(
            "background.should_wake",
            None,
            Some(serde_json::json!({
                "agent_id": agent_id,
                "effective_interval_secs": effective_interval,
                "wake_reason": reason
            })),
        );
    }
}

pub fn mark_reason_processed(state: &mut BackgroundState, reason: &WakeReason) {
    match reason {
        WakeReason::NewMessage { event_id, .. } => {
            state.processed_inbox_event_ids.push(event_id.clone());
        }
        WakeReason::TaskCompletion { task_id, .. } => {
            state
                .processed_task_keys
                .push(format!("{}:completed", task_id));
        }
        WakeReason::QueuedWork { task_id, status } => {
            state
                .processed_task_keys
                .push(format!("{}:{}", task_id, status));
        }
        WakeReason::ApprovalResolved { request_id } => {
            state
                .processed_approval_request_ids
                .push(request_id.clone());
        }
        WakeReason::Timer { .. }
        | WakeReason::StaleGoal { .. }
        | WakeReason::RetryableFailure { .. } => {}
    }
}

pub fn wake_fingerprint(agent_id: &str, session_id: &str, reason: &WakeReason) -> String {
    match reason {
        WakeReason::Timer { due_bucket } => {
            format!("timer:{agent_id}:{session_id}:{due_bucket}")
        }
        WakeReason::NewMessage { event_id, .. } => format!("message:{event_id}"),
        WakeReason::TaskCompletion { task_id, status }
        | WakeReason::QueuedWork { task_id, status } => {
            format!("task:{task_id}:{status}")
        }
        WakeReason::RetryableFailure { marker_id } => format!("retry:{marker_id}"),
        WakeReason::StaleGoal { marker_id } => format!("stale:{marker_id}"),
        WakeReason::ApprovalResolved { request_id } => format!("approval:{request_id}"),
    }
}

pub fn background_session_id(agent_id: &str) -> String {
    format!("background::{}", agent_id)
}

pub fn parse_timestamp(value: &str) -> anyhow::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| anyhow::anyhow!("Invalid timestamp: {}", e))
}
