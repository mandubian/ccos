//! Approval resolution for the background scheduler.
//! Handles loading, approving, and rejecting approval requests.
//!
//! When an agent.install approval is resolved, the gateway automatically
//! executes the approved install using the stored payload.

use crate::execution::{gateway_actor_id, gateway_root_dir, init_gateway_causal_logger};
use crate::policy::PolicyEngine;
use crate::runtime::tools::{AgentInstallTool, NativeTool, SandboxExecTool};
use crate::tracing::{EventScope, SessionId, TraceSession};
use autonoetic_types::agent::AgentManifest;
use autonoetic_types::background::{
    ApprovalDecision, ApprovalRequest, ApprovalStatus, ScheduledAction,
};
use autonoetic_types::config::GatewayConfig;
use std::sync::Arc;

/// Load approval requests from the gateway store for a specific session.
///
/// Fetches pending approval requests stored directly in the SQLite `GatewayStore`.
/// Returns an empty list if the gateway store is unavailable.
pub fn load_approval_requests(
    _config: &GatewayConfig,
    gateway_store: Option<&crate::scheduler::gateway_store::GatewayStore>,
) -> anyhow::Result<Vec<ApprovalRequest>> {
    if let Some(store) = gateway_store {
        store.get_pending_approvals()
    } else {
        // GatewayStore not available - return empty list instead of error
        Ok(Vec::new())
    }
}

/// Pending approvals whose [`ApprovalRequest::session_id`] shares the same root session as
/// `root_session_id` (see [`crate::runtime::content_store::root_session_id`]).
pub fn pending_approval_requests_for_root(
    config: &GatewayConfig,
    gateway_store: Option<&crate::scheduler::gateway_store::GatewayStore>,
    root_session_id: &str,
) -> anyhow::Result<Vec<ApprovalRequest>> {
    let all = load_approval_requests(config, gateway_store)?;
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
    gateway_store: Option<&crate::scheduler::gateway_store::GatewayStore>,
    session_id: &str,
) -> anyhow::Result<Vec<ApprovalRequest>> {
    if session_id.is_empty() {
        return Ok(Vec::new());
    }
    let mut v: Vec<ApprovalRequest> = load_approval_requests(config, gateway_store)?
        .into_iter()
        .filter(|r| r.session_id == session_id)
        .filter(|r| matches!(r.action, ScheduledAction::SandboxExec { .. }))
        .collect();
    v.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    Ok(v)
}

pub fn approve_request(
    config: &GatewayConfig,
    gateway_store: Option<&crate::scheduler::gateway_store::GatewayStore>,
    request_id: &str,
    decided_by: &str,
    reason: Option<String>,
) -> anyhow::Result<ApprovalDecision> {
    let decision = decide_request(
        config,
        gateway_store,
        request_id,
        decided_by,
        reason,
        ApprovalStatus::Approved,
    )?;

    // Directly execute the approved action (install, write, or sandbox exec).
    // This ensures the action completes even if the caller session has ended.
    let action_result = execute_approved_action(config, gateway_store, &decision);
    let (action_succeeded, exec_result) = match &action_result {
        Ok(value) => (true, value.clone()),
        Err(_) => (false, None),
    };

    // ALWAYS notify the waiting session so the LLM knows the agent is available
    // and can use it. This enables the LLM to automatically use the new agent.
    if let Err(e) = &action_result {
        tracing::warn!(
            target: "approval",
            request_id = %decision.request_id,
            error = %e,
            "Failed to execute approved action directly; agent may need manual retry"
        );
    }

    // Workflow-bound tasks already continue through the durable Runnable/Failed
    // transition plus checkpointed resume message. Sending a second approval-
    // resolved notification to the same child session would wake it twice.
    if should_resume_waiting_session(&decision) {
        if let Err(e) = resume_session_after_approval(
            config,
            gateway_store,
            &decision,
            action_succeeded,
            exec_result.clone(),
        ) {
            tracing::warn!(
                target: "approval",
                request_id = %decision.request_id,
                error = %e,
                "Failed to send session resume notification"
            );
        }
    } else {
        tracing::info!(
            target: "approval",
            request_id = %decision.request_id,
            workflow_id = ?decision.workflow_id,
            task_id = ?decision.task_id,
            "Skipping direct session resume; workflow-bound task will continue via durable re-queue"
        );
    }

    // Store the approval result in the task checkpoint so that when the workflow
    // re-queues after unblocking, it uses the exec result as the resume message
    // rather than the original task message (which causes the coder to restart fresh).
    store_approval_result_in_checkpoint(config, gateway_store, &decision, exec_result.as_deref());

    // Unblock the task in the workflow (if bound to one)
    unblock_task_on_approval(config, gateway_store, &decision);

    Ok(decision)
}

