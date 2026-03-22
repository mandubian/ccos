//! Gateway-owned background scheduler.
//! 
//! This module has been split by domain responsibility:
//! - [`crate::scheduler::decision`] - Wake-decision logic
//! - [`crate::scheduler::store`] - Persistence helpers  
//! - [`crate::scheduler::approval`] - Approval resolution
//! - [`crate::scheduler::runner`] - Side-effecting execution
//!
//! The main entry points remain in this file for backwards compatibility.

use std::sync::Arc;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

pub mod decision;
pub mod store;
pub mod approval;
pub mod runner;
pub mod signal;
pub mod workflow_store;
pub mod workflow_causal;
pub mod gateway_store;

pub use decision::*;
pub use store::*;
pub use approval::*;
pub use runner::*;
pub use signal::*;
pub use workflow_store::*;
pub use workflow_causal::*;
pub use gateway_store::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxEvent {
    pub event_id: String,
    pub message: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
}

pub async fn start_background_scheduler(
    execution: Arc<crate::execution::GatewayExecutionService>,
) -> anyhow::Result<()> {
    let config = execution.config();
    if !config.background_scheduler_enabled {
        tracing::info!("Background scheduler disabled");
        std::future::pending::<()>().await;
        unreachable!();
    }

    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(
        config.background_tick_secs.max(1),
    ));
    loop {
        ticker.tick().await;
        if let Err(e) = run_scheduler_tick(execution.clone()).await {
            tracing::warn!(error = %e, "Background scheduler tick failed");
        }
    }
}

pub async fn run_scheduler_tick(execution: Arc<crate::execution::GatewayExecutionService>) -> anyhow::Result<()> {
    run_scheduler_tick_at(execution, Utc::now()).await
}

async fn run_scheduler_tick_at(
    execution: Arc<crate::execution::GatewayExecutionService>,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    // Process pending notifications
    if let Some(store) = execution.gateway_store() {
        if let Err(e) = process_pending_notifications(execution.clone(), store.as_ref()).await {
            tracing::warn!(error = %e, "Failed to process pending notifications");
        }
        // Cleanup stale notifications (e.g., older than 24h)
        let _ = store.cleanup_stale_notifications(24);
    }

    let config = execution.config();
    let repo = crate::agent::AgentRepository::from_config(&config);
    let agent_metas = repo.list().await?;
    let mut admitted = 0usize;

    for agent_meta in agent_metas {
        if admitted >= config.max_background_due_per_tick.max(1) {
            break;
        }

        let loaded = repo.get_sync(&agent_meta.id).map_err(|e| {
            anyhow::anyhow!(
                "Failed to load agent '{}': {}. Fix or remove the agent directory.",
                agent_meta.id,
                e
            )
        })?;

        let Some(background) = loaded.manifest.background.clone() else {
            continue;
        };
        if !background.enabled {
            continue;
        }

        let policy = crate::policy::PolicyEngine::new(loaded.manifest.clone());
        let Some((cap_min_interval, allow_reasoning)) = policy.background_reevaluation_limits()
        else {
            continue;
        };

        let session_id = decision::background_session_id(&loaded.manifest.agent.id);
        let effective_interval = decision::effective_interval_secs(&config, &background, cap_min_interval);
        let state_path = store::background_state_path(&config, &loaded.manifest.agent.id);
        let mut state = store::load_background_state(&state_path, &loaded.manifest.agent.id, &session_id)?;
        if state.next_due_at.is_none() {
            state.next_due_at =
                Some((now + Duration::seconds(effective_interval as i64)).to_rfc3339());
            store::save_background_state(&state_path, &state)?;
        }

        let reevaluation = crate::runtime::reevaluation_state::load_reevaluation_state(&loaded.dir)?;
        let reason = decision::should_wake(
            &config,
            &loaded.manifest.agent.id,
            &session_id,
            &background,
            &state,
            &reevaluation,
            execution.gateway_store().as_deref(),
            now,
        )?;
        decision::log_should_wake(
            &config,
            &session_id,
            &loaded.manifest.agent.id,
            &reason,
            effective_interval,
        );

        let Some(reason) = reason else {
            continue;
        };
        admitted += 1;

        runner::handle_due_wake(
            execution.clone(),
            &loaded.manifest.agent.id,
            &loaded.dir,
            &background,
            allow_reasoning,
            effective_interval,
            &session_id,
            state,
            reevaluation,
            reason,
            now,
        )
        .await?;
    }

    // Process Runnable tasks (approval-unblocked tasks that need execution)
    if let Err(e) = process_runnable_workflow_tasks(execution.clone()).await {
        tracing::warn!(error = %e, "Failed to process runnable workflow tasks");
    }

    // Process queued async workflow tasks (Phase 2)
    if let Err(e) = process_queued_workflow_tasks(execution).await {
        tracing::warn!(error = %e, "Failed to process queued workflow tasks");
    }

    Ok(())
}

