//! Durable workflow / task persistence under the gateway scheduler directory.
//!
//! Layout (under `<agents_dir>/.gateway/scheduler/workflows/`):
//! - `index/by_root/<sha256-hex>.json` — maps stable root key → `workflow_id`
//! - `runs/<workflow_id>/workflow.json` — [`WorkflowRun`](autonoetic_types::workflow::WorkflowRun)
//! - `runs/<workflow_id>/events.jsonl` — append-only [`WorkflowEventRecord`](autonoetic_types::workflow::WorkflowEventRecord)
//! - `runs/<workflow_id>/tasks/<task_id>.json` — [`TaskRun`](autonoetic_types::workflow::TaskRun)

use crate::execution::gateway_root_dir;
use crate::runtime::session_timeline::base_session_id;
use crate::scheduler::store::{append_jsonl_record, read_json_file, write_json_file};
use autonoetic_types::causal_chain::EntryStatus;
use autonoetic_types::config::GatewayConfig;
use autonoetic_types::workflow::{
    JoinPolicy, QueuedTaskRun, TaskCheckpoint, TaskRun, TaskRunStatus, WorkflowCheckpoint,
    WorkflowEventRecord, WorkflowRun, WorkflowRunStatus,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RootWorkflowIndex {
    workflow_id: String,
    root_session_id: String,
}

pub fn workflows_root(config: &GatewayConfig) -> PathBuf {
    gateway_root_dir(config).join("scheduler").join("workflows")
}

fn index_dir(config: &GatewayConfig) -> PathBuf {
    workflows_root(config).join("index").join("by_root")
}

fn root_index_path(config: &GatewayConfig, root_session_id: &str) -> PathBuf {
    index_dir(config).join(format!("{}.json", root_session_key(root_session_id)))
}

fn root_session_key(root_session_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(root_session_id.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn workflow_run_dir(config: &GatewayConfig, workflow_id: &str) -> PathBuf {
    workflows_root(config).join("runs").join(workflow_id)
}

pub fn workflow_run_path(config: &GatewayConfig, workflow_id: &str) -> PathBuf {
    workflow_run_dir(config, workflow_id).join("workflow.json")
}

pub fn workflow_events_path(config: &GatewayConfig, workflow_id: &str) -> PathBuf {
    workflow_run_dir(config, workflow_id).join("events.jsonl")
}

pub fn task_run_path(config: &GatewayConfig, workflow_id: &str, task_id: &str) -> PathBuf {
    workflow_run_dir(config, workflow_id)
        .join("tasks")
        .join(format!("{task_id}.json"))
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

fn new_workflow_id() -> String {
    format!("wf-{}", &uuid::Uuid::new_v4().to_string()[..8])
}

/// Allocate a new `task-*` id (separate from session paths).
pub fn new_task_id() -> String {
    format!("task-{}", &uuid::Uuid::new_v4().to_string()[..8])
}

fn new_event_id() -> String {
    format!("wevt-{}", &uuid::Uuid::new_v4().to_string()[..8])
}

/// Load a workflow run by id, if present.
pub fn load_workflow_run(
    config: &GatewayConfig,
    workflow_id: &str,
) -> anyhow::Result<Option<WorkflowRun>> {
    let p = workflow_run_path(config, workflow_id);
    if !p.exists() {
        return Ok(None);
    }
    Ok(Some(read_json_file(&p)?))
}

/// Resolve `wf-*` id from a root session id (`agent.spawn` root), if an index exists.
pub fn resolve_workflow_id_for_root_session(
    config: &GatewayConfig,
    root_session_id: &str,
) -> anyhow::Result<Option<String>> {
    let p = root_index_path(config, root_session_id);
    if !p.exists() {
        return Ok(None);
    }
    let idx: RootWorkflowIndex = read_json_file(&p)?;
    Ok(Some(idx.workflow_id))
}

/// Load append-only workflow events from `events.jsonl` (empty if missing).
pub fn load_workflow_events(
    config: &GatewayConfig,
    workflow_id: &str,
) -> anyhow::Result<Vec<WorkflowEventRecord>> {
    let path = workflow_events_path(config, workflow_id);
    crate::scheduler::store::load_jsonl_file(&path)
}

/// Persist full workflow run (creates parent dirs).
pub fn save_workflow_run(config: &GatewayConfig, run: &WorkflowRun) -> anyhow::Result<()> {
    let path = workflow_run_path(config, &run.workflow_id);
    write_json_file(&path, run)
}

/// Create or load the [`WorkflowRun`] for a root session (one workflow per root).
///
/// `lead_agent_id`, when `Some`, is written on first creation or used to fill an empty
/// `lead_agent_id` on an existing record.
pub fn ensure_workflow_for_root_session(
    config: &GatewayConfig,
    root_session_id: &str,
    lead_agent_id: Option<&str>,
) -> anyhow::Result<WorkflowRun> {
    anyhow::ensure!(
        !root_session_id.trim().is_empty(),
        "root_session_id must not be empty"
    );

    let idx_path = root_index_path(config, root_session_id);
    if idx_path.exists() {
        let idx: RootWorkflowIndex = read_json_file(&idx_path)?;
        let mut run: WorkflowRun = match load_workflow_run(config, &idx.workflow_id)? {
            Some(r) => r,
            None => {
                // Index exists but run file missing — recreate minimal run
                WorkflowRun {
                    workflow_id: idx.workflow_id.clone(),
                    root_session_id: root_session_id.to_string(),
                    lead_agent_id: lead_agent_id.unwrap_or("").to_string(),
                    status: WorkflowRunStatus::Active,
                    created_at: now_rfc3339(),
                    updated_at: now_rfc3339(),
                    active_task_ids: vec![],
                    blocked_task_ids: vec![],
                    pending_approval_ids: vec![],
                    queued_task_ids: vec![],
                    join_policy: Default::default(),
                    join_task_ids: vec![],
                }
            }
        };
        if run.lead_agent_id.is_empty() {
            if let Some(lead) = lead_agent_id.filter(|s| !s.is_empty()) {
                run.lead_agent_id = lead.to_string();
                run.updated_at = now_rfc3339();
                save_workflow_run(config, &run)?;
            }
        }
        return Ok(run);
    }

    let workflow_id = new_workflow_id();
    let ts = now_rfc3339();
    let run = WorkflowRun {
        workflow_id: workflow_id.clone(),
        root_session_id: root_session_id.to_string(),
        lead_agent_id: lead_agent_id.unwrap_or("").to_string(),
        status: WorkflowRunStatus::Active,
        created_at: ts.clone(),
        updated_at: ts,
        active_task_ids: vec![],
        blocked_task_ids: vec![],
        pending_approval_ids: vec![],
        queued_task_ids: vec![],
        join_policy: Default::default(),
        join_task_ids: vec![],
    };

    save_workflow_run(config, &run)?;
    write_json_file(
        &idx_path,
        &RootWorkflowIndex {
            workflow_id: workflow_id.clone(),
            root_session_id: root_session_id.to_string(),
        },
    )?;

    append_workflow_event(
        config,
        &WorkflowEventRecord {
            event_id: new_event_id(),
            workflow_id: workflow_id.clone(),
            task_id: None,
            event_type: "workflow.started".to_string(),
            payload: serde_json::json!({
                "root_session_id": root_session_id,
                "lead_agent_id": run.lead_agent_id,
            }),
            occurred_at: now_rfc3339(),
        },
    )?;

    crate::scheduler::workflow_causal::mirror_orchestration_event(
        config,
        root_session_id,
        "workflow.started",
        EntryStatus::Success,
        serde_json::json!({
            "workflow_id": workflow_id,
            "root_session_id": root_session_id,
            "lead_agent_id": run.lead_agent_id,
        }),
    );

    Ok(run)
}

fn workflow_session_dir(config: &GatewayConfig, root_session_id: &str) -> PathBuf {
    gateway_root_dir(config)
        .join("sessions")
        .join(base_session_id(root_session_id))
}

fn workflow_run_status_snake(s: WorkflowRunStatus) -> &'static str {
    match s {
        WorkflowRunStatus::Active => "active",
        WorkflowRunStatus::WaitingChildren => "waiting_children",
        WorkflowRunStatus::BlockedApproval => "blocked_approval",
        WorkflowRunStatus::Resumable => "resumable",
        WorkflowRunStatus::Completed => "completed",
        WorkflowRunStatus::Failed => "failed",
        WorkflowRunStatus::Cancelled => "cancelled",
    }
}

fn task_run_status_snake(s: TaskRunStatus) -> &'static str {
    match s {
        TaskRunStatus::Pending => "pending",
        TaskRunStatus::Runnable => "runnable",
        TaskRunStatus::Running => "running",
        TaskRunStatus::AwaitingApproval => "awaiting_approval",
        TaskRunStatus::Paused => "paused",
        TaskRunStatus::Succeeded => "succeeded",
        TaskRunStatus::Failed => "failed",
        TaskRunStatus::Cancelled => "cancelled",
    }
}

/// Rewrite `.gateway/sessions/{root}/workflow_graph.md` from current workflow + task + event state.
///
/// Called after each append to `events.jsonl` so operators can open it beside `timeline.md`.
pub fn refresh_workflow_graph_markdown(
    config: &GatewayConfig,
    workflow_id: &str,
) -> anyhow::Result<()> {
    let run = match load_workflow_run(config, workflow_id)? {
        Some(r) => r,
        None => return Ok(()),
    };
    let tasks = list_task_runs_for_workflow(config, workflow_id)?;
    let events = load_workflow_events(config, workflow_id)?;
    let start = events.len().saturating_sub(16);
    let recent = &events[start..];

    let dir = workflow_session_dir(config, &run.root_session_id);
    fs::create_dir_all(&dir)?;
    let path = dir.join("workflow_graph.md");

    let mut body = String::new();
    writeln!(body, "# Workflow graph: `{}`", run.root_session_id)?;
    writeln!(body)?;
    writeln!(
        body,
        "_Auto-updated when workflow orchestration events append (`events.jsonl`)._"
    )?;
    writeln!(body)?;
    writeln!(body, "| Field | Value |")?;
    writeln!(body, "|-------|-------|")?;
    writeln!(body, "| workflow_id | `{}` |", run.workflow_id)?;
    writeln!(
        body,
        "| status | `{}` |",
        workflow_run_status_snake(run.status)
    )?;
    let lead = if run.lead_agent_id.is_empty() {
        "_(unknown)_"
    } else {
        run.lead_agent_id.as_str()
    };
    writeln!(body, "| lead (planner) | `{}` |", lead)?;
    if !run.pending_approval_ids.is_empty() {
        writeln!(
            body,
            "| pending_approvals | `{}` |",
            run.pending_approval_ids.join(", ")
        )?;
    }
    writeln!(body)?;
    writeln!(body, "## Tasks")?;
    writeln!(body)?;
    if tasks.is_empty() {
        writeln!(body, "_(none yet)_")?;
    } else {
        for t in &tasks {
            writeln!(
                body,
                "- **{}** · `{}` · _{}_ — `{}`",
                t.agent_id,
                t.task_id,
                task_run_status_snake(t.status),
                t.session_id
            )?;
        }
    }
    writeln!(body)?;
    writeln!(body, "## Recent workflow events")?;
    writeln!(body)?;
    if recent.is_empty() {
        writeln!(body, "_(none)_")?;
    } else {
        for e in recent {
            let tid = e.task_id.as_deref().unwrap_or("—");
            let ts_short: String = e.occurred_at.chars().take(22).collect();
            writeln!(
                body,
                "- `{}` · **{}** · task `{}`",
                ts_short, e.event_type, tid
            )?;
        }
    }
    writeln!(body)?;
    writeln!(body, "---")?;
    writeln!(
        body,
        "_Generated: {} (UTC)_",
        chrono::Utc::now().to_rfc3339()
    )?;

    fs::write(&path, body)?;
    Ok(())
}

/// Append one event to the workflow's `events.jsonl`.
pub fn append_workflow_event(
    config: &GatewayConfig,
    event: &WorkflowEventRecord,
) -> anyhow::Result<()> {
    let path = workflow_events_path(config, &event.workflow_id);
    append_jsonl_record(&path, event)?;
    if let Err(e) = refresh_workflow_graph_markdown(config, &event.workflow_id) {
        tracing::warn!(
            target: "session_timeline",
            workflow_id = %event.workflow_id,
            error = %e,
            "Failed to refresh workflow_graph.md"
        );
    }
    Ok(())
}

/// Write or replace a task record and refresh `workflow.active_task_ids`.
pub fn save_task_run(config: &GatewayConfig, task: &TaskRun) -> anyhow::Result<()> {
    let path = task_run_path(config, &task.workflow_id, &task.task_id);
    write_json_file(&path, task)?;

    let mut run = load_workflow_run(config, &task.workflow_id)?
        .ok_or_else(|| anyhow::anyhow!("workflow '{}' not found", task.workflow_id))?;
    if !run.active_task_ids.contains(&task.task_id) {
        run.active_task_ids.push(task.task_id.clone());
    }
    run.updated_at = now_rfc3339();
    save_workflow_run(config, &run)?;
    Ok(())
}

/// Load a task run if the file exists.
pub fn load_task_run(
    config: &GatewayConfig,
    workflow_id: &str,
    task_id: &str,
) -> anyhow::Result<Option<TaskRun>> {
    let p = task_run_path(config, workflow_id, task_id);
    if !p.exists() {
        return Ok(None);
    }
    Ok(Some(read_json_file(&p)?))
}

/// List all persisted [`TaskRun`] records under `runs/<workflow_id>/tasks/`.
pub fn list_task_runs_for_workflow(
    config: &GatewayConfig,
    workflow_id: &str,
) -> anyhow::Result<Vec<TaskRun>> {
    let dir: PathBuf = workflow_run_dir(config, workflow_id).join("tasks");
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out: Vec<TaskRun> = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        match read_json_file::<TaskRun>(&path) {
            Ok(t) => out.push(t),
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "skip invalid task json");
            }
        }
    }
    out.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.task_id.cmp(&b.task_id))
    });
    Ok(out)
}

