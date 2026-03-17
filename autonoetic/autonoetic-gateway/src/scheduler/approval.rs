//! Approval resolution for the background scheduler.
//! Handles loading, approving, and rejecting approval requests.
//!
//! When an agent.install approval is resolved, the gateway automatically
//! executes the approved install using the stored payload.

use crate::execution::{gateway_actor_id, init_gateway_causal_logger};
use crate::runtime::tools::{AgentInstallTool, NativeTool};
use crate::policy::PolicyEngine;
use crate::tracing::{EventScope, SessionId, TraceSession};
use autonoetic_types::background::{ApprovalDecision, ApprovalRequest, ApprovalStatus, ScheduledAction};
use autonoetic_types::config::GatewayConfig;
use autonoetic_types::agent::AgentManifest;
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
    
    Ok(decision)
}

/// Resume a session after approval/rejection by sending an event to the gateway.
/// This allows the caller (e.g., planner) to automatically continue without
/// requiring the user to type "approved" in chat.
/// 
/// `install_succeeded` indicates whether the auto-install succeeded (if applicable).
fn resume_session_after_approval(
    config: &GatewayConfig,
    decision: &ApprovalDecision,
    install_succeeded: bool,
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
    
    // Extract agent_id for signal and message
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
        _ => ("unknown".to_string(), format!("Approval {} for request {}", status_str, decision.request_id)),
    };
    
    let message = serde_json::json!({
        "type": "approval_resolved",
        "request_id": decision.request_id,
        "agent_id": agent_id,
        "status": status_str,
        "install_completed": install_succeeded,
        "message": status_message
    }).to_string();
    
    // Write signal file for CLI to detect (enables auto-resume)
    let signal = super::signal::Signal::ApprovalResolved {
        request_id: decision.request_id.clone(),
        agent_id: agent_id.clone(),
        status: status_str.to_string(),
        install_completed: install_succeeded,
        message: status_message.clone(),
        timestamp: chrono::Utc::now().to_rfc3339(),
    };
    
    if let Err(e) = super::signal::write_signal(&config.agents_dir.join(".gateway"), session_id, &decision.request_id, &signal) {
        tracing::warn!(
            target: "approval",
            request_id = %decision.request_id,
            error = %e,
            "Failed to write signal file"
        );
    }
    
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
    let payload_path = pending_approvals_dir(config).join(format!("{}_payload.json", decision.request_id));
    
    let (write_path, write_content) = if payload_path.exists() {
        // Use stored payload
        let payload_json = std::fs::read_to_string(&payload_path)?;
        let payload: serde_json::Value = serde_json::from_str(&payload_json)
            .map_err(|e| anyhow::anyhow!("Failed to parse stored payload: {}", e))?;
        
        let p = payload.get("path").and_then(|v| v.as_str()).unwrap_or(path).to_string();
        let c = payload.get("content").and_then(|v| v.as_str()).unwrap_or(content).to_string();
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
    use autonoetic_types::background::ScheduledAction;
    use autonoetic_types::agent::{RuntimeDeclaration, AgentIdentity};
    
    // Only handle agent_install actions
    let ScheduledAction::AgentInstall { agent_id, .. } = &decision.action else {
        return Ok(());
    };
    
    // Load the stored install payload (created when approval was first requested)
    let payload_path = pending_approvals_dir(config).join(format!("{}_payload.json", decision.request_id));
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
            obj.insert("promotion_gate".to_string(), serde_json::json!({
                "evaluator_pass": true,
                "auditor_pass": true,
                "install_approval_ref": decision.request_id
            }));
        } else if let Some(gate) = obj.get_mut("promotion_gate") {
            if let Some(gate_obj) = gate.as_object_mut() {
                gate_obj.insert("install_approval_ref".to_string(), 
                    serde_json::Value::String(decision.request_id.clone()));
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
    let gateway_dir = config.agents_dir.parent();
    let policy = PolicyEngine::new(parent_manifest.clone());
    
    match tool.execute(
        &parent_manifest,
        &policy,
        agent_dir,
        gateway_dir,
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