fn task_claim_heartbeat_interval_secs(config: &autonoetic_types::config::GatewayConfig) -> u64 {
    config.background_tick_secs.clamp(1, 5)
}

fn task_claim_stale_after_secs(config: &autonoetic_types::config::GatewayConfig) -> u64 {
    task_claim_heartbeat_interval_secs(config)
        .saturating_mul(4)
        .max(30)
}

/// Process queued workflow tasks: dequeue, create TaskRun records, and spawn child agents.
///
/// Called by the scheduler tick after processing background agents.
/// Each queued task must first acquire a durable claim before it is launched.
/// The tokio task runs `spawn_agent_once`, heartbeats its claim, and cleans up on completion.
pub async fn process_queued_workflow_tasks(
    execution: Arc<crate::execution::GatewayExecutionService>,
) -> anyhow::Result<()> {
    let config = execution.config();
    let store = execution.gateway_store();
    let store = store.as_deref();
    let queued = workflow_store::load_all_queued_tasks(&config, store)?;
    if queued.is_empty() {
        return Ok(());
    }

    tracing::info!(
        target: "workflow",
        count = queued.len(),
        "Processing queued workflow tasks"
    );

    for queued_task in queued {
        let existing_task = workflow_store::load_task_run(
            &config,
            store,
            &queued_task.workflow_id,
            &queued_task.task_id,
        )?;

        if let Some(existing) = existing_task.as_ref() {
            match existing.status {
                autonoetic_types::workflow::TaskRunStatus::Succeeded
                | autonoetic_types::workflow::TaskRunStatus::Failed
                | autonoetic_types::workflow::TaskRunStatus::Cancelled => {
                    let _ = workflow_store::dequeue_task(
                        &config,
                        store,
                        &queued_task.workflow_id,
                        &queued_task.task_id,
                    );
                    continue;
                }
                autonoetic_types::workflow::TaskRunStatus::Running => {
                    tracing::info!(
                        target: "workflow",
                        task_id = %queued_task.task_id,
                        "Task marked Running; claim freshness decides recovery"
                    );
                }
                _ => {}
            }
        }

        let Some(_claim) = workflow_store::acquire_task_claim(
            &config,
            store,
            &queued_task.workflow_id,
            &queued_task.task_id,
            task_claim_stale_after_secs(&config),
        )?
        else {
            tracing::debug!(
                target: "workflow",
                task_id = %queued_task.task_id,
                "Task already claimed by a live executor"
            );
            continue;
        };

        if let Some(mut existing) = existing_task {
            if existing.status == autonoetic_types::workflow::TaskRunStatus::Running {
                tracing::info!(
                    target: "workflow",
                    task_id = %queued_task.task_id,
                    "Recovered stale claimed task; re-spawning execution"
                );
            } else {
                existing.status = autonoetic_types::workflow::TaskRunStatus::Running;
                existing.updated_at = chrono::Utc::now().to_rfc3339();
                existing.message = Some(queued_task.message.clone());
                existing.metadata = queued_task.metadata.clone();
                if let Err(e) = workflow_store::save_task_run(&config, store, &existing) {
                    tracing::warn!(
                        target: "workflow",
                        task_id = %queued_task.task_id,
                        error = %e,
                        "Failed to persist claimed task"
                    );
                    let _ = workflow_store::release_task_claim(
                        &config,
                        store,
                        &queued_task.workflow_id,
                        &queued_task.task_id,
                    );
                    continue;
                }

                let _ = workflow_store::append_workflow_event(
                    &config,
                    store,
                    &autonoetic_types::workflow::WorkflowEventRecord {
                        event_id: format!("wevt-{}", &uuid::Uuid::new_v4().to_string()[..8]),
                        workflow_id: queued_task.workflow_id.clone(),
                        task_id: Some(queued_task.task_id.clone()),
                        event_type: "task.started".to_string(),
                        agent_id: Some(queued_task.agent_id.clone()),
                        payload: serde_json::json!({
                            "agent_id": queued_task.agent_id,
                            "child_session_id": queued_task.child_session_id,
                        }),
                        occurred_at: chrono::Utc::now().to_rfc3339(),
                    },
                );
            }
        } else {
            let ts = chrono::Utc::now().to_rfc3339();
            let task_run = autonoetic_types::workflow::TaskRun {
                task_id: queued_task.task_id.clone(),
                workflow_id: queued_task.workflow_id.clone(),
                agent_id: queued_task.agent_id.clone(),
                session_id: queued_task.child_session_id.clone(),
                parent_session_id: queued_task.parent_session_id.clone(),
                status: autonoetic_types::workflow::TaskRunStatus::Running,
                created_at: ts.clone(),
                updated_at: ts,
                source_agent_id: Some(queued_task.source_agent_id.clone()),
                result_summary: None,
                join_group: queued_task.join_group.clone(),
                message: Some(queued_task.message.clone()),
                metadata: queued_task.metadata.clone(),
            };
            if let Err(e) = workflow_store::save_task_run(&config, store, &task_run) {
                tracing::warn!(
                    target: "workflow",
                    task_id = %queued_task.task_id,
                    error = %e,
                    "Failed to save task run"
                );
                let _ = workflow_store::release_task_claim(
                    &config,
                    store,
                    &queued_task.workflow_id,
                    &queued_task.task_id,
                );
                continue;
            }

            let _ = workflow_store::append_workflow_event(
                &config,
                store,
                &autonoetic_types::workflow::WorkflowEventRecord {
                    event_id: format!("wevt-{}", &uuid::Uuid::new_v4().to_string()[..8]),
                    workflow_id: queued_task.workflow_id.clone(),
                    task_id: Some(queued_task.task_id.clone()),
                    event_type: "task.started".to_string(),
                    agent_id: Some(queued_task.agent_id.clone()),
                    payload: serde_json::json!({
                        "agent_id": queued_task.agent_id,
                        "child_session_id": queued_task.child_session_id,
                    }),
                    occurred_at: chrono::Utc::now().to_rfc3339(),
                },
            );
        }

        // Spawn background execution
        let exec = execution.clone();
        let agent_id = queued_task.agent_id.clone();
        let message = queued_task.message.clone();
        let session_id = queued_task.child_session_id.clone();
        let source_id = queued_task.source_agent_id.clone();
        let metadata = queued_task.metadata.clone();
        let wf_id = queued_task.workflow_id.clone();
        let t_id = queued_task.task_id.clone();
        let cfg = config.clone();

        tokio::spawn(async move {
            spawn_task_execution(
                exec, cfg, wf_id, t_id, agent_id, message, session_id, source_id, metadata,
            )
            .await;
        });
    }

    Ok(())
}

