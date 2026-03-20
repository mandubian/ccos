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

pub use decision::*;
pub use store::*;
pub use approval::*;
pub use runner::*;
pub use signal::*;
pub use workflow_store::*;
pub use workflow_causal::*;

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

    // Process queued async workflow tasks (Phase 2)
    if let Err(e) = process_queued_workflow_tasks(execution.clone()).await {
        tracing::warn!(error = %e, "Failed to process queued workflow tasks");
    }

    // Process Runnable tasks (approval-unblocked tasks that need execution)
    if let Err(e) = process_runnable_workflow_tasks(execution).await {
        tracing::warn!(error = %e, "Failed to process runnable workflow tasks");
    }

    Ok(())
}

/// Process queued workflow tasks: dequeue, create TaskRun records, and spawn child agents.
///
/// Called by the scheduler tick after processing background agents.
/// Each queued task is dequeued, marked as Running, and launched as a background tokio task.
/// The tokio task runs `spawn_agent_once` and updates the TaskRun on completion.
pub async fn process_queued_workflow_tasks(
    execution: Arc<crate::execution::GatewayExecutionService>,
) -> anyhow::Result<()> {
    let config = execution.config();
    let queued = workflow_store::load_all_queued_tasks(&config)?;
    if queued.is_empty() {
        return Ok(());
    }

    tracing::info!(
        target: "workflow",
        count = queued.len(),
        "Processing queued workflow tasks"
    );

    for queued_task in queued {
        // Crash-safe: check if task is already being processed (Running TaskRun exists).
        // If so, skip — it will be dequeued on completion. This prevents duplicate
        // processing while keeping the queue file as a durable crash-recovery record.
        if let Ok(Some(existing)) = workflow_store::load_task_run(
            &config,
            &queued_task.workflow_id,
            &queued_task.task_id,
        ) {
            match existing.status {
                autonoetic_types::workflow::TaskRunStatus::Running
                | autonoetic_types::workflow::TaskRunStatus::Succeeded
                | autonoetic_types::workflow::TaskRunStatus::Failed
                | autonoetic_types::workflow::TaskRunStatus::Cancelled => {
                    // Already processed or in-flight — skip. Dequeue to clean up.
                    let _ = workflow_store::dequeue_task(
                        &config,
                        &queued_task.workflow_id,
                        &queued_task.task_id,
                    );
                    continue;
                }
                _ => {}
            }
        }

        // Create TaskRun as Running (durably persisted BEFORE dequeue)
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
        };
        if let Err(e) = workflow_store::save_task_run(&config, &task_run) {
            tracing::warn!(
                target: "workflow",
                task_id = %queued_task.task_id,
                error = %e,
                "Failed to save task run"
            );
            continue;
        }

        // Emit task.started event
        let _ = workflow_store::append_workflow_event(
            &config,
            &autonoetic_types::workflow::WorkflowEventRecord {
                event_id: format!("wevt-{}", &uuid::Uuid::new_v4().to_string()[..8]),
                workflow_id: queued_task.workflow_id.clone(),
                task_id: Some(queued_task.task_id.clone()),
                event_type: "task.started".to_string(),
                payload: serde_json::json!({
                    "agent_id": queued_task.agent_id,
                    "child_session_id": queued_task.child_session_id,
                }),
                occurred_at: chrono::Utc::now().to_rfc3339(),
            },
        );

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
            tracing::info!(
                target: "workflow",
                task_id = %t_id,
                agent_id = %agent_id,
                "Starting async task execution"
            );

            // Load previous task checkpoint (if any) and checkpoint: starting execution
            let prev_checkpoint = workflow_store::load_task_checkpoint(&cfg, &wf_id, &t_id).ok().flatten();
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
                &wf_id,
                &t_id,
                "starting".to_string(),
                checkpoint_context,
            );

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

            match result {
                Ok(spawn_result) => {
                    let summary = spawn_result.assistant_reply.as_ref().map(|s| {
                        const MAX: usize = 512;
                        if s.len() <= MAX {
                            s.clone()
                        } else {
                            format!("{}…", &s[..MAX])
                        }
                    });
                    if let Err(e) = workflow_store::update_task_run_status(
                        &cfg,
                        &wf_id,
                        &t_id,
                        autonoetic_types::workflow::TaskRunStatus::Succeeded,
                        summary,
                    ) {
                        tracing::warn!(
                            target: "workflow",
                            error = %e,
                            "Failed to persist async task completion"
                        );
                    }
                    tracing::info!(
                        target: "workflow",
                        task_id = %t_id,
                        "Async task completed successfully"
                    );
                    // Task checkpoint: completed
                    let _ = workflow_store::checkpoint_task(
                        &cfg,
                        &wf_id,
                        &t_id,
                        "completed".to_string(),
                        serde_json::json!({
                            "status": "succeeded",
                            "result_summary": spawn_result.assistant_reply.as_ref().map(|s| &s[..s.len().min(200)]),
                        }),
                    );
                    // Dequeue: task is durably terminal, safe to remove from queue
                    let _ = workflow_store::dequeue_task(&cfg, &wf_id, &t_id);
                }
                Err(e) => {
                    if let Err(inner) = workflow_store::update_task_run_status(
                        &cfg,
                        &wf_id,
                        &t_id,
                        autonoetic_types::workflow::TaskRunStatus::Failed,
                        Some(e.to_string()),
                    ) {
                        tracing::warn!(
                            target: "workflow",
                            error = %inner,
                            "Failed to persist async task failure"
                        );
                    }
                    tracing::warn!(
                        target: "workflow",
                        task_id = %t_id,
                        error = %e,
                        "Async task failed"
                    );
                    // Task checkpoint: failed
                    let _ = workflow_store::checkpoint_task(
                        &cfg,
                        &wf_id,
                        &t_id,
                        "failed".to_string(),
                        serde_json::json!({
                            "status": "failed",
                            "error": e.to_string(),
                        }),
                    );
                    // Dequeue: task is durably terminal, safe to remove from queue
                    let _ = workflow_store::dequeue_task(&cfg, &wf_id, &t_id);
                }
            }
        });
    }

    Ok(())
}