pub fn reject_request(
    config: &GatewayConfig,
    gateway_store: Option<&crate::scheduler::gateway_store::GatewayStore>,
    request_id: &str,
    decided_by: &str,
    reason: Option<String>,
) -> anyhow::Result<ApprovalDecision> {
    let decision = decide_request(
        config,
        gateway_store,
        request_id,
        decided_by,
        reason,
        ApprovalStatus::Rejected,
    )?;

    // Workflow-bound tasks surface rejection through task failure + workflow
    // resume. Non-workflow callers still need a direct notification.
    if should_resume_waiting_session(&decision) {
        resume_session_after_approval(config, gateway_store, &decision, false, None)?;
    } else {
        tracing::info!(
            target: "approval",
            request_id = %decision.request_id,
            workflow_id = ?decision.workflow_id,
            task_id = ?decision.task_id,
            "Skipping direct rejection resume; workflow-bound task will continue via workflow failure"
        );
    }

    // Unblock the task in the workflow (marks as Failed)
    unblock_task_on_approval(config, gateway_store, &decision);

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
    _config: &GatewayConfig,
    gateway_store: Option<&crate::scheduler::gateway_store::GatewayStore>,
    decision: &ApprovalDecision,
    action_succeeded: bool,
    exec_result: Option<String>,
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
            let msg = if action_succeeded {
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
                if let Some(result_json) = exec_result {
                    let parsed: serde_json::Value = serde_json::from_str(&result_json)
                        .unwrap_or_else(|_| {
                            serde_json::json!({
                                "ok": false,
                                "exit_code": serde_json::Value::Null,
                                "stdout": "",
                                "stderr": result_json,
                            })
                        });
                    let ok = parsed
                        .get("ok")
                        .and_then(|value| value.as_bool())
                        .unwrap_or(false);
                    let exit_code = parsed
                        .get("exit_code")
                        .and_then(|value| value.as_i64())
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    let stdout = parsed
                        .get("stdout")
                        .and_then(|value| value.as_str())
                        .unwrap_or("");
                    let stderr = parsed
                        .get("stderr")
                        .and_then(|value| value.as_str())
                        .unwrap_or("");
                    format!(
                        "Sandbox execution completed {}.\nCommand: {}\nExit code: {}\nStdout:\n{}\nStderr:\n{}",
                        if ok { "successfully" } else { "with errors" },
                        command,
                        exit_code,
                        stdout,
                        stderr,
                    )
                } else {
                    format!(
                        "Sandbox execution approved but auto-execution failed. Retry sandbox.exec with the SAME command PLUS approval_ref='{}' to execute:\n  Command: {}",
                        decision.request_id, command
                    )
                }
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

    // Write approval resolution signal to GatewayStore for scheduler delivery (enables auto-resume).
    let signal = super::signal::Signal::ApprovalResolved {
        request_id: decision.request_id.clone(),
        agent_id: agent_id.clone(),
        status: status_str.to_string(),
        install_completed: action_succeeded,
        message: status_message.clone(),
        timestamp: chrono::Utc::now().to_rfc3339(),
    };

    // Write signal to the child session (the original waiting runtime).
    // write_signal persists the record to GatewayStore for durable scheduler delivery.
    if let Err(e) = super::signal::write_signal(
        gateway_store.as_deref(),
        session_id,
        &decision.request_id,
        &signal,
    ) {
        tracing::warn!(
            target: "approval",
            request_id = %decision.request_id,
            error = %e,
            "Failed to write approval signal to store"
        );
    }

    // Session ID format: "parent_session/child_agent-uuid"
    let parent_session_id = session_id.split('/').next().unwrap_or(session_id);

    // For SandboxExec in a child session: the child has already returned (spawn completed)
    // with "blocked on approval". The child is no longer waiting. The parent (planner)
    // has the chat connection. We MUST notify the parent with the exec result so the
    // planner sees the approval outcome and can continue rather than restart from scratch.
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
            install_completed: action_succeeded,
            message: status_message.clone(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };

        // write_signal persists the record to GatewayStore for durable scheduler delivery.
        if let Err(e) = super::signal::write_signal(
            gateway_store.as_deref(),
            parent_session_id,
            &decision.request_id,
            &parent_signal,
        ) {
            tracing::warn!(
                target: "approval",
                request_id = %decision.request_id,
                parent_session = %parent_session_id,
                error = %e,
                "Failed to write approval signal to parent session store"
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
    let is_child_session = session_id.contains('/');
    match action {
        ScheduledAction::AgentInstall { .. } => is_child_session,
        // SandboxExec in child (e.g. evaluator): child already returned; parent has chat.
        // Parent must receive exec result so planner continues instead of restarting.
        ScheduledAction::SandboxExec { .. } => is_child_session,
        _ => false,
    }
}

fn should_resume_waiting_session(decision: &ApprovalDecision) -> bool {
    !(decision.workflow_id.is_some() && decision.task_id.is_some())
}

fn preview_for_resume(value: &str, max_chars: usize) -> (String, bool) {
    let total_chars = value.chars().count();
    if total_chars <= max_chars {
        return (value.to_string(), false);
    }
    let preview: String = value.chars().take(max_chars).collect();
    (preview, true)
}

/// Build a human-readable resume message for an approval decision.
///
/// This message is stored in the task checkpoint and used by the workflow scheduler
/// when re-queueing an approval-unblocked task, so the agent receives the exec result
/// and can continue from where it left off rather than restarting the task from scratch.
fn build_approval_resume_message(decision: &ApprovalDecision, exec_result: Option<&str>) -> String {
    let status_str = match decision.status {
        ApprovalStatus::Approved => "approved",
        ApprovalStatus::Rejected => "rejected",
    };

    let payload = match &decision.action {
        ScheduledAction::SandboxExec { command, .. } => {
            if decision.status == ApprovalStatus::Approved {
                if let Some(result_json) = exec_result {
                    let parsed: serde_json::Value = serde_json::from_str(result_json)
                        .unwrap_or_else(|_| {
                            serde_json::json!({
                                "ok": false,
                                "exit_code": serde_json::Value::Null,
                                "stdout": "",
                                "stderr": result_json,
                            })
                        });
                    let ok = parsed.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
                    let exit_code = parsed
                        .get("exit_code")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    let stdout = parsed.get("stdout").and_then(|v| v.as_str()).unwrap_or("");
                    let stderr = parsed.get("stderr").and_then(|v| v.as_str()).unwrap_or("");
                    let (stdout_preview, stdout_truncated) = preview_for_resume(stdout, 4096);
                    let (stderr_preview, stderr_truncated) = preview_for_resume(stderr, 4096);

                    serde_json::json!({
                        "type": "approval_resolved",
                        "request_id": decision.request_id,
                        // Keep backward-compatible alias for any consumer still reading approval_id.
                        "approval_id": decision.request_id,
                        "agent_id": decision.agent_id,
                        "status": "approved",
                        "action_kind": "sandbox_exec",
                        "command": command,
                        "execution": {
                            "ok": ok,
                            "exit_code": exit_code,
                            "stdout_preview": stdout_preview,
                            "stdout_truncated": stdout_truncated,
                            "stderr_preview": stderr_preview,
                            "stderr_truncated": stderr_truncated,
                        },
                        "message": format!(
                            "Sandbox execution completed {}.",
                            if ok { "successfully" } else { "with errors" }
                        ),
                    })
                } else {
                    serde_json::json!({
                        "type": "approval_resolved",
                        "request_id": decision.request_id,
                        "approval_id": decision.request_id,
                        "agent_id": decision.agent_id,
                        "status": "approved",
                        "action_kind": "sandbox_exec",
                        "command": command,
                        "message": format!(
                            "Sandbox execution approved but auto-execution failed. Retry sandbox.exec with approval_ref='{}'.",
                            decision.request_id
                        ),
                    })
                }
            } else {
                serde_json::json!({
                    "type": "approval_resolved",
                    "request_id": decision.request_id,
                    "approval_id": decision.request_id,
                    "agent_id": decision.agent_id,
                    "status": "rejected",
                    "action_kind": "sandbox_exec",
                    "command": command,
                    "message": format!("Sandbox execution for '{}' was rejected.", command),
                })
            }
        }
        _ => serde_json::json!({
            "type": "approval_resolved",
            "request_id": decision.request_id,
            "approval_id": decision.request_id,
            "agent_id": decision.agent_id,
            "status": status_str,
            "action_kind": decision.action.kind(),
        }),
    };

    serde_json::to_string(&payload).unwrap_or_else(|_| {
        format!(
            "{{\"type\":\"approval_resolved\",\"request_id\":\"{}\",\"status\":\"{}\"}}",
            decision.request_id, status_str
        )
    })
}

/// Store the approval exec result in the task checkpoint for workflow re-queue.
///
/// When `process_runnable_workflow_tasks` picks up the unblocked task, it will
/// check this checkpoint and use the stored `resume_message` instead of the
/// original task message, preventing the agent from restarting the task from scratch.
fn store_approval_result_in_checkpoint(
    config: &GatewayConfig,
    gateway_store: Option<&crate::scheduler::gateway_store::GatewayStore>,
    decision: &ApprovalDecision,
    exec_result: Option<&str>,
) {
    let (Some(wf_id), Some(t_id)) = (&decision.workflow_id, &decision.task_id) else {
        return;
    };
    let resume_message = build_approval_resume_message(decision, exec_result);
    let status_str = match decision.status {
        ApprovalStatus::Approved => "approved",
        ApprovalStatus::Rejected => "rejected",
    };
    if let Err(e) = super::workflow_store::checkpoint_task(
        config,
        gateway_store,
        wf_id,
        t_id,
        "approval_resolved".to_string(),
        serde_json::json!({
            "request_id": decision.request_id,
            "approval_id": decision.request_id,
            "status": status_str,
            "resume_message": resume_message,
        }),
    ) {
        tracing::warn!(
            target: "approval",
            workflow_id = %wf_id,
            task_id = %t_id,
            error = %e,
            "Failed to store approval result in task checkpoint"
        );
    } else {
        tracing::info!(
            target: "approval",
            workflow_id = %wf_id,
            task_id = %t_id,
            request_id = %decision.request_id,
            "Stored approval result in task checkpoint for workflow re-queue"
        );
    }
}

/// On approval resolution, update the blocked task's status and emit workflow events.
fn unblock_task_on_approval(
    config: &GatewayConfig,
    gateway_store: Option<&crate::scheduler::gateway_store::GatewayStore>,
    decision: &ApprovalDecision,
) {
    let (Some(wf_id), Some(t_id)) = (&decision.workflow_id, &decision.task_id) else {
        return;
    };
    let new_status = match decision.status {
        ApprovalStatus::Approved => autonoetic_types::workflow::TaskRunStatus::Runnable,
        ApprovalStatus::Rejected => autonoetic_types::workflow::TaskRunStatus::Failed,
    };
    if let Err(e) = super::workflow_store::update_task_run_status(
        config,
        gateway_store,
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
    gateway_store: Option<&crate::scheduler::gateway_store::GatewayStore>,
    decision: &ApprovalDecision,
) -> anyhow::Result<Option<String>> {
    match &decision.action {
        autonoetic_types::background::ScheduledAction::AgentInstall { .. } => {
            execute_approved_install(config, decision)?;
            Ok(None)
        }
        autonoetic_types::background::ScheduledAction::WriteFile { .. } => {
            execute_approved_write(config, decision)?;
            Ok(None)
        }
        autonoetic_types::background::ScheduledAction::SandboxExec { .. } => {
            let result = execute_approved_sandbox_exec(config, gateway_store, decision)?;
            Ok(Some(result))
        }
    }
}

/// Execute an approved SandboxExec action directly, bypassing LLM retry.
fn execute_approved_sandbox_exec(
    config: &GatewayConfig,
    gateway_store: Option<&crate::scheduler::gateway_store::GatewayStore>,
    decision: &ApprovalDecision,
) -> anyhow::Result<String> {
    let ScheduledAction::SandboxExec {
        command,
        dependencies,
        ..
    } = &decision.action
    else {
        anyhow::bail!("execute_approved_sandbox_exec called with non-SandboxExec action");
    };

    let repo = crate::agent::AgentRepository::from_config(config);
    let loaded = repo.get_sync(&decision.agent_id)?;
    let manifest = loaded.manifest;
    let agent_dir = loaded.dir;
    let gateway_dir = gateway_root_dir(config);
    let policy = PolicyEngine::new(manifest.clone());
    let approval_store = if gateway_store.is_some() {
        Some(std::sync::Arc::new(
            crate::scheduler::gateway_store::GatewayStore::open(&gateway_dir)?,
        ))
    } else {
        None
    };

    // Inject approval_ref so sandbox.exec validates against the approved action.
    let args_json = serde_json::to_string(&serde_json::json!({
        "command": command,
        "dependencies": dependencies.as_ref().map(|d| serde_json::json!({
            "runtime": d.runtime,
            "packages": d.packages,
        })),
        "approval_ref": decision.request_id,
    }))?;

    tracing::info!(
        target: "approval",
        request_id = %decision.request_id,
        agent_id = %decision.agent_id,
        "Executing approved sandbox.exec directly"
    );

    SandboxExecTool.execute(
        &manifest,
        &policy,
        &agent_dir,
        Some(gateway_dir.as_path()),
        &args_json,
        Some(&decision.session_id),
        None,
        Some(config),
        approval_store,
    )
}

/// Execute an approved WriteFile action.
fn execute_approved_write(
    config: &GatewayConfig,
    decision: &ApprovalDecision,
) -> anyhow::Result<()> {
    let ScheduledAction::WriteFile { path, content, .. } = &decision.action else {
        return Ok(());
    };

    // The payload is stored directly in the ScheduledAction::WriteFile action,
    // so we can use the path and content directly.
    let write_path = path.clone();
    let write_content = content.clone();

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
    let ScheduledAction::AgentInstall {
        agent_id, payload, ..
    } = &decision.action
    else {
        return Ok(());
    };

    // Load the stored install payload (created when approval was first requested, now via GatewayStore)
    let mut args_value = payload
        .clone()
        .ok_or_else(|| anyhow::anyhow!("No stored payload found in SQLite for request"))?;

    // Use the requested agent's directory as the parent (e.g., specialized_builder.default)
    let parent_agent_id = &decision.agent_id;
    let parent_dir = config.agents_dir.join(parent_agent_id);

    // Add promotion_gate with install_approval_ref if not present
    if let Some(obj) = args_value.as_object_mut() {
        if !obj.contains_key("promotion_gate") {
            obj.insert(
                "promotion_gate".to_string(),
                serde_json::json!({
                    "evaluator_pass": true,
                    "auditor_pass": true,
                    "install_approval_ref": decision.request_id,
                    "override_approval_ref": decision.request_id
                }),
            );
        } else if let Some(gate) = obj.get_mut("promotion_gate") {
            if let Some(gate_obj) = gate.as_object_mut() {
                gate_obj.insert(
                    "install_approval_ref".to_string(),
                    serde_json::Value::String(decision.request_id.clone()),
                );
                gate_obj.insert(
                    "override_approval_ref".to_string(),
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
        None,
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

            Ok(())
        }
        Err(e) => Err(anyhow::anyhow!("Failed to execute approved install: {}", e)),
    }
}

fn decide_request(
    config: &GatewayConfig,
    gateway_store: Option<&crate::scheduler::gateway_store::GatewayStore>,
    request_id: &str,
    decided_by: &str,
    reason: Option<String>,
    status: ApprovalStatus,
) -> anyhow::Result<ApprovalDecision> {
    let request = if let Some(store) = gateway_store {
        store
            .get_approval(request_id)?
            .ok_or_else(|| anyhow::anyhow!("Approval request not found in store: {}", request_id))?
    } else {
        anyhow::bail!("GatewayStore is required to decide approvals");
    };

    let decision = ApprovalDecision {
        request_id: request.request_id,
        agent_id: request.agent_id,
        session_id: request.session_id,
        action: request.action,
        status: status.clone(),
        decided_at: chrono::Utc::now().to_rfc3339(),
        decided_by: decided_by.to_string(),
        reason,
        root_session_id: request.root_session_id.clone(),
        workflow_id: request.workflow_id.clone(),
        task_id: request.task_id.clone(),
    };
    // Persist decision in GatewayStore
    if let Some(store) = gateway_store {
        if let Err(e) = store.record_decision(
            &decision.request_id,
            match decision.status {
                ApprovalStatus::Approved => "approved",
                ApprovalStatus::Rejected => "rejected",
            },
            &decision.decided_by,
            &decision.decided_at,
        ) {
            tracing::warn!(
                target: "approval",
                request_id = %decision.request_id,
                error = %e,
                "Failed to record decision in store"
            );
        }
    }

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
    use super::{
        build_approval_resume_message, should_notify_parent_session, should_resume_waiting_session,
    };
    use crate::scheduler::workflow_store::{ensure_workflow_for_root_session, save_task_run};
    use autonoetic_types::background::{
        ApprovalDecision, ApprovalRequest, ApprovalStatus, ScheduledAction,
    };
    use autonoetic_types::config::GatewayConfig;
    use autonoetic_types::workflow::{TaskRun, TaskRunStatus};
    use tempfile::tempdir;

    #[test]
    fn load_approval_requests_skips_payload_companion_files() {
        let dir = tempdir().unwrap();
        let agents_dir = dir.path().join("agents");
        let gateway_dir = agents_dir.join(".gateway");
        std::fs::create_dir_all(&gateway_dir).unwrap();
        let cfg = GatewayConfig {
            agents_dir: agents_dir.clone(),
            ..Default::default()
        };
        let store = crate::scheduler::gateway_store::GatewayStore::open(&gateway_dir).unwrap();

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
            root_session_id: None,
            status: None,
            decided_at: None,
            decided_by: None,
        };
        store.create_approval(&req).unwrap();

        let loaded = super::load_approval_requests(&cfg, Some(&store)).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].request_id, "apr-test1234");
    }

    #[test]
    fn pending_approval_requests_for_root_filters_by_session() {
        let dir = tempdir().unwrap();
        let agents_dir = dir.path().join("agents");
        let gateway_dir = agents_dir.join(".gateway");
        std::fs::create_dir_all(&gateway_dir).unwrap();
        let cfg = GatewayConfig {
            agents_dir: agents_dir.clone(),
            ..Default::default()
        };
        let store = crate::scheduler::gateway_store::GatewayStore::open(&gateway_dir).unwrap();

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
            root_session_id: None,
            status: None,
            decided_at: None,
            decided_by: None,
        };
        store
            .create_approval(&req("apr-a", "root-a/coder-1"))
            .unwrap();
        store
            .create_approval(&req("apr-b", "root-b/coder-1"))
            .unwrap();

        let for_a =
            super::pending_approval_requests_for_root(&cfg, Some(&store), "root-a").unwrap();
        assert_eq!(for_a.len(), 1);
        assert_eq!(for_a[0].request_id, "apr-a");
    }

    #[test]
    fn pending_sandbox_exec_requests_for_session_filters_and_sorts() {
        let dir = tempdir().unwrap();
        let agents_dir = dir.path().join("agents");
        let gateway_dir = agents_dir.join(".gateway");
        std::fs::create_dir_all(&gateway_dir).unwrap();
        let cfg = GatewayConfig {
            agents_dir: agents_dir.clone(),
            ..Default::default()
        };
        let store = crate::scheduler::gateway_store::GatewayStore::open(&gateway_dir).unwrap();

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
            root_session_id: None,
            status: None,
            decided_at: None,
            decided_by: None,
        };
        store
            .create_approval(&req("apr-second", "2020-01-02T00:00:00Z"))
            .unwrap();
        store
            .create_approval(&req("apr-first", "2020-01-01T00:00:00Z"))
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
                payload: None,
            },
            created_at: "2019-01-01T00:00:00Z".to_string(),
            reason: None,
            evidence_ref: None,
            workflow_id: None,
            task_id: None,
            root_session_id: None,
            status: None,
            decided_at: None,
            decided_by: None,
        };
        store.create_approval(&install).unwrap();

        let list = super::pending_sandbox_exec_requests_for_session(
            &cfg,
            Some(&store),
            "sess/evaluator-1",
        )
        .unwrap();
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
            payload: None,
        };
        assert!(should_notify_parent_session(
            &action,
            "demo-session/specialized_builder.default-abcd1234"
        ));
    }

    #[test]
    fn test_should_notify_parent_session_for_sandbox_exec_in_child() {
        let action = ScheduledAction::SandboxExec {
            command: "python3 /tmp/weather.py".to_string(),
            dependencies: None,
            requires_approval: true,
            evidence_ref: None,
        };
        // Child session (evaluator/coder): parent has chat; parent must receive exec result.
        assert!(should_notify_parent_session(
            &action,
            "demo-session/coder.default-6738ac56"
        ));
    }

    #[test]
    fn test_should_not_notify_parent_session_for_sandbox_exec_root_only() {
        let action = ScheduledAction::SandboxExec {
            command: "python3 /tmp/weather.py".to_string(),
            dependencies: None,
            requires_approval: true,
            evidence_ref: None,
        };
        // Root session (no slash): no parent to notify.
        assert!(!should_notify_parent_session(&action, "demo-session"));
    }

    #[test]
    fn test_should_not_resume_waiting_session_for_workflow_bound_approval() {
        let decision = ApprovalDecision {
            request_id: "apr-workflow1".to_string(),
            agent_id: "coder.default".to_string(),
            session_id: "demo-session/coder.default-6738ac56".to_string(),
            action: ScheduledAction::SandboxExec {
                command: "python3 /tmp/weather.py".to_string(),
                dependencies: None,
                requires_approval: true,
                evidence_ref: None,
            },
            status: ApprovalStatus::Approved,
            decided_at: chrono::Utc::now().to_rfc3339(),
            decided_by: "operator".to_string(),
            reason: None,
            workflow_id: Some("wf-demo".to_string()),
            task_id: Some("task-demo".to_string()),
            root_session_id: Some("demo-session".to_string()),
        };

        assert!(!should_resume_waiting_session(&decision));
    }

    #[test]
    fn test_should_resume_waiting_session_for_non_workflow_approval() {
        let decision = ApprovalDecision {
            request_id: "apr-direct1".to_string(),
            agent_id: "coder.default".to_string(),
            session_id: "demo-session/coder.default-6738ac56".to_string(),
            action: ScheduledAction::SandboxExec {
                command: "python3 /tmp/weather.py".to_string(),
                dependencies: None,
                requires_approval: true,
                evidence_ref: None,
            },
            status: ApprovalStatus::Approved,
            decided_at: chrono::Utc::now().to_rfc3339(),
            decided_by: "operator".to_string(),
            reason: None,
            workflow_id: None,
            task_id: None,
            root_session_id: None,
        };

        assert!(should_resume_waiting_session(&decision));
    }

    #[test]
    fn workflow_bound_approval_skips_direct_session_notification() {
        let dir = tempdir().unwrap();
        let agents_dir = dir.path().join("agents");
        let gateway_dir = agents_dir.join(".gateway");
        let agent_dir = agents_dir.join("coder.default");
        std::fs::create_dir_all(&gateway_dir).unwrap();
        std::fs::create_dir_all(&agent_dir).unwrap();
        let cfg = GatewayConfig {
            agents_dir: agents_dir.clone(),
            ..Default::default()
        };
        let store = crate::scheduler::gateway_store::GatewayStore::open(&gateway_dir).unwrap();
        let wf =
            ensure_workflow_for_root_session(&cfg, Some(&store), "demo-session", None).unwrap();

        let task = TaskRun {
            task_id: "task-approval".to_string(),
            workflow_id: wf.workflow_id.clone(),
            agent_id: "coder.default".to_string(),
            session_id: "demo-session/coder.default-6738ac56".to_string(),
            parent_session_id: "demo-session".to_string(),
            status: TaskRunStatus::AwaitingApproval,
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            source_agent_id: Some("planner.default".to_string()),
            result_summary: None,
            join_group: None,
            message: Some("Continue after approval".to_string()),
            metadata: None,
        };
        save_task_run(&cfg, Some(&store), &task).unwrap();

        let request = ApprovalRequest {
            request_id: "apr-write123".to_string(),
            agent_id: "coder.default".to_string(),
            session_id: task.session_id.clone(),
            action: ScheduledAction::WriteFile {
                path: "approved.txt".to_string(),
                content: "approved".to_string(),
                requires_approval: true,
                evidence_ref: None,
            },
            created_at: chrono::Utc::now().to_rfc3339(),
            reason: None,
            evidence_ref: None,
            workflow_id: Some(wf.workflow_id.clone()),
            task_id: Some(task.task_id.clone()),
            root_session_id: Some("demo-session".to_string()),
            status: None,
            decided_at: None,
            decided_by: None,
        };
        store.create_approval(&request).unwrap();

        super::approve_request(&cfg, Some(&store), &request.request_id, "operator", None).unwrap();

        let pending = store.list_pending_notifications().unwrap();
        assert!(
            pending.is_empty(),
            "workflow-bound approvals should continue through workflow re-queue only"
        );
    }

    #[test]
    fn sandbox_approval_signal_includes_execution_output_after_auto_run() {
        let dir = tempdir().unwrap();
        let agents_dir = dir.path().join("agents");
        let gateway_dir = agents_dir.join(".gateway");
        std::fs::create_dir_all(&gateway_dir).unwrap();
        let cfg = GatewayConfig {
            agents_dir,
            ..Default::default()
        };
        let store = std::sync::Arc::new(
            crate::scheduler::gateway_store::GatewayStore::open(&gateway_dir).unwrap(),
        );

        let decision = ApprovalDecision {
            request_id: "apr-out1234".to_string(),
            agent_id: "coder.default".to_string(),
            session_id: "demo-session/coder.default-6738ac56".to_string(),
            action: ScheduledAction::SandboxExec {
                command: "python3 /tmp/weather.py".to_string(),
                dependencies: None,
                requires_approval: true,
                evidence_ref: None,
            },
            status: ApprovalStatus::Approved,
            decided_at: chrono::Utc::now().to_rfc3339(),
            decided_by: "operator".to_string(),
            reason: None,
            workflow_id: None,
            task_id: None,
            root_session_id: None,
        };

        let exec_result = serde_json::json!({
            "ok": true,
            "exit_code": 0,
            "stdout": "Paris: 18C",
            "stderr": "",
        })
        .to_string();

        super::resume_session_after_approval(
            &cfg,
            Some(store.as_ref()),
            &decision,
            true,
            Some(exec_result),
        )
        .unwrap();

        let pending = store.list_pending_notifications().unwrap();
        assert!(!pending.is_empty(), "should have created a notification");
    }

    #[test]
    fn sandbox_approval_signal_falls_back_to_manual_retry_when_exec_result_missing() {
        let dir = tempdir().unwrap();
        let agents_dir = dir.path().join("agents");
        let gateway_dir = agents_dir.join(".gateway");
        std::fs::create_dir_all(&gateway_dir).unwrap();
        let cfg = GatewayConfig {
            agents_dir,
            ..Default::default()
        };
        let store = std::sync::Arc::new(
            crate::scheduler::gateway_store::GatewayStore::open(&gateway_dir).unwrap(),
        );

        let decision = ApprovalDecision {
            request_id: "apr-fallback1".to_string(),
            agent_id: "coder.default".to_string(),
            session_id: "demo-session/coder.default-6738ac56".to_string(),
            action: ScheduledAction::SandboxExec {
                command: "python3 /tmp/weather.py".to_string(),
                dependencies: None,
                requires_approval: true,
                evidence_ref: None,
            },
            status: ApprovalStatus::Approved,
            decided_at: chrono::Utc::now().to_rfc3339(),
            decided_by: "operator".to_string(),
            reason: None,
            workflow_id: None,
            task_id: None,
            root_session_id: None,
        };

        super::resume_session_after_approval(&cfg, Some(store.as_ref()), &decision, false, None)
            .unwrap();

        let pending = store.list_pending_notifications().unwrap();
        assert!(
            !pending.is_empty(),
            "should have created a fallback notification"
        );
    }

    #[test]
    fn build_approval_resume_message_returns_valid_json_with_escaped_output() {
        let decision = ApprovalDecision {
            request_id: "apr-jsonsafe1".to_string(),
            agent_id: "coder.default".to_string(),
            session_id: "demo-session/coder.default-1234".to_string(),
            action: ScheduledAction::SandboxExec {
                command: "python3 /tmp/weather.py".to_string(),
                dependencies: None,
                requires_approval: true,
                evidence_ref: None,
            },
            status: ApprovalStatus::Approved,
            decided_at: chrono::Utc::now().to_rfc3339(),
            decided_by: "operator".to_string(),
            reason: None,
            workflow_id: None,
            task_id: None,
            root_session_id: None,
        };

        let exec_result = serde_json::json!({
            "ok": true,
            "exit_code": 0,
            "stdout": "line1\n\"quoted\" value",
            "stderr": "",
        })
        .to_string();

        let msg = build_approval_resume_message(&decision, Some(&exec_result));
        let parsed: serde_json::Value = serde_json::from_str(&msg).unwrap();

        assert_eq!(
            parsed.get("type").and_then(|v| v.as_str()),
            Some("approval_resolved")
        );
        assert_eq!(
            parsed.get("request_id").and_then(|v| v.as_str()),
            Some("apr-jsonsafe1")
        );
        assert_eq!(
            parsed.get("status").and_then(|v| v.as_str()),
            Some("approved")
        );
        assert_eq!(
            parsed
                .get("execution")
                .and_then(|v| v.get("ok"))
                .and_then(|v| v.as_bool()),
            Some(true)
        );
    }
}