/// Execute a task: spawn agent, update status, checkpoint, dequeue on completion.
/// Shared by normal execution, crash recovery, and approval-resume paths.
async fn spawn_task_execution(
    exec: Arc<crate::execution::GatewayExecutionService>,
    cfg: Arc<autonoetic_types::config::GatewayConfig>,
    wf_id: String,
    t_id: String,
    agent_id: String,
    message: String,
    session_id: String,
    source_id: String,
    metadata: Option<serde_json::Value>,
) {
    let store = exec.gateway_store();
    let store = store.as_deref();
    // Load previous task checkpoint (if any) for resume context
    let prev_checkpoint = workflow_store::load_task_checkpoint(&cfg, store, &wf_id, &t_id).ok().flatten();
    let checkpoint_context = if let Some(ref prev) = prev_checkpoint {
        tracing::info!(
            target: "workflow",
            task_id = %t_id,
            prev_step = %prev.step,
            prev_version = prev.version,
            "Resuming task from checkpoint"
        );
        serde_json::json!({
            "agent_id": agent_id,
            "session_id": session_id,
            "message_preview": &message[..message.len().min(120)],
            "resuming_from": { "step": prev.step, "version": prev.version, "state": prev.state },
        })
    } else {
        serde_json::json!({
            "agent_id": agent_id,
            "session_id": session_id,
            "message_preview": &message[..message.len().min(120)],
        })
    };
    let _ = workflow_store::checkpoint_task(
        &cfg,
        store,
        &wf_id,
        &t_id,
        "starting".to_string(),
        checkpoint_context,
    );

    let heartbeat_cfg = cfg.clone();
    let heartbeat_wf_id = wf_id.clone();
    let heartbeat_task_id = t_id.clone();
    let heartbeat_interval_secs = task_claim_heartbeat_interval_secs(cfg.as_ref());
    let heartbeat = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(
            heartbeat_interval_secs,
        ));
        loop {
            interval.tick().await;
            let _ = workflow_store::refresh_task_claim_heartbeat(
                &heartbeat_cfg,
                None, // Heartbeat in async task currently doesn't have easy access to store without more refactoring
                &heartbeat_wf_id,
                &heartbeat_task_id,
            );
        }
    });

    let result = exec
        .spawn_agent_once(
            &agent_id,
            &message,
            &session_id,
            Some(&source_id),
            false,
            None,
            metadata.as_ref(),
        )
        .await;

    heartbeat.abort();
    let _ = heartbeat.await;

    match result {
        Ok(spawn_result) => {
            let pending_approval_ids: Vec<String> = crate::scheduler::approval::load_approval_requests(&cfg, exec.gateway_store().as_deref())
                .map(|requests| {
                    requests
                        .into_iter()
                        .filter(|request| request.session_id == session_id)
                        .map(|request| request.request_id)
                        .collect()
                })
                .unwrap_or_default();

            if !pending_approval_ids.is_empty() {
                let summary = format!(
                    "awaiting approval {}",
                    pending_approval_ids.join(",")
                );
                if let Err(e) = workflow_store::update_task_run_status(
                    &cfg,
                    store,
                    &wf_id,
                    &t_id,
                    autonoetic_types::workflow::TaskRunStatus::AwaitingApproval,
                    Some(summary.clone()),
                ) {
                    tracing::warn!(
                        target: "workflow",
                        error = %e,
                        task_id = %t_id,
                        "Failed to persist async task awaiting approval status"
                    );
                }
                let _ = workflow_store::checkpoint_task(
                    &cfg,
                    store,
                    &wf_id,
                    &t_id,
                    "awaiting_approval".to_string(),
                    serde_json::json!({
                        "status": "awaiting_approval",
                        "pending_approval_ids": pending_approval_ids,
                        "result_summary": spawn_result.assistant_reply.as_ref().map(|s| &s[..s.len().min(200)]),
                    }),
                );
                let _ = workflow_store::dequeue_task(&cfg, store, &wf_id, &t_id);
                tracing::info!(
                    target: "workflow",
                    task_id = %t_id,
                    "Async task is awaiting approval; not marking completed"
                );
                return;
            }

            let summary = spawn_result.assistant_reply.as_ref().map(|s| {
                const MAX: usize = 512;
                if s.len() <= MAX { s.clone() } else { format!("{}…", &s[..MAX]) }
            });
            if let Err(e) = workflow_store::update_task_run_status(
                &cfg,
                store,
                &wf_id,
                &t_id,
                autonoetic_types::workflow::TaskRunStatus::Succeeded,
                summary,
            ) {
                tracing::warn!(target: "workflow", error = %e, "Failed to persist async task completion");
            }
            tracing::info!(target: "workflow", task_id = %t_id, "Async task completed successfully");
            let _ = workflow_store::checkpoint_task(
                &cfg,
                store,
                &wf_id,
                &t_id,
                "completed".to_string(),
                serde_json::json!({
                    "status": "succeeded",
                    "result_summary": spawn_result.assistant_reply.as_ref().map(|s| &s[..s.len().min(200)]),
                }),
            );
            let _ = workflow_store::dequeue_task(&cfg, store, &wf_id, &t_id);
        }
        Err(e) => {
            if let Err(inner) = workflow_store::update_task_run_status(
                &cfg,
                store,
                &wf_id,
                &t_id,
                autonoetic_types::workflow::TaskRunStatus::Failed,
                Some(e.to_string()),
            ) {
                tracing::warn!(target: "workflow", error = %inner, "Failed to persist async task failure");
            }
            tracing::warn!(target: "workflow", task_id = %t_id, error = %e, "Async task failed");
            let _ = workflow_store::checkpoint_task(
                &cfg,
                store,
                &wf_id,
                &t_id,
                "failed".to_string(),
                serde_json::json!({ "status": "failed", "error": e.to_string() }),
            );
            let _ = workflow_store::dequeue_task(&cfg, store, &wf_id, &t_id);
        }
    }
}