/// Scan all workflows for Runnable tasks (approval-unblocked) and execute them.
///
/// When a task is unblocked by approval resolution (AwaitingApproval → Runnable),
/// it has no queue file to drive execution. This function picks up such tasks,
/// marks them Running, and spawns their execution.
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
        let tasks = workflow_store::list_task_runs_for_workflow(&config, &wf_id)?;

        for task in tasks {
            if task.status != autonoetic_types::workflow::TaskRunStatus::Runnable {
                continue;
            }

            tracing::info!(
                target: "workflow",
                workflow_id = %wf_id,
                task_id = %task.task_id,
                agent_id = %task.agent_id,
                "Executing approval-unblocked Runnable task"
            );

            // Mark as Running
            if let Err(e) = workflow_store::update_task_run_status(
                &config,
                &wf_id,
                &task.task_id,
                autonoetic_types::workflow::TaskRunStatus::Running,
                Some("resumed_after_approval".to_string()),
            ) {
                tracing::warn!(
                    target: "workflow",
                    task_id = %task.task_id,
                    error = %e,
                    "Failed to mark Runnable task as Running"
                );
                continue;
            }

            // Spawn execution
            let exec = execution.clone();
            let wf_id_clone = wf_id.clone();
            let task_clone = task.clone();
            let cfg = config.clone();

            tokio::spawn(async move {
                let result = exec
                    .spawn_agent_once(
                        &task_clone.agent_id,
                        &format!("Resume after approval: {}", task_clone.session_id),
                        &task_clone.session_id,
                        task_clone.source_agent_id.as_deref(),
                        false,
                        None,
                        None,
                    )
                    .await;

                match result {
                    Ok(spawn_result) => {
                        let summary = spawn_result.assistant_reply.as_ref().map(|s| {
                            const MAX: usize = 512;
                            if s.len() <= MAX { s.clone() } else { format!("{}…", &s[..MAX]) }
                        });
                        let _ = workflow_store::update_task_run_status(
                            &cfg,
                            &wf_id_clone,
                            &task_clone.task_id,
                            autonoetic_types::workflow::TaskRunStatus::Succeeded,
                            summary,
                        );
                    }
                    Err(e) => {
                        let _ = workflow_store::update_task_run_status(
                            &cfg,
                            &wf_id_clone,
                            &task_clone.task_id,
                            autonoetic_types::workflow::TaskRunStatus::Failed,
                            Some(e.to_string()),
                        );
                    }
                }
            });
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
