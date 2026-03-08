//! Side-effecting execution for the background scheduler.
//! Handles actual execution of background actions, including spawning agents,
//! executing scheduled actions, and handling approval resolutions.

use crate::execution::{gateway_actor_id, init_gateway_causal_logger, GatewayExecutionService};
use crate::runtime::reevaluation_state::persist_reevaluation_state;
use crate::tracing::{EventScope, SessionId, TraceSession};
use autonoetic_types::background::{
    ApprovalDecision, ApprovalRequest, BackgroundMode, BackgroundPolicy, BackgroundState,
    ReevaluationState, WakeReason,
};
use chrono::{DateTime, Duration, Utc};
use std::path::Path;
use std::sync::Arc;

pub use super::decision::{background_session_id, wake_fingerprint};
pub use super::store::{background_state_path, save_background_state};

pub fn background_kickoff_prompt(reason: &WakeReason, reevaluation: &ReevaluationState) -> String {
    format!(
        "Background reevaluation wake.\nReason: {:?}\nReevaluation state: {}",
        reason,
        serde_json::to_string_pretty(reevaluation).unwrap_or_else(|_| "{}".to_string())
    )
}

pub fn clear_reevaluation_after_success(agent_dir: &Path, reason: &WakeReason) -> anyhow::Result<()> {
    persist_reevaluation_state(agent_dir, |state| {
        state.last_outcome = Some("background_success".to_string());
        state.retry_not_before = None;
        match reason {
            WakeReason::ApprovalResolved { request_id } => {
                state
                    .open_approval_request_ids
                    .retain(|existing| existing != request_id);
                state.pending_scheduled_action = None;
            }
            WakeReason::StaleGoal { .. } => {
                state.stale_goal_at = None;
                state.pending_scheduled_action = None;
            }
            WakeReason::RetryableFailure { .. }
            | WakeReason::Timer { .. }
            | WakeReason::NewMessage { .. }
            | WakeReason::TaskCompletion { .. }
            | WakeReason::QueuedWork { .. } => {
                state.pending_scheduled_action = None;
            }
        }
    })?;
    Ok(())
}