/// Scan all workflows for Runnable tasks (approval-unblocked) and execute them.
///
/// When a task is unblocked by approval resolution (AwaitingApproval → Runnable),
/// it is re-queued into the same durable execution path used by async spawns.
pub async fn process_runnable_workflow_tasks(
    execution: Arc<crate::execution::GatewayExecutionService>,
) -> anyhow::Result<()> {
    let config = execution.config();
    let workflows_root = workflow_store::workflows_root(&config).join("runs");
    if !workflows_root.is_dir() {
        return Ok(());
    }

    for entry in std::fs::read_dir(&workflows_root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let wf_id = entry.file_name().to_string_lossy().to_string();
        let store = execution.gateway_store();
        let store = store.as_deref();
        let workflow_run = workflow_store::load_workflow_run(&config, store, &wf_id)?;
        let tasks = workflow_store::list_task_runs_for_workflow(&config, store, &wf_id)?;

        for task in tasks {
            if task.status != autonoetic_types::workflow::TaskRunStatus::Runnable {
                continue;
            }

            let blocks_planner = workflow_run
                .as_ref()
                .is_some_and(|run| run.join_task_ids.contains(&task.task_id));

            tracing::info!(
                target: "workflow",
                workflow_id = %wf_id,
                task_id = %task.task_id,
                agent_id = %task.agent_id,
                "Re-queueing approval-unblocked task for durable execution"
            );

            // Check if the approval stored a resume_message in the task checkpoint.
            // If so, use it instead of the original task message so the agent
            // receives the exec result and continues from where it left off rather
            // than restarting the task from scratch.
            let checkpoint = workflow_store::load_task_checkpoint(&config, store, &wf_id, &task.task_id)
                .ok()
                .flatten();
            let resume_message = checkpoint
                .as_ref()
                .filter(|cp| cp.step == "approval_resolved")
                .and_then(|cp| cp.state.get("resume_message")?.as_str().map(str::to_string));
            let effective_message = resume_message
                .clone()
                .unwrap_or_else(|| {
                    task.message
                        .clone()
                        .unwrap_or_else(|| format!("Resume after approval: {}", task.session_id))
                });

            // If a stale queued task exists (for example after crash/recovery),
            // reconcile its message with approval-resolved resume context.
            if workflow_store::queued_task_exists(&config, &wf_id, &task.task_id) {
                if let Some(resume_message) = resume_message {
                    if let Ok(queued_tasks) = workflow_store::load_queued_tasks(&config, store, &wf_id) {
                        if let Some(existing) = queued_tasks.into_iter().find(|q| q.task_id == task.task_id)
                        {
                            if existing.message != resume_message {
                                tracing::info!(
                                    target: "workflow",
                                    workflow_id = %wf_id,
                                    task_id = %task.task_id,
                                    "Refreshing stale queued task message from approval_resolved checkpoint"
                                );

                                let _ = workflow_store::dequeue_task(&config, store, &wf_id, &task.task_id);
                                let mut refreshed = existing;
                                refreshed.message = resume_message;
                                refreshed.enqueued_at = chrono::Utc::now().to_rfc3339();

                                if let Err(e) = workflow_store::enqueue_task(&config, store, &refreshed) {
                                    tracing::warn!(
                                        target: "workflow",
                                        workflow_id = %wf_id,
                                        task_id = %task.task_id,
                                        error = %e,
                                        "Failed to refresh queued task message"
                                    );
                                }
                            }
                        }
                    }
                }
                continue;
            }

            let queued = autonoetic_types::workflow::QueuedTaskRun {
                task_id: task.task_id.clone(),
                workflow_id: wf_id.clone(),
                agent_id: task.agent_id.clone(),
                message: effective_message,
                child_session_id: task.session_id.clone(),
                parent_session_id: task.parent_session_id.clone(),
                source_agent_id: task.source_agent_id.clone().unwrap_or_default(),
                metadata: task.metadata.clone(),
                join_group: task.join_group.clone(),
                blocks_planner,
                enqueued_at: chrono::Utc::now().to_rfc3339(),
            };
            workflow_store::enqueue_task(&config, store, &queued)?;
        }
    }

    Ok(())
}