/// Update task status (and optional result summary) and append a completion-style event.
pub fn update_task_run_status(
    config: &GatewayConfig,
    workflow_id: &str,
    task_id: &str,
    status: TaskRunStatus,
    result_summary: Option<String>,
) -> anyhow::Result<()> {
    let mut task = load_task_run(config, workflow_id, task_id)?
        .ok_or_else(|| anyhow::anyhow!("task '{}' not in workflow '{}'", task_id, workflow_id))?;
    task.status = status;
    task.updated_at = now_rfc3339();
    task.result_summary = result_summary;
    save_task_run(config, &task)?;

    let event_type = match status {
        TaskRunStatus::Succeeded => "task.completed",
        TaskRunStatus::Failed => "task.failed",
        TaskRunStatus::AwaitingApproval => "task.awaiting_approval",
        TaskRunStatus::Running => "task.started",
        TaskRunStatus::Cancelled => "task.failed",
        _ => "task.updated",
    };
    append_workflow_event(
        config,
        &WorkflowEventRecord {
            event_id: new_event_id(),
            workflow_id: workflow_id.to_string(),
            task_id: Some(task_id.to_string()),
            event_type: event_type.to_string(),
            payload: serde_json::json!({ "status": status }),
            occurred_at: now_rfc3339(),
        },
    )?;

    if let Some(wf) = load_workflow_run(config, workflow_id)? {
        let (causal_action, causal_status) = match status {
            TaskRunStatus::Succeeded => ("workflow.task.completed", EntryStatus::Success),
            TaskRunStatus::Failed | TaskRunStatus::Cancelled => {
                ("workflow.task.failed", EntryStatus::Error)
            }
            TaskRunStatus::AwaitingApproval => {
                ("workflow.task.awaiting_approval", EntryStatus::Success)
            }
            _ => ("workflow.task.updated", EntryStatus::Success),
        };
        crate::scheduler::workflow_causal::mirror_orchestration_event(
            config,
            &wf.root_session_id,
            causal_action,
            causal_status,
            serde_json::json!({
                "workflow_id": workflow_id,
                "task_id": task_id,
                "workflow_event_type": event_type,
                "target_agent_id": task.agent_id,
                "child_session_id": task.session_id,
                "parent_session_id": task.parent_session_id,
            }),
        );

        // Set workflow to BlockedApproval when a task enters AwaitingApproval
        if status == TaskRunStatus::AwaitingApproval
            && wf.status != WorkflowRunStatus::BlockedApproval
        {
            let mut wf_update = wf.clone();
            wf_update.status = WorkflowRunStatus::BlockedApproval;
            wf_update.updated_at = now_rfc3339();
            if let Err(e) = save_workflow_run(config, &wf_update) {
                tracing::warn!(target: "workflow", error = %e, "Failed to set BlockedApproval");
            }
        }

        // Check join condition after terminal task updates
        let is_terminal = matches!(
            status,
            TaskRunStatus::Succeeded | TaskRunStatus::Failed | TaskRunStatus::Cancelled
        );
        if is_terminal && !wf.join_task_ids.is_empty() {
            if let Ok(true) = check_join_condition(config, workflow_id) {
                let mut wf_mut = wf;
                wf_mut.status = WorkflowRunStatus::Resumable;
                wf_mut.updated_at = now_rfc3339();
                if let Err(e) = save_workflow_run(config, &wf_mut) {
                    tracing::warn!(
                        target: "workflow",
                        error = %e,
                        "Failed to mark workflow as Resumable"
                    );
                }
                append_workflow_event(
                    config,
                    &WorkflowEventRecord {
                        event_id: new_event_id(),
                        workflow_id: workflow_id.to_string(),
                        task_id: None,
                        event_type: "workflow.join.satisfied".to_string(),
                        payload: serde_json::json!({
                            "join_task_ids": wf_mut.join_task_ids,
                        }),
                        occurred_at: now_rfc3339(),
                    },
                )?;
                tracing::info!(
                    target: "workflow",
                    workflow_id = %workflow_id,
                    "Join condition satisfied — workflow marked Resumable"
                );

                // Send a signal to the planner session to resume
                if let Err(e) = crate::scheduler::signal::send_workflow_join_satisfied(
                    config,
                    &wf_mut.root_session_id,
                    workflow_id,
                    wf_mut.join_task_ids.clone(),
                ) {
                    tracing::warn!(
                        target: "signal",
                        error = %e,
                        "Failed to send workflow join satisfied signal"
                    );
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Async task queue
// ---------------------------------------------------------------------------

fn queue_dir(config: &GatewayConfig, workflow_id: &str) -> PathBuf {
    workflow_run_dir(config, workflow_id).join("queue")
}

fn queued_task_path(config: &GatewayConfig, workflow_id: &str, task_id: &str) -> PathBuf {
    queue_dir(config, workflow_id).join(format!("{task_id}.json"))
}

/// Enqueue a task for async execution by the scheduler.
/// Also updates the workflow's `queued_task_ids` and `join_task_ids`.
pub fn enqueue_task(config: &GatewayConfig, queued: &QueuedTaskRun) -> anyhow::Result<()> {
    let dir = queue_dir(config, &queued.workflow_id);
    fs::create_dir_all(&dir)?;
    let path = queued_task_path(config, &queued.workflow_id, &queued.task_id);
    write_json_file(&path, queued)?;

    let mut run = load_workflow_run(config, &queued.workflow_id)?
        .ok_or_else(|| anyhow::anyhow!("workflow '{}' not found", queued.workflow_id))?;
    if !run.queued_task_ids.contains(&queued.task_id) {
        run.queued_task_ids.push(queued.task_id.clone());
    }
    if queued.blocks_planner && !run.join_task_ids.contains(&queued.task_id) {
        run.join_task_ids.push(queued.task_id.clone());
    }
    // Set workflow to WaitingChildren when blocking tasks are enqueued
    if queued.blocks_planner && run.status == WorkflowRunStatus::Active {
        run.status = WorkflowRunStatus::WaitingChildren;
    }
    run.updated_at = now_rfc3339();
    save_workflow_run(config, &run)?;

    append_workflow_event(
        config,
        &WorkflowEventRecord {
            event_id: new_event_id(),
            workflow_id: queued.workflow_id.clone(),
            task_id: Some(queued.task_id.clone()),
            event_type: "task.queued".to_string(),
            payload: serde_json::json!({
                "agent_id": queued.agent_id,
                "child_session_id": queued.child_session_id,
                "parent_session_id": queued.parent_session_id,
                "blocks_planner": queued.blocks_planner,
            }),
            occurred_at: now_rfc3339(),
        },
    )?;

    tracing::info!(
        target: "workflow",
        workflow_id = %queued.workflow_id,
        task_id = %queued.task_id,
        agent_id = %queued.agent_id,
        "Task enqueued for async execution"
    );
    Ok(())
}

/// Dequeue (remove from queue) after task execution completes.
pub fn dequeue_task(
    config: &GatewayConfig,
    workflow_id: &str,
    task_id: &str,
) -> anyhow::Result<()> {
    let path = queued_task_path(config, workflow_id, task_id);
    if path.exists() {
        if let Err(e) = fs::remove_file(&path) {
            tracing::warn!(
                target: "workflow",
                path = %path.display(),
                error = %e,
                "Failed to remove queued task file"
            );
        }
    }

    let mut run = match load_workflow_run(config, workflow_id)? {
        Some(r) => r,
        None => return Ok(()),
    };
    run.queued_task_ids.retain(|id| id != task_id);
    run.updated_at = now_rfc3339();
    save_workflow_run(config, &run)?;
    Ok(())
}

/// Load all queued tasks for a workflow.
pub fn load_queued_tasks(
    config: &GatewayConfig,
    workflow_id: &str,
) -> anyhow::Result<Vec<QueuedTaskRun>> {
    let dir = queue_dir(config, workflow_id);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        match read_json_file::<QueuedTaskRun>(&path) {
            Ok(q) => out.push(q),
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "skip invalid queued task json");
            }
        }
    }
    out.sort_by(|a, b| a.enqueued_at.cmp(&b.enqueued_at));
    Ok(out)
}

/// Load ALL queued tasks across all workflows (for the scheduler tick).
/// Scans the runs/ directory for any workflow with a non-empty queue/.
pub fn load_all_queued_tasks(config: &GatewayConfig) -> anyhow::Result<Vec<QueuedTaskRun>> {
    let root = workflows_root(config).join("runs");
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let wf_id = entry.file_name().to_string_lossy().to_string();
        let queued = load_queued_tasks(config, &wf_id)?;
        out.extend(queued);
    }
    out.sort_by(|a, b| a.enqueued_at.cmp(&b.enqueued_at));
    Ok(out)
}

/// Check if a workflow's join condition is satisfied.
/// Returns true if all tasks in `join_task_ids` have reached a terminal status.
pub fn check_join_condition(config: &GatewayConfig, workflow_id: &str) -> anyhow::Result<bool> {
    let run = match load_workflow_run(config, workflow_id)? {
        Some(r) => r,
        None => return Ok(false),
    };
    if run.join_task_ids.is_empty() {
        return Ok(true);
    }
    for task_id in &run.join_task_ids {
        match load_task_run(config, workflow_id, task_id)? {
            Some(task) => match task.status {
                TaskRunStatus::Succeeded | TaskRunStatus::Failed | TaskRunStatus::Cancelled => {
                    continue;
                }
                _ => return Ok(false),
            },
            None => return Ok(false),
        }
    }
    Ok(true)
}

/// Generate a compact, single-line summary of a workflow's current state.
/// Returns `None` if no workflow exists for the given root session.
/// This is intended for injecting into chat at turn boundaries.
pub fn compact_workflow_summary(
    config: &GatewayConfig,
    root_session_id: &str,
) -> anyhow::Result<Option<String>> {
    let wf_id = match resolve_workflow_id_for_root_session(config, root_session_id)? {
        Some(id) => id,
        None => return Ok(None),
    };
    let run = match load_workflow_run(config, &wf_id)? {
        Some(r) => r,
        None => return Ok(None),
    };
    let tasks = list_task_runs_for_workflow(config, &wf_id)?;
    let queued = load_queued_tasks(config, &wf_id)?;

    let mut running = 0usize;
    let mut succeeded = 0usize;
    let mut failed = 0usize;
    let mut other = 0usize;
    for t in &tasks {
        match t.status {
            TaskRunStatus::Running | TaskRunStatus::Runnable => running += 1,
            TaskRunStatus::Succeeded => succeeded += 1,
            TaskRunStatus::Failed | TaskRunStatus::Cancelled => failed += 1,
            _ => other += 1,
        }
    }

    let total = tasks.len() + queued.len();
    if total == 0 {
        return Ok(None);
    }

    let mut parts = Vec::new();
    parts.push(format!("workflow {}", &wf_id));
    if running > 0 {
        parts.push(format!("{} running", running));
    }
    if !queued.is_empty() {
        parts.push(format!("{} queued", queued.len()));
    }
    if succeeded > 0 {
        parts.push(format!("{} done", succeeded));
    }
    if failed > 0 {
        parts.push(format!("{} failed", failed));
    }
    if other > 0 {
        parts.push(format!("{} other", other));
    }

    let status_str = match run.status {
        WorkflowRunStatus::Resumable => " [RESUMABLE]",
        WorkflowRunStatus::BlockedApproval => " [BLOCKED]",
        WorkflowRunStatus::WaitingChildren => " [WAITING]",
        _ => "",
    };

    let mut summary = format!("{}{}", parts.join(" · "), status_str);

    // Consume planner checkpoint on resume: append the last delegation intent
    if run.status == WorkflowRunStatus::Resumable {
        if let Ok(Some(cp)) = load_workflow_checkpoint(config, &wf_id) {
            if !cp.planner_intent.is_empty() {
                summary.push_str(&format!(
                    "\n  last intent (v{}): {}",
                    cp.version, cp.planner_intent
                ));
            }
        }
    }

    Ok(Some(summary))
}

// ---------------------------------------------------------------------------
// In-process workflow event stream (Phase 6)
// ---------------------------------------------------------------------------

use std::sync::mpsc;

/// Handle for an in-process subscription to workflow events.
/// Events are delivered via a `std::sync::mpsc` channel.
pub struct WorkflowEventStream {
    pub workflow_id: String,
    pub root_session_id: String,
    receiver: mpsc::Receiver<WorkflowEventRecord>,
    _poller: std::thread::JoinHandle<()>,
}

impl WorkflowEventStream {
    /// Start streaming events for a workflow. Polls the JSONL file at the given interval.
    pub fn start(
        config: GatewayConfig,
        workflow_id: String,
        root_session_id: String,
        poll_secs: u64,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let wf_id = workflow_id.clone();
        let poller = std::thread::spawn(move || {
            let mut last_offset = 0usize;
            loop {
                std::thread::sleep(std::time::Duration::from_secs(poll_secs.max(1)));
                let path = workflow_events_path(&config, &wf_id);
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let lines: Vec<&str> = content.lines().collect();
                    for line in &lines[last_offset..] {
                        if line.trim().is_empty() {
                            continue;
                        }
                        if let Ok(event) = serde_json::from_str::<WorkflowEventRecord>(line) {
                            if tx.send(event).is_err() {
                                return; // receiver dropped
                            }
                        }
                    }
                    last_offset = lines.len();
                }
            }
        });
        Self {
            workflow_id,
            root_session_id,
            receiver: rx,
            _poller: poller,
        }
    }

    /// Try to receive the next event without blocking.
    pub fn try_recv(&self) -> Option<WorkflowEventRecord> {
        self.receiver.try_recv().ok()
    }

    /// Receive the next event, blocking until one arrives.
    pub fn recv(&self) -> Result<WorkflowEventRecord, mpsc::RecvError> {
        self.receiver.recv()
    }
}

