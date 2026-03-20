//! Approval resolution for the background scheduler.
//! Handles loading, approving, and rejecting approval requests.
//!
//! When an agent.install approval is resolved, the gateway automatically
//! executes the approved install using the stored payload.

use crate::execution::{gateway_actor_id, gateway_root_dir, init_gateway_causal_logger};
use crate::policy::PolicyEngine;
use crate::runtime::tools::{AgentInstallTool, NativeTool};
use crate::tracing::{EventScope, SessionId, TraceSession};
use autonoetic_types::agent::AgentManifest;
use autonoetic_types::background::{
    ApprovalDecision, ApprovalRequest, ApprovalStatus, ScheduledAction,
};
use autonoetic_types::config::GatewayConfig;
use std::sync::Arc;

pub use super::store::{
    approved_approvals_dir, pending_approvals_dir, read_json_file, rejected_approvals_dir,
    write_json_file,
};

/// Load approval request JSON files from the pending directory.
///
/// Skips `*_payload.json` companion files (install payload blobs) so listing does not fail
/// deserialization.
pub fn load_approval_requests(config: &GatewayConfig) -> anyhow::Result<Vec<ApprovalRequest>> {
    let dir = pending_approvals_dir(config);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        if !entry.path().is_file() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.ends_with("_payload.json") {
            continue;
        }
        out.push(read_json_file::<ApprovalRequest>(&entry.path())?);
    }
    Ok(out)
}

/// Pending approvals whose [`ApprovalRequest::session_id`] shares the same root session as
/// `root_session_id` (see [`crate::runtime::content_store::root_session_id`]).
pub fn pending_approval_requests_for_root(
    config: &GatewayConfig,
    root_session_id: &str,
) -> anyhow::Result<Vec<ApprovalRequest>> {
    let all = load_approval_requests(config)?;
    Ok(all
        .into_iter()
        .filter(|r| {
            crate::runtime::content_store::root_session_id(&r.session_id) == root_session_id
        })
        .collect())
}

/// Pending [`ScheduledAction::SandboxExec`] approvals for an exact `session_id` (e.g. child
/// delegation path), oldest first. Used to stop repeated `sandbox.exec` calls from minting many
/// `apr-*` rows while an approval is still open.
pub fn pending_sandbox_exec_requests_for_session(
    config: &GatewayConfig,
    session_id: &str,
) -> anyhow::Result<Vec<ApprovalRequest>> {
    if session_id.is_empty() {
        return Ok(Vec::new());
    }
    let mut v: Vec<ApprovalRequest> = load_approval_requests(config)?
        .into_iter()
        .filter(|r| r.session_id == session_id)
        .filter(|r| matches!(r.action, ScheduledAction::SandboxExec { .. }))
        .collect();
    v.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    Ok(v)
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

    // Directly execute the approved action (install or write).
    // This ensures the action completes even if the caller session has ended.
    let install_result = execute_approved_action(config, &decision);

    // ALWAYS notify the waiting session so the LLM knows the agent is available
    // and can use it. This enables the LLM to automatically use the new agent.
    if let Err(e) = &install_result {
        tracing::warn!(
            target: "approval",
            request_id = %decision.request_id,
            error = %e,
            "Failed to execute approved action directly; agent may need manual retry"
        );
    }

    // Always resume session to notify LLM - either with success or error info
    if let Err(e) = resume_session_after_approval(config, &decision, install_result.is_ok()) {
        tracing::warn!(
            target: "approval",
            request_id = %decision.request_id,
            error = %e,
            "Failed to send session resume notification"
        );
    }

    // Unblock the task in the workflow (if bound to one)
    unblock_task_on_approval(config, &decision);

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
    resume_session_after_approval(config, &decision, false)?;

    // Unblock the task in the workflow (marks as Failed)
    unblock_task_on_approval(config, &decision);

    Ok(decision)
}