pub fn append_inbox_event(
    config: &autonoetic_types::config::GatewayConfig,
    agent_id: &str,
    message: impl Into<String>,
    session_id: Option<&str>,
) -> anyhow::Result<InboxEvent> {
    let event = InboxEvent {
        event_id: uuid::Uuid::new_v4().to_string(),
        message: message.into(),
        session_id: session_id.map(|value| value.to_string()),
        created_at: Some(Utc::now().to_rfc3339()),
    };
    store::append_jsonl_record(&store::inbox_path(config, agent_id), &event)?;
    Ok(event)
}

pub fn append_task_board_entry(
    config: &autonoetic_types::config::GatewayConfig,
    entry: &autonoetic_types::task_board::TaskBoardEntry,
) -> anyhow::Result<()> {
    store::append_jsonl_record(&store::task_board_path(config), entry)
}

async fn process_pending_notifications(
    execution: Arc<crate::execution::GatewayExecutionService>,
    store: &crate::scheduler::gateway_store::GatewayStore,
) -> anyhow::Result<()> {
    let pending = store.list_pending_notifications()?;
    if pending.is_empty() {
        return Ok(());
    }

    let port = execution.config().port;

    for n in pending {
        // Map NotificationRecord payload to Signal.
        // Only current Signal payloads are accepted for deterministic delivery.
        let signal = match n.notification_type {
            autonoetic_types::notification::NotificationType::ApprovalResolved => {
                serde_json::from_value::<crate::scheduler::signal::Signal>(n.payload.clone()).ok()
            }
            autonoetic_types::notification::NotificationType::WorkflowJoinSatisfied => {
                serde_json::from_value::<crate::scheduler::signal::Signal>(n.payload.clone()).ok()
            }
        };

        if let Some(signal) = signal {
            let pending_signal = crate::scheduler::signal::PendingSignal {
                request_id: n.request_id.clone().unwrap_or_else(|| n.notification_id.clone()),
                signal,
                filename: format!("{}.json", n.request_id.as_deref().unwrap_or(&n.notification_id)),
            };
            if let Err(e) = crate::scheduler::signal::deliver_signal(&pending_signal, &n.target_session_id, port).await {
                tracing::warn!(notification_id = %n.notification_id, error = %e, "Failed to deliver signal");

                // Track attempts and eventually fail so malformed/unreachable
                // notifications don't stay pending forever.
                let _ = store.increment_attempt(&n.notification_id, Some(&e.to_string()));
                let next_attempt = n.attempt_count.saturating_add(1);
                if next_attempt >= 3 {
                    let _ = store.update_notification_status(
                        &n.notification_id,
                        autonoetic_types::notification::NotificationStatus::Failed,
                    );
                }
            } else {
                store.update_notification_status(&n.notification_id, autonoetic_types::notification::NotificationStatus::Delivered)?;
            }
        } else {
            // Payload is not interpretable as a supported notification signal:
            // fail deterministically instead of leaving it pending forever.
            let _ = store.increment_attempt(
                &n.notification_id,
                Some("Unrecognized notification payload for scheduler delivery"),
            );
            let _ = store.update_notification_status(
                &n.notification_id,
                autonoetic_types::notification::NotificationStatus::Failed,
            );
        }
    }
    Ok(())
}