/// Resolve a task_id from a session_id within a workflow.
/// Scans all task runs in the workflow for a matching session_id.
pub fn resolve_task_id_for_session(
    config: &GatewayConfig,
    workflow_id: &str,
    session_id: &str,
) -> anyhow::Result<Option<String>> {
    let tasks = list_task_runs_for_workflow(config, workflow_id)?;
    for task in tasks {
        if task.session_id == session_id {
            return Ok(Some(task.task_id));
        }
    }
    // Also check queued tasks
    let queued = load_queued_tasks(config, workflow_id)?;
    for q in queued {
        if q.child_session_id == session_id {
            return Ok(Some(q.task_id));
        }
    }
    Ok(None)
}

/// Check if the workflow graph has been updated since the given timestamp.
/// Returns true if any workflow events were emitted after `since`.
pub fn workflow_updated_since(config: &GatewayConfig, root_session_id: &str, since: &str) -> bool {
    let wf_id = match resolve_workflow_id_for_root_session(config, root_session_id) {
        Ok(Some(id)) => id,
        _ => return false,
    };
    let events = match load_workflow_events(config, &wf_id) {
        Ok(e) => e,
        Err(_) => return false,
    };
    events.iter().any(|e| e.occurred_at.as_str() > since)
}

// ---------------------------------------------------------------------------
// Durable checkpoints (Phase 3)
// ---------------------------------------------------------------------------

