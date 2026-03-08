//! Approval resolution for the background scheduler.
//! Handles loading, approving, and rejecting approval requests.

use crate::execution::{gateway_actor_id, init_gateway_causal_logger};
use crate::tracing::{EventScope, SessionId, TraceSession};
use autonoetic_types::background::{ApprovalDecision, ApprovalRequest, ApprovalStatus};
use autonoetic_types::config::GatewayConfig;
use std::sync::Arc;

pub use super::store::{
    approved_approvals_dir, load_json_dir, pending_approvals_dir, read_json_file,
    rejected_approvals_dir, write_json_file,
};

pub fn load_approval_requests(config: &GatewayConfig) -> anyhow::Result<Vec<ApprovalRequest>> {
    load_json_dir::<ApprovalRequest>(&pending_approvals_dir(config))
}

pub fn approve_request(
    config: &GatewayConfig,
    request_id: &str,
    decided_by: &str,
    reason: Option<String>,
) -> anyhow::Result<ApprovalDecision> {
    decide_request(
        config,
        request_id,
        decided_by,
        reason,
        ApprovalStatus::Approved,
    )
}

pub fn reject_request(
    config: &GatewayConfig,
    request_id: &str,
    decided_by: &str,
    reason: Option<String>,
) -> anyhow::Result<ApprovalDecision> {
    decide_request(
        config,
        request_id,
        decided_by,
        reason,
        ApprovalStatus::Rejected,
    )
}

fn decide_request(
    config: &GatewayConfig,
    request_id: &str,
    decided_by: &str,
    reason: Option<String>,
    status: ApprovalStatus,
) -> anyhow::Result<ApprovalDecision> {
    let request_path = pending_approvals_dir(config).join(format!("{request_id}.json"));
    let request: ApprovalRequest = read_json_file(&request_path)?;
    let decision = ApprovalDecision {
        request_id: request.request_id,
        agent_id: request.agent_id,
        session_id: request.session_id,
        action: request.action,
        status: status.clone(),
        decided_at: chrono::Utc::now().to_rfc3339(),
        decided_by: decided_by.to_string(),
        reason,
    };
    let target_dir = match status {
        ApprovalStatus::Approved => approved_approvals_dir(config),
        ApprovalStatus::Rejected => rejected_approvals_dir(config),
    };
    write_json_file(
        &target_dir.join(format!("{}.json", decision.request_id)),
        &decision,
    )?;
    std::fs::remove_file(request_path)?;

    let background_session_id = super::decision::background_session_id;
    let load_background_state = super::store::load_background_state;
    let save_background_state = super::store::save_background_state;

    if matches!(status, ApprovalStatus::Rejected) {
        let agent_dir = config.agents_dir.join(&decision.agent_id);
        crate::runtime::lifecycle::persist_reevaluation_state(&agent_dir, |state| {
            state
                .open_approval_request_ids
                .retain(|existing| existing != &decision.request_id);
            state.pending_scheduled_action = None;
            state.last_outcome = Some("approval_rejected".to_string());
        })?;
        let state_path = super::store::background_state_path(config, &decision.agent_id);
        let mut background_state = load_background_state(
            &state_path,
            &decision.agent_id,
            &background_session_id(&decision.agent_id),
        )?;
        background_state.approval_blocked = false;
        background_state
            .pending_approval_request_ids
            .retain(|existing| existing != &decision.request_id);
        background_state
            .processed_approval_request_ids
            .push(decision.request_id.clone());
        save_background_state(&state_path, &background_state)?;
    }

    let causal_logger = init_gateway_causal_logger(config)?;
    let mut trace_session = TraceSession::create_with_session_id(
        SessionId::from_string(decision.session_id.clone()),
        Arc::new(causal_logger),
        gateway_actor_id(),
        EventScope::Session,
    );
    let action = match status {
        ApprovalStatus::Approved => "background.approval",
        ApprovalStatus::Rejected => "background.approval",
    };
    let status_str = match status {
        ApprovalStatus::Approved => "approved",
        ApprovalStatus::Rejected => "rejected",
    };
    let _ = trace_session.log_completed(
        action,
        Some(status_str),
        Some(serde_json::json!({
            "agent_id": decision.agent_id,
            "request_id": decision.request_id,
            "decided_by": decision.decided_by,
            "action_kind": decision.action.kind()
        })),
    );
    Ok(decision)
}