pub async fn handle_due_wake(
    execution: Arc<GatewayExecutionService>,
    agent_id: &str,
    agent_dir: &Path,
    background: &BackgroundPolicy,
    allow_reasoning: bool,
    effective_interval: u64,
    session_id: &str,
    mut state: BackgroundState,
    reevaluation: ReevaluationState,
    reason: WakeReason,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    let config = execution.config();
    let state_path = background_state_path(&config, agent_id);
    let fingerprint = wake_fingerprint(agent_id, session_id, &reason);
    let causal_logger = init_gateway_causal_logger(config.as_ref())?;
    let mut trace_session = TraceSession::create_with_session_id(
        SessionId::from_string(session_id.to_string()),
        Arc::new(causal_logger),
        gateway_actor_id(),
        EventScope::Session,
    );

    if state.active_session_ids.iter().any(|id| id == session_id)
        || state
            .pending_wake_fingerprints
            .iter()
            .any(|existing| existing == &fingerprint)
    {
        let _ = trace_session.log_skipped(
            "background.wake",
            "duplicate_or_active",
            Some(serde_json::json!({
                "agent_id": agent_id,
                "reason": &reason,
            })),
        );
        return Ok(());
    }

    state.pending_wake_fingerprints.push(fingerprint.clone());
    state.active_session_ids.push(session_id.to_string());
    save_background_state(&state_path, &state)?;

    let _ = trace_session.log_requested(
        "background.wake",
        Some(serde_json::json!({
            "agent_id": agent_id,
            "reason": &reason,
            "mode": format!("{:?}", background.mode).to_ascii_lowercase()
        })),
    );

    let mut outcome = "skipped".to_string();
    let result = if matches!(reason, WakeReason::ApprovalResolved { .. }) {
        let request_id = match &reason {
            WakeReason::ApprovalResolved { request_id } => request_id,
            _ => unreachable!(),
        };
        let decision: ApprovalDecision = super::store::read_json_file(
            &super::approval::approved_approvals_dir(config.as_ref())
                .join(format!("{request_id}.json")),
        )?;
        outcome = "executed".to_string();
        Some(
            execution
                .execute_background_action(agent_id, session_id, &decision.action)
                .await,
        )
    } else if let Some(action) = reevaluation.pending_scheduled_action.clone() {
        if action.requires_approval() {
            if reevaluation.open_approval_request_ids.is_empty() {
                let request = ApprovalRequest {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    agent_id: agent_id.to_string(),
                    session_id: session_id.to_string(),
                    action,
                    created_at: now.to_rfc3339(),
                    reason: Some(format!("Wake reason: {:?}", reason)),
                    evidence_ref: reevaluation
                        .pending_scheduled_action
                        .as_ref()
                        .and_then(|a| a.evidence_ref()),
                };
                super::store::write_json_file(
                    &super::approval::pending_approvals_dir(config.as_ref())
                        .join(format!("{}.json", request.request_id)),
                    &request,
                )?;
                persist_reevaluation_state(agent_dir, |state| {
                    state
                        .open_approval_request_ids
                        .push(request.request_id.clone());
                })?;
                state.approval_blocked = true;
                state
                    .pending_approval_request_ids
                    .push(request.request_id.clone());
                save_background_state(&state_path, &state)?;
                let _ = trace_session.log_requested(
                    "background.approval",
                    Some(serde_json::json!({
                        "agent_id": agent_id,
                        "request_id": request.request_id,
                        "action_kind": request.action.kind()
                    })),
                );
            }
            None
        } else {
            outcome = "executed".to_string();
            Some(
                execution
                    .execute_background_action(agent_id, session_id, &action)
                    .await,
            )
        }
    } else if matches!(background.mode, BackgroundMode::Reasoning) && allow_reasoning {
        outcome = "executed".to_string();
        let kickoff = background_kickoff_prompt(&reason, &reevaluation);
        Some(
            execution
                .spawn_agent_once(agent_id, &kickoff, session_id, None, false, None)
                .await
                .map(|spawn| spawn.assistant_reply.unwrap_or_default()),
        )
    } else {
        None
    };

    state.last_wake_at = Some(now.to_rfc3339());
    state.last_wake_reason = Some(reason.clone());
    state.last_result = Some(outcome.clone());
    state.next_due_at = Some((now + Duration::seconds(effective_interval as i64)).to_rfc3339());
    state
        .pending_wake_fingerprints
        .retain(|existing| existing != &fingerprint);
    state
        .active_session_ids
        .retain(|existing| existing != session_id);
    super::decision::mark_reason_processed(&mut state, &reason);

    match result {
        Some(Ok(result_body)) => {
            state.retry_not_before = None;
            state.approval_blocked = false;
            clear_reevaluation_after_success(agent_dir, &reason)?;
            save_background_state(&state_path, &state)?;
            let _ = trace_session.log_completed(
                "background.wake",
                None,
                Some(serde_json::json!({
                    "agent_id": agent_id,
                    "reason": &reason,
                    "result_len": result_body.len()
                })),
            );
        }
        Some(Err(error)) => {
            let retry_after = now + Duration::seconds(effective_interval as i64);
            state.retry_not_before = Some(retry_after.to_rfc3339());
            save_background_state(&state_path, &state)?;
            persist_reevaluation_state(agent_dir, |state| {
                state.last_outcome = Some(format!("background_error:{error}"));
                state.retry_not_before = Some(retry_after.to_rfc3339());
            })?;
            let _ = trace_session.log_failed(
                "background.wake",
                &error.to_string(),
                Some(serde_json::json!({
                    "agent_id": agent_id,
                    "reason": &reason,
                })),
            );
        }
        None => {
            save_background_state(&state_path, &state)?;
            let _ = trace_session.log_skipped(
                "background.wake",
                "no_executable_work",
                Some(serde_json::json!({
                    "agent_id": agent_id,
                    "reason": &reason,
                })),
            );
        }
    }

    Ok(())
}