fn checkpoints_dir(config: &GatewayConfig, workflow_id: &str) -> PathBuf {
    workflow_run_dir(config, workflow_id).join("checkpoints")
}

fn workflow_checkpoint_path(config: &GatewayConfig, workflow_id: &str) -> PathBuf {
    checkpoints_dir(config, workflow_id).join("planner.json")
}

fn task_checkpoint_path(config: &GatewayConfig, workflow_id: &str, task_id: &str) -> PathBuf {
    checkpoints_dir(config, workflow_id).join(format!("{task_id}.json"))
}

/// Save a planner-level checkpoint. Increments the version automatically.
pub fn save_workflow_checkpoint(
    config: &GatewayConfig,
    checkpoint: &WorkflowCheckpoint,
) -> anyhow::Result<()> {
    let dir = checkpoints_dir(config, &checkpoint.workflow_id);
    fs::create_dir_all(&dir)?;
    write_json_file(
        &workflow_checkpoint_path(config, &checkpoint.workflow_id),
        checkpoint,
    )?;
    tracing::info!(
        target: "workflow",
        workflow_id = %checkpoint.workflow_id,
        version = checkpoint.version,
        "Saved planner checkpoint"
    );
    Ok(())
}

/// Load the latest planner checkpoint for a workflow.
pub fn load_workflow_checkpoint(
    config: &GatewayConfig,
    workflow_id: &str,
) -> anyhow::Result<Option<WorkflowCheckpoint>> {
    let path = workflow_checkpoint_path(config, workflow_id);
    if !path.exists() {
        return Ok(None);
    }
    read_json_file(&path).map(Some)
}

