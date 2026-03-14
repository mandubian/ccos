//! Approval resolution for the background scheduler.
//! Handles loading, approving, and rejecting approval requests.
//!
//! When an agent.install approval is resolved, the gateway automatically
//! resumes the caller's session with the approval result so they can
//! retry with the install_approval_ref.

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
    let decision = decide_request(
        config,
        request_id,
        decided_by,
        reason,
        ApprovalStatus::Approved,
    )?;
    
    // Resume the caller's session so they can retry with install_approval_ref
    resume_session_after_approval(config, &decision)?;
    
    Ok(decision)
}

pub fn reject_request(
    config: &GatewayConfig,
    request_id: &str,
    decided_by: &str,
    reason: Option<String>,
) -> anyhow::Result<ApprovalDecision> {
    let decision = decide_request(
        config,
        request_id,
        decided_by,
        reason,
        ApprovalStatus::Rejected,
    )?;
    
    // Resume the caller's session so they can handle the rejection
    resume_session_after_approval(config, &decision)?;
    
    Ok(decision)
}

/// Resume a session after approval/rejection by sending an event to the gateway.
/// This allows the caller (e.g., planner) to automatically continue without
/// requiring the user to type "approved" in chat.
fn resume_session_after_approval(
    config: &GatewayConfig,
    decision: &ApprovalDecision,
) -> anyhow::Result<()> {
    use crate::router::JsonRpcRequest;
    
    // Only resume for agent_install actions - these have a caller waiting
    if !matches!(&decision.action, autonoetic_types::background::ScheduledAction::AgentInstall { .. }) {
        return Ok(());
    }
    
    let session_id = &decision.session_id;
    if session_id.is_empty() {
        return Ok(());
    }
    
    tracing::info!(
        target: "approval",
        request_id = %decision.request_id,
        session_id = %session_id,
        status = ?decision.status,
        "Resuming session after approval resolution"
    );
    
    // Build a synthetic message that the gateway will route to the waiting agent
    let status_str = match decision.status {
        ApprovalStatus::Approved => "approved",
        ApprovalStatus::Rejected => "rejected",
    };
    
    let message = match &decision.action {
        autonoetic_types::background::ScheduledAction::AgentInstall { agent_id, .. } => {
            serde_json::json!({
                "type": "approval_resolved",
                "request_id": decision.request_id,
                "agent_id": agent_id,
                "status": status_str,
                "message": format!(
                    "The approval for installing agent '{}' has been {}. You can now retry agent.install with install_approval_ref='{}'.",
                    agent_id, status_str, decision.request_id
                )
            }).to_string()
        }
        _ => format!("Approval {} for request {}", status_str, decision.request_id),
    };
    
    // Clone data needed for the thread
    let request_id = decision.request_id.clone();
    let session_id_owned = session_id.to_string();
    
    // Create the JSON-RPC event to send to the gateway
    // Note: We do NOT specify target_agent_id - let the router resolve it from session binding
    // The session binding points to the planner (who spawned specialized_builder)
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: format!("approval-resume-{}", request_id),
        method: "event.ingest".to_string(),
        params: serde_json::json!({
            "event_type": "chat",
            "message": message,
            "session_id": session_id_owned,
            "metadata": {
                "sender_id": "gateway-approval",
                "channel_id": format!("gateway-approval-{}", session_id),
                "approval_resume": true,
                "approval_request_id": request_id,
                "approval_status": status_str,
            }
            // NOTE: No target_agent_id here - router resolves from session binding
        }),
    };
    
    // Connect to gateway's JSON-RPC server and send the event
    let gateway_addr = format!("127.0.0.1:{}", config.port);
    let addr = gateway_addr.clone();
    
    // Use a blocking task to send the resume event
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new();
        match rt {
            Ok(runtime) => {
                runtime.block_on(async {
                    match tokio::net::TcpStream::connect(&addr).await {
                        Ok(stream) => {
                            use tokio::io::{AsyncWriteExt, BufWriter};
                            use tokio::io::AsyncBufReadExt;
                            
                            let (read_half, write_half) = stream.into_split();
                            let mut writer = BufWriter::new(write_half);
                            let mut reader = tokio::io::BufReader::new(read_half);
                            
                            let encoded = serde_json::to_string(&request).unwrap_or_default();
                            if let Err(e) = writer.write_all(encoded.as_bytes()).await {
                                tracing::warn!(
                                    target: "approval",
                                    error = %e,
                                    "Failed to write approval resume to gateway"
                                );
                                return;
                            }
                            if let Err(e) = writer.write_all(b"\n").await {
                                tracing::warn!(
                                    target: "approval",
                                    error = %e,
                                    "Failed to write newline to gateway"
                                );
                                return;
                            }
                            if let Err(e) = writer.flush().await {
                                tracing::warn!(
                                    target: "approval",
                                    error = %e,
                                    "Failed to flush to gateway"
                                );
                                return;
                            }
                            
                            // Read response (don't block forever)
                            let mut response_line = String::new();
                            let read_result = tokio::time::timeout(
                                std::time::Duration::from_secs(5),
                                reader.read_line(&mut response_line)
                            ).await;
                            
                            match read_result {
                                Ok(Ok(_)) => {
                                    tracing::info!(
                                        target: "approval",
                                        request_id = %request_id,
                                        "Session resume sent successfully"
                                    );
                                }
                                Ok(Err(e)) => {
                                    tracing::warn!(
                                        target: "approval",
                                        error = %e,
                                        "Failed to read response from gateway"
                                    );
                                }
                                Err(_) => {
                                    tracing::debug!(
                                        target: "approval",
                                        "Gateway response timeout (non-fatal)"
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                target: "approval",
                                error = %e,
                                gateway_addr = %gateway_addr,
                                "Failed to connect to gateway for session resume"
                            );
                        }
                    }
                });
            }
            Err(e) => {
                tracing::warn!(
                    target: "approval",
                    error = %e,
                    "Failed to create runtime for session resume"
                );
            }
        }
    });
    
    Ok(())
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
        crate::runtime::reevaluation_state::persist_reevaluation_state(&agent_dir, |state| {
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