/// Queue durable session notifications after approval/rejection.
///
/// This function persists approval-resolution signals that are consumed by
/// gateway-owned delivery loops and/or channel clients (for example, the TUI
/// chat client resuming on its own connection).
///
/// `install_succeeded` indicates whether the auto-install succeeded (if applicable).
fn resume_session_after_approval(
    config: &GatewayConfig,
    decision: &ApprovalDecision,
    install_succeeded: bool,
) -> anyhow::Result<()> {
    // Resume for agent_install and sandbox_exec actions - both have a caller waiting
    let is_supported_action = matches!(
        &decision.action,
        autonoetic_types::background::ScheduledAction::AgentInstall { .. }
            | autonoetic_types::background::ScheduledAction::SandboxExec { .. }
    );

    if !is_supported_action {
        tracing::warn!(
            target: "approval",
            request_id = %decision.request_id,
            action = ?std::mem::discriminant(&decision.action),
            "Unsupported action type for auto-execute"
        );
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

    // Extract agent_id and build status message based on action type
    let (agent_id, status_message) = match &decision.action {
        autonoetic_types::background::ScheduledAction::AgentInstall { agent_id, .. } => {
            let msg = if install_succeeded {
                format!(
                    "Agent '{}' has been approved and installed successfully. You can now use it with agent.spawn('{}', message='...').",
                    agent_id, agent_id
                )
            } else {
                format!(
                    "The approval for installing agent '{}' has been {}. Retry agent.install with install_approval_ref='{}'.",
                    agent_id, status_str, decision.request_id
                )
            };
            (agent_id.clone(), msg)
        }
        autonoetic_types::background::ScheduledAction::SandboxExec { command, .. } => {
            let msg = if decision.status == ApprovalStatus::Approved {
                format!(
                    "Sandbox execution approved! Retry sandbox.exec with the SAME command PLUS approval_ref='{}' to execute:\n  Command: {}",
                    decision.request_id, command
                )
            } else {
                format!(
                    "Sandbox execution was rejected. Request: {}",
                    decision.request_id
                )
            };
            // Use the decision-level requester id to avoid brittle parsing of
            // nested session names.
            (decision.agent_id.clone(), msg)
        }
        _ => (
            "unknown".to_string(),
            format!(
                "Approval {} for request {}",
                status_str, decision.request_id
            ),
        ),
    };

    // Write signal file for CLI to detect (enables auto-resume)
    let signal = super::signal::Signal::ApprovalResolved {
        request_id: decision.request_id.clone(),
        agent_id: agent_id.clone(),
        status: status_str.to_string(),
        install_completed: install_succeeded,
        message: status_message.clone(),
        timestamp: chrono::Utc::now().to_rfc3339(),
    };

    // Write signal to the child session (the original waiting runtime).
    if let Err(e) = super::signal::write_signal(
        &config.agents_dir.join(".gateway"),
        session_id,
        &decision.request_id,
        &signal,
    ) {
        tracing::warn!(
            target: "approval",
            request_id = %decision.request_id,
            error = %e,
            "Failed to write signal file"
        );
    }

    // Session ID format: "parent_session/child_agent-uuid"
    let parent_session_id = session_id.split('/').next().unwrap_or(session_id);

    // IMPORTANT: for SandboxExec approvals, do NOT notify the parent planner
    // session. The approved command is bound to the waiting child session.
    // Notifying the parent causes planner-level re-delegation loops.
    let notify_parent = should_notify_parent_session(&decision.action, session_id);

    if notify_parent {
        tracing::info!(
            target: "approval",
            parent_session = %parent_session_id,
            "Also notifying parent session of approval resolution"
        );

        // Write signal to parent session too
        let parent_signal = super::signal::Signal::ApprovalResolved {
            request_id: decision.request_id.clone(),
            agent_id: agent_id.clone(),
            status: status_str.to_string(),
            install_completed: install_succeeded,
            message: status_message.clone(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };

        if let Err(e) = super::signal::write_signal(
            &config.agents_dir.join(".gateway"),
            parent_session_id,
            &decision.request_id,
            &parent_signal,
        ) {
            tracing::warn!(
                target: "approval",
                request_id = %decision.request_id,
                parent_session = %parent_session_id,
                error = %e,
                "Failed to write signal file to parent session"
            );
        }
    }

    // Delivery ownership is gateway-side and durable:
    // this function only persists signals. Gateway pollers and channel-specific
    // consumers (such as the chat TUI on its own socket) perform delivery + ack.
    let target_session = if notify_parent {
        format!("{},{}", session_id, parent_session_id)
    } else {
        session_id.to_string()
    };
    tracing::info!(
        target: "approval",
        request_id = %decision.request_id,
        target_session = %target_session,
        "Approval notification queued for gateway-owned delivery"
    );

    Ok(())
}

fn should_notify_parent_session(action: &ScheduledAction, session_id: &str) -> bool {
    matches!(action, ScheduledAction::AgentInstall { .. }) && session_id.contains('/')
}

/// On approval resolution, update the blocked task's status and emit workflow events.
fn unblock_task_on_approval(config: &GatewayConfig, decision: &ApprovalDecision) {
    let (Some(wf_id), Some(t_id)) = (&decision.workflow_id, &decision.task_id) else {
        return;
    };
    let new_status = match decision.status {
        ApprovalStatus::Approved => autonoetic_types::workflow::TaskRunStatus::Runnable,
        ApprovalStatus::Rejected => autonoetic_types::workflow::TaskRunStatus::Failed,
    };
    if let Err(e) = super::workflow_store::update_task_run_status(
        config,
        wf_id,
        t_id,
        new_status,
        Some(format!(
            "approval_{}",
            match decision.status {
                ApprovalStatus::Approved => "approved",
                ApprovalStatus::Rejected => "rejected",
            }
        )),
    ) {
        tracing::warn!(
            target: "approval",
            workflow_id = %wf_id,
            task_id = %t_id,
            error = %e,
            "Failed to unblock task on approval resolution"
        );
    } else {
        tracing::info!(
            target: "approval",
            workflow_id = %wf_id,
            task_id = %t_id,
            status = ?decision.status,
            "Task unblocked after approval resolution"
        );
    }
}

/// Execute the approved action (agent.install or file write).
fn execute_approved_action(
    config: &GatewayConfig,
    decision: &ApprovalDecision,
) -> anyhow::Result<()> {
    match &decision.action {
        autonoetic_types::background::ScheduledAction::AgentInstall { .. } => {
            execute_approved_install(config, decision)
        }
        autonoetic_types::background::ScheduledAction::WriteFile { .. } => {
            execute_approved_write(config, decision)
        }
        _ => {
            tracing::warn!(
                target: "approval",
                request_id = %decision.request_id,
                "Unsupported action type for auto-execute"
            );
            Ok(())
        }
    }
}

/// Execute an approved WriteFile action.
fn execute_approved_write(
    config: &GatewayConfig,
    decision: &ApprovalDecision,
) -> anyhow::Result<()> {
    let ScheduledAction::WriteFile { path, content, .. } = &decision.action else {
        return Ok(());
    };

    // Load the stored payload (created when the write was requested)
    let payload_path =
        pending_approvals_dir(config).join(format!("{}_payload.json", decision.request_id));

    let (write_path, write_content) = if payload_path.exists() {
        // Use stored payload
        let payload_json = std::fs::read_to_string(&payload_path)?;
        let payload: serde_json::Value = serde_json::from_str(&payload_json)
            .map_err(|e| anyhow::anyhow!("Failed to parse stored payload: {}", e))?;

        let p = payload
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or(path)
            .to_string();
        let c = payload
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or(content)
            .to_string();
        (p, c)
    } else {
        (path.clone(), content.clone())
    };

    // Find the agent directory from the request
    let agent_dir = config.agents_dir.join(&decision.agent_id);

    tracing::info!(
        target: "approval",
        request_id = %decision.request_id,
        path = %write_path,
        agent_id = %decision.agent_id,
        "Executing approved file write"
    );

    // Write the skill file
    let target_path = agent_dir.join(&write_path);
    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&target_path, &write_content)?;

    // Log completion to causal chain
    let causal_logger = init_gateway_causal_logger(config)?;
    let mut trace_session = TraceSession::create_with_session_id(
        SessionId::from_string(decision.session_id.clone()),
        Arc::new(causal_logger),
        gateway_actor_id(),
        EventScope::Session,
    );
    let _ = trace_session.log_completed(
        "file.write.completed",
        Some("approved"),
        Some(serde_json::json!({
            "path": write_path,
            "request_id": decision.request_id,
            "executed_by": "gateway_auto_complete",
        })),
    );

    // Cleanup payload
    let _ = std::fs::remove_file(&payload_path);

    tracing::info!(
        target: "approval",
        request_id = %decision.request_id,
        path = %write_path,
        "Successfully auto-completed approved file write"
    );

    Ok(())
}

/// Directly execute an approved agent.install action using the stored payload.
/// This reuses the AgentInstallTool by adding install_approval_ref and calling it.
fn execute_approved_install(
    config: &GatewayConfig,
    decision: &ApprovalDecision,
) -> anyhow::Result<()> {
    use autonoetic_types::agent::{AgentIdentity, RuntimeDeclaration};
    use autonoetic_types::background::ScheduledAction;

    // Only handle agent_install actions
    let ScheduledAction::AgentInstall { agent_id, .. } = &decision.action else {
        return Ok(());
    };

    // Load the stored install payload (created when approval was first requested)
    let payload_path =
        pending_approvals_dir(config).join(format!("{}_payload.json", decision.request_id));
    if !payload_path.exists() {
        return Err(anyhow::anyhow!(
            "No stored payload found at {} for request {}; cannot auto-complete install",
            payload_path.display(),
            decision.request_id
        ));
    }

    // Use the requested agent's directory as the parent (e.g., specialized_builder.default)
    let parent_agent_id = &decision.agent_id;
    let parent_dir = config.agents_dir.join(parent_agent_id);

    // Modify the stored payload to include install_approval_ref
    let payload_json = std::fs::read_to_string(&payload_path)?;
    let mut args_value: serde_json::Value = serde_json::from_str(&payload_json)
        .map_err(|e| anyhow::anyhow!("Failed to parse stored install payload: {}", e))?;

    // Add promotion_gate with install_approval_ref if not present
    if let Some(obj) = args_value.as_object_mut() {
        if !obj.contains_key("promotion_gate") {
            obj.insert(
                "promotion_gate".to_string(),
                serde_json::json!({
                    "evaluator_pass": true,
                    "auditor_pass": true,
                    "install_approval_ref": decision.request_id
                }),
            );
        } else if let Some(gate) = obj.get_mut("promotion_gate") {
            if let Some(gate_obj) = gate.as_object_mut() {
                gate_obj.insert(
                    "install_approval_ref".to_string(),
                    serde_json::Value::String(decision.request_id.clone()),
                );
            }
        }
    }

    let arguments_json = args_value.to_string();

    tracing::info!(
        target: "approval",
        request_id = %decision.request_id,
        agent_id = %agent_id,
        "Executing approved install directly via AgentInstallTool"
    );

    // Build minimal parent manifest for the tool
    let parent_manifest = AgentManifest {
        version: "1.0".to_string(),
        runtime: RuntimeDeclaration {
            engine: "autonoetic".to_string(),
            gateway_version: "0.1.0".to_string(),
            sdk_version: "0.1.0".to_string(),
            runtime_type: "stateful".to_string(),
            sandbox: "bubblewrap".to_string(),
            runtime_lock: "runtime.lock".to_string(),
        },
        agent: AgentIdentity {
            id: parent_agent_id.clone(),
            name: parent_agent_id.clone(),
            description: format!("Gateway auto-complete parent for {}", decision.request_id),
        },
        capabilities: vec![],
        llm_config: None,
        limits: None,
        background: None,
        disclosure: None,
        io: None,
        middleware: None,
        execution_mode: autonoetic_types::agent::ExecutionMode::Reasoning,
        script_entry: None,
        gateway_url: None,
        gateway_token: None,
    };

    // Execute the install using the existing tool
    let tool = AgentInstallTool;
    let agent_dir = &parent_dir;
    // Must match `ExecutionContext` / `ToolRegistry`: artifacts and content live under
    // `agents_dir/.gateway/`, not `agents_dir.parent()`.
    let gateway_path = gateway_root_dir(config);
    let policy = PolicyEngine::new(parent_manifest.clone());

    match tool.execute(
        &parent_manifest,
        &policy,
        agent_dir,
        Some(gateway_path.as_path()),
        &arguments_json,
        Some(&decision.session_id),
        None,
        Some(config),
    ) {
        Ok(result) => {
            tracing::info!(
                target: "approval",
                request_id = %decision.request_id,
                agent_id = %agent_id,
                result = %result,
                "Successfully auto-completed approved agent.install"
            );

            // Log completion to causal chain
            let causal_logger = init_gateway_causal_logger(config)?;
            let mut trace_session = TraceSession::create_with_session_id(
                SessionId::from_string(decision.session_id.clone()),
                Arc::new(causal_logger),
                gateway_actor_id(),
                EventScope::Session,
            );
            let _ = trace_session.log_completed(
                "agent.install.completed",
                Some("approved"),
                Some(serde_json::json!({
                    "agent_id": agent_id,
                    "request_id": decision.request_id,
                    "executed_by": "gateway_auto_complete",
                })),
            );

            // Cleanup payload on success
            let _ = std::fs::remove_file(&payload_path);

            Ok(())
        }
        Err(e) => Err(anyhow::anyhow!("Failed to execute approved install: {}", e)),
    }
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
        workflow_id: request.workflow_id.clone(),
        task_id: request.task_id.clone(),
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

#[cfg(test)]
mod tests {
    use super::should_notify_parent_session;
    use crate::scheduler::pending_approvals_dir;
    use autonoetic_types::background::{ApprovalRequest, ScheduledAction};
    use autonoetic_types::config::GatewayConfig;
    use tempfile::tempdir;

    #[test]
    fn load_approval_requests_skips_payload_companion_files() {
        let dir = tempdir().unwrap();
        let agents_dir = dir.path().join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();
        let cfg = GatewayConfig {
            agents_dir: agents_dir.clone(),
            ..Default::default()
        };
        let pending = pending_approvals_dir(&cfg);
        std::fs::create_dir_all(&pending).unwrap();

        let req = ApprovalRequest {
            request_id: "apr-test1234".to_string(),
            agent_id: "coder.default".to_string(),
            session_id: "root-session/coder-abc".to_string(),
            action: ScheduledAction::SandboxExec {
                command: "python3 x".to_string(),
                dependencies: None,
                requires_approval: true,
                evidence_ref: None,
            },
            created_at: "2020-01-01T00:00:00Z".to_string(),
            reason: None,
            evidence_ref: None,
            workflow_id: None,
            task_id: None,
        };
        std::fs::write(
            pending.join("apr-test1234.json"),
            serde_json::to_string(&req).unwrap(),
        )
        .unwrap();
        std::fs::write(
            pending.join("apr-test1234_payload.json"),
            r#"{"not":"approval_request"}"#,
        )
        .unwrap();

        let loaded = super::load_approval_requests(&cfg).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].request_id, "apr-test1234");
    }

    #[test]
    fn pending_approval_requests_for_root_filters_by_session() {
        let dir = tempdir().unwrap();
        let agents_dir = dir.path().join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();
        let cfg = GatewayConfig {
            agents_dir: agents_dir.clone(),
            ..Default::default()
        };
        let pending = pending_approvals_dir(&cfg);
        std::fs::create_dir_all(&pending).unwrap();

        let req = |id: &str, sess: &str| ApprovalRequest {
            request_id: id.to_string(),
            agent_id: "a".to_string(),
            session_id: sess.to_string(),
            action: ScheduledAction::SandboxExec {
                command: "c".to_string(),
                dependencies: None,
                requires_approval: true,
                evidence_ref: None,
            },
            created_at: "2020-01-01T00:00:00Z".to_string(),
            reason: None,
            evidence_ref: None,
            workflow_id: None,
            task_id: None,
        };
        std::fs::write(
            pending.join("apr-a.json"),
            serde_json::to_string(&req("apr-a", "root-a/coder-1")).unwrap(),
        )
        .unwrap();
        std::fs::write(
            pending.join("apr-b.json"),
            serde_json::to_string(&req("apr-b", "root-b/coder-1")).unwrap(),
        )
        .unwrap();

        let for_a = super::pending_approval_requests_for_root(&cfg, "root-a").unwrap();
        assert_eq!(for_a.len(), 1);
        assert_eq!(for_a[0].request_id, "apr-a");
    }

    #[test]
    fn pending_sandbox_exec_requests_for_session_filters_and_sorts() {
        let dir = tempdir().unwrap();
        let agents_dir = dir.path().join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();
        let cfg = GatewayConfig {
            agents_dir: agents_dir.clone(),
            ..Default::default()
        };
        let pending = pending_approvals_dir(&cfg);
        std::fs::create_dir_all(&pending).unwrap();

        let req = |id: &str, created: &str| ApprovalRequest {
            request_id: id.to_string(),
            agent_id: "evaluator.default".to_string(),
            session_id: "sess/evaluator-1".to_string(),
            action: ScheduledAction::SandboxExec {
                command: "python3 x".to_string(),
                dependencies: None,
                requires_approval: true,
                evidence_ref: None,
            },
            created_at: created.to_string(),
            reason: None,
            evidence_ref: None,
            workflow_id: None,
            task_id: None,
        };
        std::fs::write(
            pending.join("apr-second.json"),
            serde_json::to_string(&req("apr-second", "2020-01-02T00:00:00Z")).unwrap(),
        )
        .unwrap();
        std::fs::write(
            pending.join("apr-first.json"),
            serde_json::to_string(&req("apr-first", "2020-01-01T00:00:00Z")).unwrap(),
        )
        .unwrap();
        // Install-style request same session — must not appear in sandbox-only list
        let install = ApprovalRequest {
            request_id: "apr-install".to_string(),
            agent_id: "b".to_string(),
            session_id: "sess/evaluator-1".to_string(),
            action: ScheduledAction::AgentInstall {
                agent_id: "x".to_string(),
                summary: "s".to_string(),
                requested_by_agent_id: "y".to_string(),
                install_fingerprint: "fp".to_string(),
            },
            created_at: "2019-01-01T00:00:00Z".to_string(),
            reason: None,
            evidence_ref: None,
            workflow_id: None,
            task_id: None,
        };
        std::fs::write(
            pending.join("apr-install.json"),
            serde_json::to_string(&install).unwrap(),
        )
        .unwrap();

        let list =
            super::pending_sandbox_exec_requests_for_session(&cfg, "sess/evaluator-1").unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].request_id, "apr-first");
        assert_eq!(list[1].request_id, "apr-second");
    }

    #[test]
    fn test_should_notify_parent_session_for_agent_install_nested_session() {
        let action = ScheduledAction::AgentInstall {
            agent_id: "specialist.weather".to_string(),
            summary: "install specialist.weather".to_string(),
            requested_by_agent_id: "specialized_builder.default".to_string(),
            install_fingerprint: "sha256:abc123".to_string(),
        };
        assert!(should_notify_parent_session(
            &action,
            "demo-session/specialized_builder.default-abcd1234"
        ));
    }

    #[test]
    fn test_should_not_notify_parent_session_for_sandbox_exec() {
        let action = ScheduledAction::SandboxExec {
            command: "python3 /tmp/weather.py".to_string(),
            dependencies: None,
            requires_approval: true,
            evidence_ref: None,
        };
        assert!(!should_notify_parent_session(
            &action,
            "demo-session/coder.default-6738ac56"
        ));
    }
}