/// Save a task-level checkpoint. Increments the version automatically.
pub fn save_task_checkpoint(
    config: &GatewayConfig,
    checkpoint: &TaskCheckpoint,
) -> anyhow::Result<()> {
    let dir = checkpoints_dir(config, &checkpoint.workflow_id);
    fs::create_dir_all(&dir)?;
    write_json_file(
        &task_checkpoint_path(config, &checkpoint.workflow_id, &checkpoint.task_id),
        checkpoint,
    )?;
    tracing::info!(
        target: "workflow",
        workflow_id = %checkpoint.workflow_id,
        task_id = %checkpoint.task_id,
        version = checkpoint.version,
        step = %checkpoint.step,
        "Saved task checkpoint"
    );
    Ok(())
}

/// Load a task checkpoint.
pub fn load_task_checkpoint(
    config: &GatewayConfig,
    workflow_id: &str,
    task_id: &str,
) -> anyhow::Result<Option<TaskCheckpoint>> {
    let path = task_checkpoint_path(config, workflow_id, task_id);
    if !path.exists() {
        return Ok(None);
    }
    read_json_file(&path).map(Some)
}

/// Load all task checkpoints for a workflow.
pub fn load_all_task_checkpoints(
    config: &GatewayConfig,
    workflow_id: &str,
) -> anyhow::Result<Vec<TaskCheckpoint>> {
    let dir = checkpoints_dir(config, workflow_id);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if name == "planner" {
            continue; // skip planner checkpoint
        }
        match read_json_file::<TaskCheckpoint>(&path) {
            Ok(cp) => out.push(cp),
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "skip invalid task checkpoint");
            }
        }
    }
    out.sort_by(|a, b| a.task_id.cmp(&b.task_id));
    Ok(out)
}

/// Create a new planner checkpoint from the current workflow state.
/// Auto-increments the version from the existing checkpoint.
pub fn checkpoint_planner(
    config: &GatewayConfig,
    workflow_id: &str,
    planner_intent: String,
    context: serde_json::Value,
) -> anyhow::Result<WorkflowCheckpoint> {
    let run = load_workflow_run(config, workflow_id)?
        .ok_or_else(|| anyhow::anyhow!("workflow '{}' not found", workflow_id))?;

    let prev_version = load_workflow_checkpoint(config, workflow_id)?
        .map(|cp| cp.version)
        .unwrap_or(0);

    let checkpoint = WorkflowCheckpoint {
        workflow_id: workflow_id.to_string(),
        version: prev_version + 1,
        planner_intent,
        pending_task_ids: run.join_task_ids.clone(),
        join_policy: run.join_policy,
        context,
        created_at: now_rfc3339(),
    };
    save_workflow_checkpoint(config, &checkpoint)?;

    append_workflow_event(
        config,
        &WorkflowEventRecord {
            event_id: new_event_id(),
            workflow_id: workflow_id.to_string(),
            task_id: None,
            event_type: "workflow.checkpoint.saved".to_string(),
            payload: serde_json::json!({
                "version": checkpoint.version,
                "planner_intent": checkpoint.planner_intent,
                "pending_task_ids": checkpoint.pending_task_ids,
            }),
            occurred_at: now_rfc3339(),
        },
    )?;

    Ok(checkpoint)
}

/// Create a new task checkpoint. Auto-increments the version.
pub fn checkpoint_task(
    config: &GatewayConfig,
    workflow_id: &str,
    task_id: &str,
    step: String,
    state: serde_json::Value,
) -> anyhow::Result<TaskCheckpoint> {
    let prev_version = load_task_checkpoint(config, workflow_id, task_id)?
        .map(|cp| cp.version)
        .unwrap_or(0);

    let checkpoint = TaskCheckpoint {
        workflow_id: workflow_id.to_string(),
        task_id: task_id.to_string(),
        version: prev_version + 1,
        step: step.clone(),
        state,
        created_at: now_rfc3339(),
    };
    save_task_checkpoint(config, &checkpoint)?;

    append_workflow_event(
        config,
        &WorkflowEventRecord {
            event_id: new_event_id(),
            workflow_id: workflow_id.to_string(),
            task_id: Some(task_id.to_string()),
            event_type: "task.checkpoint.saved".to_string(),
            payload: serde_json::json!({
                "version": checkpoint.version,
                "step": step,
            }),
            occurred_at: now_rfc3339(),
        },
    )?;

    Ok(checkpoint)
}

#[cfg(test)]
mod tests {
    use super::*;
    use autonoetic_types::config::GatewayConfig;
    use std::path::Path;
    use tempfile::tempdir;

    fn test_config(agents_dir: &Path) -> GatewayConfig {
        GatewayConfig {
            agents_dir: agents_dir.to_path_buf(),
            ..GatewayConfig::default()
        }
    }

    #[test]
    fn ensure_workflow_is_idempotent_per_root() {
        let dir = tempdir().unwrap();
        let agents = dir.path().join("agents");
        std::fs::create_dir_all(&agents).unwrap();
        let cfg = test_config(&agents);
        let a =
            ensure_workflow_for_root_session(&cfg, "demo-root", Some("planner.default")).unwrap();
        let b = ensure_workflow_for_root_session(&cfg, "demo-root", Some("other")).unwrap();
        assert_eq!(a.workflow_id, b.workflow_id);
        assert_eq!(a.root_session_id, "demo-root");
        assert_eq!(b.lead_agent_id, "planner.default");
    }

    #[test]
    fn task_roundtrip_and_events_append() {
        let dir = tempdir().unwrap();
        let agents = dir.path().join("agents");
        std::fs::create_dir_all(&agents).unwrap();
        let cfg = test_config(&agents);
        let wf = ensure_workflow_for_root_session(&cfg, "r1", None).unwrap();
        let tid = new_task_id();
        let ts = now_rfc3339();
        let task = TaskRun {
            task_id: tid.clone(),
            workflow_id: wf.workflow_id.clone(),
            agent_id: "coder.default".to_string(),
            session_id: "r1/coder-abc".to_string(),
            parent_session_id: "r1".to_string(),
            status: TaskRunStatus::Running,
            created_at: ts.clone(),
            updated_at: ts,
            source_agent_id: Some("planner.default".to_string()),
            result_summary: None,
            join_group: None,
        };
        save_task_run(&cfg, &task).unwrap();
        update_task_run_status(
            &cfg,
            &wf.workflow_id,
            &tid,
            TaskRunStatus::Succeeded,
            Some("ok".to_string()),
        )
        .unwrap();
        let loaded = load_task_run(&cfg, &wf.workflow_id, &tid).unwrap().unwrap();
        assert_eq!(loaded.status, TaskRunStatus::Succeeded);
        assert_eq!(loaded.result_summary.as_deref(), Some("ok"));
    }

    #[test]
    fn resolve_root_and_load_workflow_events() {
        let dir = tempdir().unwrap();
        let agents = dir.path().join("agents");
        std::fs::create_dir_all(&agents).unwrap();
        let cfg = test_config(&agents);
        let wf = ensure_workflow_for_root_session(&cfg, "root-resolve", None).unwrap();
        let resolved = resolve_workflow_id_for_root_session(&cfg, "root-resolve")
            .unwrap()
            .expect("index");
        assert_eq!(resolved, wf.workflow_id);
        assert!(resolve_workflow_id_for_root_session(&cfg, "unknown-root")
            .unwrap()
            .is_none());
        let events = load_workflow_events(&cfg, &wf.workflow_id).unwrap();
        assert!(!events.is_empty());
        assert_eq!(events[0].event_type, "workflow.started");
    }

    #[test]
    fn list_task_runs_for_workflow_sorts_and_loads() {
        let dir = tempdir().unwrap();
        let agents = dir.path().join("agents");
        std::fs::create_dir_all(&agents).unwrap();
        let cfg = test_config(&agents);
        let wf = ensure_workflow_for_root_session(&cfg, "root-list", None).unwrap();
        let t1 = new_task_id();
        let t2 = new_task_id();
        let ts = now_rfc3339();
        for (tid, agent) in [(&t1, "a.one"), (&t2, "a.two")] {
            let task = TaskRun {
                task_id: (*tid).clone(),
                workflow_id: wf.workflow_id.clone(),
                agent_id: agent.to_string(),
                session_id: format!("root-list/{agent}-x"),
                parent_session_id: "root-list".to_string(),
                status: TaskRunStatus::Running,
                created_at: ts.clone(),
                updated_at: ts.clone(),
                source_agent_id: None,
                result_summary: None,
                join_group: None,
            };
            save_task_run(&cfg, &task).unwrap();
        }
        let listed = list_task_runs_for_workflow(&cfg, &wf.workflow_id).unwrap();
        assert_eq!(listed.len(), 2);
        assert!(listed.iter().any(|t| t.task_id == t1));
        assert!(listed.iter().any(|t| t.task_id == t2));
    }

    #[test]
    fn workflow_graph_md_written_on_event_append() {
        let dir = tempdir().unwrap();
        let agents = dir.path().join("agents");
        std::fs::create_dir_all(&agents).unwrap();
        let cfg = test_config(&agents);
        let wf = ensure_workflow_for_root_session(&cfg, "graph-root", Some("lead.agent")).unwrap();
        let graph_path = crate::execution::gateway_root_dir(&cfg)
            .join("sessions")
            .join("graph-root")
            .join("workflow_graph.md");
        assert!(graph_path.exists());
        let text = std::fs::read_to_string(&graph_path).unwrap();
        assert!(text.contains(&wf.workflow_id));
        assert!(text.contains("graph-root"));
        assert!(text.contains("lead.agent"));
        assert!(text.contains("workflow.started") || text.contains("Recent workflow"));
    }

    // -----------------------------------------------------------------------
    // Async workflow tests (Phase 2–7)
    // -----------------------------------------------------------------------

    #[test]
    fn queue_dequeue_roundtrip() {
        let dir = tempdir().unwrap();
        let agents = dir.path().join("agents");
        std::fs::create_dir_all(&agents).unwrap();
        let cfg = test_config(&agents);
        let wf = ensure_workflow_for_root_session(&cfg, "q-root", None).unwrap();

        let queued = QueuedTaskRun {
            task_id: "task-q1".to_string(),
            workflow_id: wf.workflow_id.clone(),
            agent_id: "coder.default".to_string(),
            message: "Write hello world".to_string(),
            child_session_id: "q-root/coder-q1".to_string(),
            parent_session_id: "q-root".to_string(),
            source_agent_id: "planner.default".to_string(),
            metadata: None,
            join_group: None,
            blocks_planner: true,
            enqueued_at: now_rfc3339(),
        };
        enqueue_task(&cfg, &queued).unwrap();

        // Load queued tasks
        let loaded = load_queued_tasks(&cfg, &wf.workflow_id).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].task_id, "task-q1");

        // Workflow should have queued_task_ids
        let run = load_workflow_run(&cfg, &wf.workflow_id).unwrap().unwrap();
        assert!(run.queued_task_ids.contains(&"task-q1".to_string()));

        // Dequeue
        dequeue_task(&cfg, &wf.workflow_id, "task-q1").unwrap();
        let loaded = load_queued_tasks(&cfg, &wf.workflow_id).unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn parallel_async_enqueue_and_join_condition() {
        let dir = tempdir().unwrap();
        let agents = dir.path().join("agents");
        std::fs::create_dir_all(&agents).unwrap();
        let cfg = test_config(&agents);
        let wf = ensure_workflow_for_root_session(&cfg, "parallel-root", None).unwrap();

        // Enqueue two tasks in the same join group
        for (tid, agent) in [
            ("task-p1", "coder.default"),
            ("task-p2", "researcher.default"),
        ] {
            let queued = QueuedTaskRun {
                task_id: tid.to_string(),
                workflow_id: wf.workflow_id.clone(),
                agent_id: agent.to_string(),
                message: format!("Do {}", tid),
                child_session_id: format!("parallel-root/{}-x", agent),
                parent_session_id: "parallel-root".to_string(),
                source_agent_id: "planner.default".to_string(),
                metadata: None,
                join_group: Some("main".to_string()),
                blocks_planner: true,
                enqueued_at: now_rfc3339(),
            };
            enqueue_task(&cfg, &queued).unwrap();
        }

        // Both should be in queue
        let queued = load_queued_tasks(&cfg, &wf.workflow_id).unwrap();
        assert_eq!(queued.len(), 2);

        // Load all queued tasks across all workflows
        let all_queued = load_all_queued_tasks(&cfg).unwrap();
        assert!(all_queued.len() >= 2);

        // Dequeue both and create TaskRuns
        for tid in ["task-p1", "task-p2"] {
            dequeue_task(&cfg, &wf.workflow_id, tid).unwrap();
            let task = TaskRun {
                task_id: tid.to_string(),
                workflow_id: wf.workflow_id.clone(),
                agent_id: "coder.default".to_string(),
                session_id: format!("parallel-root/{}-x", tid),
                parent_session_id: "parallel-root".to_string(),
                status: TaskRunStatus::Running,
                created_at: now_rfc3339(),
                updated_at: now_rfc3339(),
                source_agent_id: Some("planner.default".to_string()),
                result_summary: None,
                join_group: Some("main".to_string()),
            };
            save_task_run(&cfg, &task).unwrap();
        }

        // Check join: both still Running → not satisfied
        let mut run = load_workflow_run(&cfg, &wf.workflow_id).unwrap().unwrap();
        run.join_task_ids = vec!["task-p1".to_string(), "task-p2".to_string()];
        save_workflow_run(&cfg, &run).unwrap();
        assert!(!check_join_condition(&cfg, &wf.workflow_id).unwrap());

        // Complete first task
        update_task_run_status(
            &cfg,
            &wf.workflow_id,
            "task-p1",
            TaskRunStatus::Succeeded,
            Some("done p1".to_string()),
        )
        .unwrap();

        // Join still not satisfied (task-p2 still running)
        assert!(!check_join_condition(&cfg, &wf.workflow_id).unwrap());

        // Complete second task
        update_task_run_status(
            &cfg,
            &wf.workflow_id,
            "task-p2",
            TaskRunStatus::Succeeded,
            Some("done p2".to_string()),
        )
        .unwrap();

        // Now join should be satisfied
        assert!(check_join_condition(&cfg, &wf.workflow_id).unwrap());

        // Workflow should be Resumable
        let run = load_workflow_run(&cfg, &wf.workflow_id).unwrap().unwrap();
        assert_eq!(run.status, WorkflowRunStatus::Resumable);
    }

    #[test]
    fn join_satisfied_emits_event() {
        let dir = tempdir().unwrap();
        let agents = dir.path().join("agents");
        std::fs::create_dir_all(&agents).unwrap();
        let cfg = test_config(&agents);
        let wf = ensure_workflow_for_root_session(&cfg, "join-ev-root", None).unwrap();

        // Create task and set join condition
        let task = TaskRun {
            task_id: "task-je1".to_string(),
            workflow_id: wf.workflow_id.clone(),
            agent_id: "a".to_string(),
            session_id: "join-ev-root/a-x".to_string(),
            parent_session_id: "join-ev-root".to_string(),
            status: TaskRunStatus::Running,
            created_at: now_rfc3339(),
            updated_at: now_rfc3339(),
            source_agent_id: None,
            result_summary: None,
            join_group: None,
        };
        save_task_run(&cfg, &task).unwrap();

        let mut run = load_workflow_run(&cfg, &wf.workflow_id).unwrap().unwrap();
        run.join_task_ids = vec!["task-je1".to_string()];
        save_workflow_run(&cfg, &run).unwrap();

        // Complete the task → should emit workflow.join.satisfied
        update_task_run_status(
            &cfg,
            &wf.workflow_id,
            "task-je1",
            TaskRunStatus::Succeeded,
            None,
        )
        .unwrap();

        let events = load_workflow_events(&cfg, &wf.workflow_id).unwrap();
        assert!(
            events
                .iter()
                .any(|e| e.event_type == "workflow.join.satisfied"),
            "Expected workflow.join.satisfied event, got: {:?}",
            events.iter().map(|e| &e.event_type).collect::<Vec<_>>()
        );
    }

    #[test]
    fn failed_task_still_satisfies_join() {
        let dir = tempdir().unwrap();
        let agents = dir.path().join("agents");
        std::fs::create_dir_all(&agents).unwrap();
        let cfg = test_config(&agents);
        let wf = ensure_workflow_for_root_session(&cfg, "fail-join-root", None).unwrap();

        for tid in ["task-f1", "task-f2"] {
            let task = TaskRun {
                task_id: tid.to_string(),
                workflow_id: wf.workflow_id.clone(),
                agent_id: "a".to_string(),
                session_id: format!("fail-join-root/{}-x", tid),
                parent_session_id: "fail-join-root".to_string(),
                status: TaskRunStatus::Running,
                created_at: now_rfc3339(),
                updated_at: now_rfc3339(),
                source_agent_id: None,
                result_summary: None,
                join_group: None,
            };
            save_task_run(&cfg, &task).unwrap();
        }

        let mut run = load_workflow_run(&cfg, &wf.workflow_id).unwrap().unwrap();
        run.join_task_ids = vec!["task-f1".to_string(), "task-f2".to_string()];
        save_workflow_run(&cfg, &run).unwrap();

        // Task 1 fails
        update_task_run_status(
            &cfg,
            &wf.workflow_id,
            "task-f1",
            TaskRunStatus::Failed,
            Some("error".to_string()),
        )
        .unwrap();
        assert!(!check_join_condition(&cfg, &wf.workflow_id).unwrap());

        // Task 2 succeeds → join satisfied even though one failed
        update_task_run_status(
            &cfg,
            &wf.workflow_id,
            "task-f2",
            TaskRunStatus::Succeeded,
            None,
        )
        .unwrap();
        assert!(check_join_condition(&cfg, &wf.workflow_id).unwrap());

        let run = load_workflow_run(&cfg, &wf.workflow_id).unwrap().unwrap();
        assert_eq!(run.status, WorkflowRunStatus::Resumable);
    }

    #[test]
    fn compact_workflow_summary_none_when_no_tasks() {
        let dir = tempdir().unwrap();
        let agents = dir.path().join("agents");
        std::fs::create_dir_all(&agents).unwrap();
        let cfg = test_config(&agents);
        let _wf = ensure_workflow_for_root_session(&cfg, "summary-root", None).unwrap();

        let summary = compact_workflow_summary(&cfg, "summary-root").unwrap();
        assert!(summary.is_none());
    }

    #[test]
    fn compact_workflow_summary_counts_tasks() {
        let dir = tempdir().unwrap();
        let agents = dir.path().join("agents");
        std::fs::create_dir_all(&agents).unwrap();
        let cfg = test_config(&agents);
        let wf = ensure_workflow_for_root_session(&cfg, "sum-root", None).unwrap();

        // 1 running, 1 succeeded, 1 queued
        let running = TaskRun {
            task_id: "t-run".to_string(),
            workflow_id: wf.workflow_id.clone(),
            agent_id: "a".to_string(),
            session_id: "sum-root/a-x".to_string(),
            parent_session_id: "sum-root".to_string(),
            status: TaskRunStatus::Running,
            created_at: now_rfc3339(),
            updated_at: now_rfc3339(),
            source_agent_id: None,
            result_summary: None,
            join_group: None,
        };
        save_task_run(&cfg, &running).unwrap();

        let done = TaskRun {
            task_id: "t-done".to_string(),
            workflow_id: wf.workflow_id.clone(),
            agent_id: "b".to_string(),
            session_id: "sum-root/b-x".to_string(),
            parent_session_id: "sum-root".to_string(),
            status: TaskRunStatus::Succeeded,
            created_at: now_rfc3339(),
            updated_at: now_rfc3339(),
            source_agent_id: None,
            result_summary: Some("ok".to_string()),
            join_group: None,
        };
        save_task_run(&cfg, &done).unwrap();

        let queued = QueuedTaskRun {
            task_id: "t-queued".to_string(),
            workflow_id: wf.workflow_id.clone(),
            agent_id: "c".to_string(),
            message: "todo".to_string(),
            child_session_id: "sum-root/c-x".to_string(),
            parent_session_id: "sum-root".to_string(),
            source_agent_id: "planner".to_string(),
            metadata: None,
            join_group: None,
            blocks_planner: true,
            enqueued_at: now_rfc3339(),
        };
        enqueue_task(&cfg, &queued).unwrap();

        let summary = compact_workflow_summary(&cfg, "sum-root").unwrap().unwrap();
        assert!(summary.contains("1 running"), "got: {}", summary);
        assert!(summary.contains("1 queued"), "got: {}", summary);
        assert!(summary.contains("1 done"), "got: {}", summary);
    }

    #[test]
    fn approval_unblocks_task() {
        let dir = tempdir().unwrap();
        let agents = dir.path().join("agents");
        std::fs::create_dir_all(&agents).unwrap();
        let cfg = test_config(&agents);
        let wf = ensure_workflow_for_root_session(&cfg, "appr-root", None).unwrap();

        let task = TaskRun {
            task_id: "task-appr".to_string(),
            workflow_id: wf.workflow_id.clone(),
            agent_id: "coder.default".to_string(),
            session_id: "appr-root/coder-x".to_string(),
            parent_session_id: "appr-root".to_string(),
            status: TaskRunStatus::AwaitingApproval,
            created_at: now_rfc3339(),
            updated_at: now_rfc3339(),
            source_agent_id: Some("planner.default".to_string()),
            result_summary: None,
            join_group: None,
        };
        save_task_run(&cfg, &task).unwrap();

        // Simulate approval unblock (Runnable on approve)
        update_task_run_status(
            &cfg,
            &wf.workflow_id,
            "task-appr",
            TaskRunStatus::Runnable,
            Some("approval_approved".to_string()),
        )
        .unwrap();

        let loaded = load_task_run(&cfg, &wf.workflow_id, "task-appr")
            .unwrap()
            .unwrap();
        assert_eq!(loaded.status, TaskRunStatus::Runnable);
        assert_eq!(loaded.result_summary.as_deref(), Some("approval_approved"));

        // Events should contain task.updated (Runnable maps to task.updated event)
        let events = load_workflow_events(&cfg, &wf.workflow_id).unwrap();
        assert!(events.iter().any(|e| e.event_type == "task.updated"));
    }

    #[test]
    fn approval_reject_fails_task() {
        let dir = tempdir().unwrap();
        let agents = dir.path().join("agents");
        std::fs::create_dir_all(&agents).unwrap();
        let cfg = test_config(&agents);
        let wf = ensure_workflow_for_root_session(&cfg, "rej-root", None).unwrap();

        let task = TaskRun {
            task_id: "task-rej".to_string(),
            workflow_id: wf.workflow_id.clone(),
            agent_id: "coder.default".to_string(),
            session_id: "rej-root/coder-x".to_string(),
            parent_session_id: "rej-root".to_string(),
            status: TaskRunStatus::AwaitingApproval,
            created_at: now_rfc3339(),
            updated_at: now_rfc3339(),
            source_agent_id: None,
            result_summary: None,
            join_group: None,
        };
        save_task_run(&cfg, &task).unwrap();

        // Simulate rejection
        update_task_run_status(
            &cfg,
            &wf.workflow_id,
            "task-rej",
            TaskRunStatus::Failed,
            Some("approval_rejected".to_string()),
        )
        .unwrap();

        let loaded = load_task_run(&cfg, &wf.workflow_id, "task-rej")
            .unwrap()
            .unwrap();
        assert_eq!(loaded.status, TaskRunStatus::Failed);

        let events = load_workflow_events(&cfg, &wf.workflow_id).unwrap();
        assert!(events.iter().any(|e| e.event_type == "task.failed"));
    }

    #[test]
    fn load_all_queued_tasks_across_workflows() {
        let dir = tempdir().unwrap();
        let agents = dir.path().join("agents");
        std::fs::create_dir_all(&agents).unwrap();
        let cfg = test_config(&agents);

        let wf1 = ensure_workflow_for_root_session(&cfg, "multi-root-1", None).unwrap();
        let wf2 = ensure_workflow_for_root_session(&cfg, "multi-root-2", None).unwrap();

        for (wf, tid) in [(&wf1, "t-m1"), (&wf2, "t-m2")] {
            let queued = QueuedTaskRun {
                task_id: tid.to_string(),
                workflow_id: wf.workflow_id.clone(),
                agent_id: "a".to_string(),
                message: "do".to_string(),
                child_session_id: "s".to_string(),
                parent_session_id: "p".to_string(),
                source_agent_id: "planner".to_string(),
                metadata: None,
                join_group: None,
                blocks_planner: false,
                enqueued_at: now_rfc3339(),
            };
            enqueue_task(&cfg, &queued).unwrap();
        }

        let all = load_all_queued_tasks(&cfg).unwrap();
        let ids: Vec<&str> = all.iter().map(|q| q.task_id.as_str()).collect();
        assert!(ids.contains(&"t-m1"));
        assert!(ids.contains(&"t-m2"));
    }

    // -----------------------------------------------------------------------
    // Checkpoint tests (Phase 3)
    // -----------------------------------------------------------------------

    #[test]
    fn planner_checkpoint_roundtrip() {
        let dir = tempdir().unwrap();
        let agents = dir.path().join("agents");
        std::fs::create_dir_all(&agents).unwrap();
        let cfg = test_config(&agents);
        let wf = ensure_workflow_for_root_session(&cfg, "ckpt-root", None).unwrap();

        // No checkpoint initially
        assert!(load_workflow_checkpoint(&cfg, &wf.workflow_id)
            .unwrap()
            .is_none());

        // Create checkpoint
        let cp = checkpoint_planner(
            &cfg,
            &wf.workflow_id,
            "Waiting for coder and researcher results".to_string(),
            serde_json::json!({"delegation": "parallel analysis"}),
        )
        .unwrap();
        assert_eq!(cp.version, 1);
        assert_eq!(
            cp.planner_intent,
            "Waiting for coder and researcher results"
        );

        // Load it back
        let loaded = load_workflow_checkpoint(&cfg, &wf.workflow_id)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.workflow_id, wf.workflow_id);

        // Second checkpoint auto-increments version
        let cp2 = checkpoint_planner(
            &cfg,
            &wf.workflow_id,
            "Processing results".to_string(),
            serde_json::json!({}),
        )
        .unwrap();
        assert_eq!(cp2.version, 2);
    }

    #[test]
    fn task_checkpoint_roundtrip() {
        let dir = tempdir().unwrap();
        let agents = dir.path().join("agents");
        std::fs::create_dir_all(&agents).unwrap();
        let cfg = test_config(&agents);
        let wf = ensure_workflow_for_root_session(&cfg, "ckpt-task-root", None).unwrap();

        // No checkpoint initially
        assert!(load_task_checkpoint(&cfg, &wf.workflow_id, "task-ck1")
            .unwrap()
            .is_none());

        // Create task checkpoint
        let cp = checkpoint_task(
            &cfg,
            &wf.workflow_id,
            "task-ck1",
            "writing_code".to_string(),
            serde_json::json!({"files_written": ["main.py", "utils.py"]}),
        )
        .unwrap();
        assert_eq!(cp.version, 1);
        assert_eq!(cp.step, "writing_code");

        // Load it back
        let loaded = load_task_checkpoint(&cfg, &wf.workflow_id, "task-ck1")
            .unwrap()
            .unwrap();
        assert_eq!(loaded.version, 1);

        // Second checkpoint auto-increments
        let cp2 = checkpoint_task(
            &cfg,
            &wf.workflow_id,
            "task-ck1",
            "running_tests".to_string(),
            serde_json::json!({"tests_run": 5}),
        )
        .unwrap();
        assert_eq!(cp2.version, 2);

        // Load all task checkpoints
        checkpoint_task(
            &cfg,
            &wf.workflow_id,
            "task-ck2",
            "setup".to_string(),
            serde_json::json!({}),
        )
        .unwrap();
        let all = load_all_task_checkpoints(&cfg, &wf.workflow_id).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].task_id, "task-ck1");
        assert_eq!(all[1].task_id, "task-ck2");
    }

    #[test]
    fn checkpoint_planner_captures_join_state() {
        let dir = tempdir().unwrap();
        let agents = dir.path().join("agents");
        std::fs::create_dir_all(&agents).unwrap();
        let cfg = test_config(&agents);
        let wf = ensure_workflow_for_root_session(&cfg, "ckpt-join-root", None).unwrap();

        // Set up join task IDs
        let mut run = load_workflow_run(&cfg, &wf.workflow_id).unwrap().unwrap();
        run.join_task_ids = vec!["task-a".to_string(), "task-b".to_string()];
        run.join_policy = JoinPolicy::AllOf;
        save_workflow_run(&cfg, &run).unwrap();

        // Checkpoint should capture the join state
        let cp = checkpoint_planner(
            &cfg,
            &wf.workflow_id,
            "Delegated research and coding".to_string(),
            serde_json::json!({}),
        )
        .unwrap();
        assert_eq!(cp.pending_task_ids, vec!["task-a", "task-b"]);
        assert_eq!(cp.join_policy, JoinPolicy::AllOf);
    }

    #[test]
    fn checkpoint_events_emitted() {
        let dir = tempdir().unwrap();
        let agents = dir.path().join("agents");
        std::fs::create_dir_all(&agents).unwrap();
        let cfg = test_config(&agents);
        let wf = ensure_workflow_for_root_session(&cfg, "ckpt-ev-root", None).unwrap();

        // Clear initial events
        let initial_count = load_workflow_events(&cfg, &wf.workflow_id).unwrap().len();

        checkpoint_planner(
            &cfg,
            &wf.workflow_id,
            "test".to_string(),
            serde_json::json!({}),
        )
        .unwrap();
        checkpoint_task(
            &cfg,
            &wf.workflow_id,
            "t1",
            "step1".to_string(),
            serde_json::json!({}),
        )
        .unwrap();

        let events = load_workflow_events(&cfg, &wf.workflow_id).unwrap();
        let new_events = &events[initial_count..];
        assert!(new_events
            .iter()
            .any(|e| e.event_type == "workflow.checkpoint.saved"));
        assert!(new_events
            .iter()
            .any(|e| e.event_type == "task.checkpoint.saved"));
    }
}
