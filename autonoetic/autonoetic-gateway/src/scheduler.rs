//! Gateway-owned background scheduler.

use crate::execution::{
    gateway_actor_id, gateway_root_dir, init_gateway_causal_logger, log_gateway_causal_event,
    next_event_seq, GatewayExecutionService,
};
use crate::policy::PolicyEngine;
use crate::runtime::lifecycle::{load_reevaluation_state, persist_reevaluation_state};
use crate::runtime::parser::SkillParser;
use autonoetic_types::background::{
    ApprovalDecision, ApprovalRequest, ApprovalStatus, BackgroundMode, BackgroundPolicy,
    BackgroundState, ReevaluationState, WakeReason,
};
use autonoetic_types::causal_chain::EntryStatus;
use autonoetic_types::config::GatewayConfig;
use autonoetic_types::task_board::{TaskBoardEntry, TaskStatus};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::future::pending;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
    execution: Arc<GatewayExecutionService>,
) -> anyhow::Result<()> {
    let config = execution.config();
    if !config.background_scheduler_enabled {
        tracing::info!("Background scheduler disabled");
        pending::<()>().await;
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

pub async fn run_scheduler_tick(execution: Arc<GatewayExecutionService>) -> anyhow::Result<()> {
    run_scheduler_tick_at(execution, Utc::now()).await
}

async fn run_scheduler_tick_at(
    execution: Arc<GatewayExecutionService>,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    let config = execution.config();
    let agents = crate::agent::scan_agents(&config.agents_dir)?;
    let mut admitted = 0usize;

    for agent in agents {
        if admitted >= config.max_background_due_per_tick.max(1) {
            break;
        }

        let skill_path = agent.dir.join("SKILL.md");
        let skill = std::fs::read_to_string(&skill_path)?;
        let (manifest, _instructions) = SkillParser::parse(&skill)?;
        let Some(background) = manifest.background.clone() else {
            continue;
        };
        if !background.enabled {
            continue;
        }

        let policy = PolicyEngine::new(manifest.clone());
        let Some((cap_min_interval, allow_reasoning)) = policy.background_reevaluation_limits()
        else {
            continue;
        };

        let session_id = background_session_id(&manifest.agent.id);
        let effective_interval = effective_interval_secs(&config, &background, cap_min_interval);
        let state_path = background_state_path(&config, &manifest.agent.id);
        let mut state = load_background_state(&state_path, &manifest.agent.id, &session_id)?;
        if state.next_due_at.is_none() {
            state.next_due_at =
                Some((now + Duration::seconds(effective_interval as i64)).to_rfc3339());
            save_background_state(&state_path, &state)?;
        }

        let reevaluation = load_reevaluation_state(&agent.dir)?;
        let reason = should_wake(
            &config,
            &manifest.agent.id,
            &session_id,
            &background,
            &state,
            &reevaluation,
            now,
        )?;
        log_should_wake(
            &config,
            &session_id,
            &manifest.agent.id,
            &reason,
            effective_interval,
        );

        let Some(reason) = reason else {
            continue;
        };
        admitted += 1;

        handle_due_wake(
            execution.clone(),
            &manifest.agent.id,
            &agent.dir,
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

    Ok(())
}

pub fn pending_approvals_dir(config: &GatewayConfig) -> PathBuf {
    scheduler_root(config).join("approvals").join("pending")
}

pub fn approved_approvals_dir(config: &GatewayConfig) -> PathBuf {
    scheduler_root(config).join("approvals").join("approved")
}

pub fn rejected_approvals_dir(config: &GatewayConfig) -> PathBuf {
    scheduler_root(config).join("approvals").join("rejected")
}

pub fn scheduler_root(config: &GatewayConfig) -> PathBuf {
    gateway_root_dir(config).join("scheduler")
}

pub fn background_state_path(config: &GatewayConfig, agent_id: &str) -> PathBuf {
    scheduler_root(config)
        .join("agents")
        .join(format!("{agent_id}.json"))
}

pub fn inbox_path(config: &GatewayConfig, agent_id: &str) -> PathBuf {
    scheduler_root(config)
        .join("inbox")
        .join(format!("{agent_id}.jsonl"))
}

pub fn task_board_path(config: &GatewayConfig) -> PathBuf {
    scheduler_root(config).join("task_board.jsonl")
}

pub fn append_inbox_event(
    config: &GatewayConfig,
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
    append_jsonl_record(&inbox_path(config, agent_id), &event)?;
    Ok(event)
}

pub fn append_task_board_entry(
    config: &GatewayConfig,
    entry: &TaskBoardEntry,
) -> anyhow::Result<()> {
    append_jsonl_record(&task_board_path(config), entry)
}

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
        decided_at: Utc::now().to_rfc3339(),
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
    if matches!(status, ApprovalStatus::Rejected) {
        let agent_dir = config.agents_dir.join(&decision.agent_id);
        persist_reevaluation_state(&agent_dir, |state| {
            state
                .open_approval_request_ids
                .retain(|existing| existing != &decision.request_id);
            state.pending_scheduled_action = None;
            state.last_outcome = Some("approval_rejected".to_string());
        })?;
        let state_path = background_state_path(config, &decision.agent_id);
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
    let mut event_seq = 0_u64;
    let action = match status {
        ApprovalStatus::Approved => "background.approval.approved",
        ApprovalStatus::Rejected => "background.approval.rejected",
    };
    log_gateway_causal_event(
        &causal_logger,
        &gateway_actor_id(),
        &decision.session_id,
        next_event_seq(&mut event_seq),
        action,
        EntryStatus::Success,
        Some(serde_json::json!({
            "agent_id": decision.agent_id,
            "request_id": decision.request_id,
            "decided_by": decision.decided_by,
            "action_kind": decision.action.kind()
        })),
    );
    Ok(decision)
}

fn effective_interval_secs(
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

fn should_wake(
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
        if approved_approvals_dir(config)
            .join(format!("{request_id}.json"))
            .exists()
        {
            return Ok(Some(WakeReason::ApprovalResolved {
                request_id: request_id.clone(),
            }));
        }
    }

    // If the agent has a pending scheduled action with no open approval yet, always wake
    // it immediately on the next tick (regardless of the timer interval) so it can be
    // promoted to an ApprovalRequest in handle_due_wake.
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
        for event in load_inbox_events(config, agent_id)? {
            if !state.processed_inbox_event_ids.contains(&event.event_id) {
                return Ok(Some(WakeReason::NewMessage {
                    event_id: event.event_id,
                    message: event.message,
                }));
            }
        }
    }

    if background.wake_predicates.task_completions || background.wake_predicates.queued_work {
        for task in load_task_board_entries(config)? {
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

    if background.wake_predicates.retryable_failures {
        if let Some(retry_not_before) = &reevaluation.retry_not_before {
            if parse_timestamp(retry_not_before)? <= now {
                return Ok(Some(WakeReason::RetryableFailure {
                    marker_id: retry_not_before.clone(),
                }));
            }
        }
    }

    if background.wake_predicates.stale_goals {
        if let Some(stale_goal_at) = &reevaluation.stale_goal_at {
            if parse_timestamp(stale_goal_at)? <= now {
                return Ok(Some(WakeReason::StaleGoal {
                    marker_id: stale_goal_at.clone(),
                }));
            }
        }
    }

    if background.wake_predicates.timer {
        if let Some(next_due_at) = &state.next_due_at {
            if parse_timestamp(next_due_at)? <= now {
                return Ok(Some(WakeReason::Timer {
                    due_bucket: next_due_at.clone(),
                }));
            }
        }
    }

    Ok(None)
}

async fn handle_due_wake(
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
    let mut gateway_event_seq = 0_u64;
    let causal_logger = init_gateway_causal_logger(config.as_ref())?;

    if state.active_session_ids.iter().any(|id| id == session_id)
        || state
            .pending_wake_fingerprints
            .iter()
            .any(|existing| existing == &fingerprint)
    {
        log_gateway_causal_event(
            &causal_logger,
            &gateway_actor_id(),
            session_id,
            next_event_seq(&mut gateway_event_seq),
            "background.wake.skipped",
            EntryStatus::Success,
            Some(serde_json::json!({
                "agent_id": agent_id,
                "reason": &reason,
                "skip_reason": "duplicate_or_active"
            })),
        );
        return Ok(());
    }

    state.pending_wake_fingerprints.push(fingerprint.clone());
    state.active_session_ids.push(session_id.to_string());
    save_background_state(&state_path, &state)?;

    log_gateway_causal_event(
        &causal_logger,
        &gateway_actor_id(),
        session_id,
        next_event_seq(&mut gateway_event_seq),
        "background.wake.requested",
        EntryStatus::Success,
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
        let decision: ApprovalDecision = read_json_file(
            &approved_approvals_dir(config.as_ref()).join(format!("{request_id}.json")),
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
                write_json_file(
                    &pending_approvals_dir(config.as_ref())
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
                log_gateway_causal_event(
                    &causal_logger,
                    &gateway_actor_id(),
                    session_id,
                    next_event_seq(&mut gateway_event_seq),
                    "background.approval.requested",
                    EntryStatus::Success,
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
                .spawn_agent_once(agent_id, &kickoff, session_id, None, false)
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
    mark_reason_processed(&mut state, &reason);

    match result {
        Some(Ok(result_body)) => {
            state.retry_not_before = None;
            state.approval_blocked = false;
            clear_reevaluation_after_success(agent_dir, &reason)?;
            save_background_state(&state_path, &state)?;
            log_gateway_causal_event(
                &causal_logger,
                &gateway_actor_id(),
                session_id,
                next_event_seq(&mut gateway_event_seq),
                "background.wake.completed",
                EntryStatus::Success,
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
            log_gateway_causal_event(
                &causal_logger,
                &gateway_actor_id(),
                session_id,
                next_event_seq(&mut gateway_event_seq),
                "background.wake.failed",
                EntryStatus::Error,
                Some(serde_json::json!({
                    "agent_id": agent_id,
                    "reason": &reason,
                    "error": error.to_string()
                })),
            );
        }
        None => {
            save_background_state(&state_path, &state)?;
            log_gateway_causal_event(
                &causal_logger,
                &gateway_actor_id(),
                session_id,
                next_event_seq(&mut gateway_event_seq),
                "background.wake.skipped",
                EntryStatus::Success,
                Some(serde_json::json!({
                    "agent_id": agent_id,
                    "reason": &reason,
                    "skip_reason": "no_executable_work"
                })),
            );
        }
    }

    Ok(())
}

fn clear_reevaluation_after_success(agent_dir: &Path, reason: &WakeReason) -> anyhow::Result<()> {
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

fn log_should_wake(
    config: &GatewayConfig,
    session_id: &str,
    agent_id: &str,
    reason: &Option<WakeReason>,
    effective_interval: u64,
) {
    let Ok(causal_logger) = init_gateway_causal_logger(config) else {
        return;
    };
    let mut gateway_event_seq = 0_u64;
    log_gateway_causal_event(
        &causal_logger,
        &gateway_actor_id(),
        session_id,
        next_event_seq(&mut gateway_event_seq),
        "background.should_wake",
        EntryStatus::Success,
        Some(serde_json::json!({
            "agent_id": agent_id,
            "effective_interval_secs": effective_interval,
            "wake_reason": reason
        })),
    );
}

fn load_background_state(
    path: &Path,
    agent_id: &str,
    session_id: &str,
) -> anyhow::Result<BackgroundState> {
    if !path.exists() {
        return Ok(BackgroundState {
            agent_id: agent_id.to_string(),
            session_id: session_id.to_string(),
            ..BackgroundState::default()
        });
    }
    read_json_file(path)
}

fn save_background_state(path: &Path, state: &BackgroundState) -> anyhow::Result<()> {
    write_json_file(path, state)
}

fn mark_reason_processed(state: &mut BackgroundState, reason: &WakeReason) {
    match reason {
        WakeReason::NewMessage { event_id, .. } => {
            state.processed_inbox_event_ids.push(event_id.clone())
        }
        WakeReason::TaskCompletion { task_id, status }
        | WakeReason::QueuedWork { task_id, status } => {
            state
                .processed_task_keys
                .push(format!("{task_id}:{status}"));
        }
        WakeReason::ApprovalResolved { request_id } => {
            state
                .processed_approval_request_ids
                .push(request_id.clone());
            state
                .pending_approval_request_ids
                .retain(|existing| existing != request_id);
        }
        WakeReason::Timer { .. }
        | WakeReason::StaleGoal { .. }
        | WakeReason::RetryableFailure { .. } => {}
    }
}

fn background_kickoff_prompt(reason: &WakeReason, reevaluation: &ReevaluationState) -> String {
    format!(
        "Background reevaluation wake.\nReason: {:?}\nReevaluation state: {}",
        reason,
        serde_json::to_string_pretty(reevaluation).unwrap_or_else(|_| "{}".to_string())
    )
}

fn wake_fingerprint(agent_id: &str, session_id: &str, reason: &WakeReason) -> String {
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

fn background_session_id(agent_id: &str) -> String {
    format!("background::{agent_id}")
}

fn parse_timestamp(value: &str) -> anyhow::Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(value)?.with_timezone(&Utc))
}

fn load_inbox_events(config: &GatewayConfig, agent_id: &str) -> anyhow::Result<Vec<InboxEvent>> {
    load_jsonl_file(&inbox_path(config, agent_id))
}

fn load_task_board_entries(config: &GatewayConfig) -> anyhow::Result<Vec<TaskBoardEntry>> {
    load_jsonl_file(&task_board_path(config))
}

fn load_json_dir<T>(dir: &Path) -> anyhow::Result<Vec<T>>
where
    T: for<'de> Deserialize<'de>,
{
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.path().is_file() {
            continue;
        }
        entries.push(read_json_file(&entry.path())?);
    }
    Ok(entries)
}

fn load_jsonl_file<T>(path: &Path) -> anyhow::Result<Vec<T>>
where
    T: for<'de> Deserialize<'de>,
{
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(path)?;
    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(anyhow::Error::from))
        .collect()
}

fn read_json_file<T>(path: &Path) -> anyhow::Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let body = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&body)?)
}

fn append_jsonl_record<T>(path: &Path, value: &T) -> anyhow::Result<()>
where
    T: Serialize,
{
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let encoded = serde_json::to_string(value)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    use std::io::Write;
    writeln!(file, "{encoded}")?;
    Ok(())
}

fn write_json_file<T>(path: &Path, value: &T) -> anyhow::Result<()>
where
    T: Serialize,
{
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(value)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use autonoetic_types::background::{BackgroundMode, ScheduledAction, WakePredicates};
    use autonoetic_types::causal_chain::CausalChainEntry;
    use tempfile::tempdir;

    fn config_for(temp: &tempfile::TempDir) -> GatewayConfig {
        GatewayConfig {
            agents_dir: temp.path().join("agents"),
            background_scheduler_enabled: true,
            ..GatewayConfig::default()
        }
    }

    fn write_agent_with_policy(
        temp: &tempfile::TempDir,
        agent_id: &str,
        background_mode: &str,
        wake_predicates: WakePredicates,
    ) -> anyhow::Result<std::path::PathBuf> {
        let agent_dir = temp.path().join("agents").join(agent_id);
        std::fs::create_dir_all(agent_dir.join("state"))?;
        std::fs::create_dir_all(agent_dir.join("skills"))?;
        std::fs::write(
            agent_dir.join("SKILL.md"),
            format!(
                "---\nversion: \"1.0\"\nruntime:\n  engine: \"autonoetic\"\n  gateway_version: \"0.1.0\"\n  sdk_version: \"0.1.0\"\n  type: \"stateful\"\n  sandbox: \"bubblewrap\"\n  runtime_lock: \"runtime.lock\"\nagent:\n  id: \"{agent_id}\"\n  name: \"{agent_id}\"\n  description: \"Background agent\"\ncapabilities:\n  - type: BackgroundReevaluation\n    min_interval_secs: 5\n    allow_reasoning: false\n  - type: MemoryWrite\n    scopes: [\"skills/*\", \"state/*\"]\nbackground:\n  enabled: true\n  interval_secs: 5\n  mode: {background_mode}\n  wake_predicates:\n    timer: {}\n    new_messages: {}\n    task_completions: {}\n    queued_work: {}\n    stale_goals: {}\n    retryable_failures: {}\n    approval_resolved: {}\n---\n# Instructions\nBackground agent.\n",
                wake_predicates.timer,
                wake_predicates.new_messages,
                wake_predicates.task_completions,
                wake_predicates.queued_work,
                wake_predicates.stale_goals,
                wake_predicates.retryable_failures,
                wake_predicates.approval_resolved,
            ),
        )?;
        Ok(agent_dir)
    }

    fn write_agent(temp: &tempfile::TempDir, background_mode: &str) -> anyhow::Result<String> {
        let agent_id = "agent-bg";
        write_agent_with_policy(
            temp,
            agent_id,
            background_mode,
            WakePredicates {
                timer: true,
                stale_goals: true,
                approval_resolved: true,
                ..WakePredicates::default()
            },
        )?;
        Ok(agent_id.to_string())
    }

    fn write_reevaluation_state(
        agent_dir: &Path,
        reevaluation: &ReevaluationState,
    ) -> anyhow::Result<()> {
        std::fs::write(
            agent_dir.join("state").join("reevaluation.json"),
            serde_json::to_string_pretty(reevaluation)?,
        )?;
        Ok(())
    }

    fn read_gateway_entries(config: &GatewayConfig) -> anyhow::Result<Vec<CausalChainEntry>> {
        let path = crate::execution::gateway_causal_path(config);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let body = std::fs::read_to_string(path)?;
        body.lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).map_err(anyhow::Error::from))
            .collect()
    }

    fn count_gateway_action(entries: &[CausalChainEntry], session_id: &str, action: &str) -> usize {
        entries
            .iter()
            .filter(|entry| entry.session_id == session_id && entry.action == action)
            .count()
    }

    fn executed_output_count(
        temp: &tempfile::TempDir,
        agent_ids: &[&str],
        file_name: &str,
    ) -> usize {
        agent_ids
            .iter()
            .filter(|agent_id| {
                temp.path()
                    .join("agents")
                    .join(agent_id)
                    .join("skills")
                    .join(file_name)
                    .exists()
            })
            .count()
    }

    #[test]
    fn test_should_wake_for_due_timer() {
        let now = Utc::now();
        let background = BackgroundPolicy {
            enabled: true,
            interval_secs: 5,
            mode: BackgroundMode::Deterministic,
            wake_predicates: WakePredicates::default(),
        };
        let state = BackgroundState {
            next_due_at: Some((now - Duration::seconds(1)).to_rfc3339()),
            ..BackgroundState::default()
        };
        let reevaluation = ReevaluationState::default();
        let config = GatewayConfig::default();
        let reason = should_wake(
            &config,
            "agent",
            "background::agent",
            &background,
            &state,
            &reevaluation,
            now,
        )
        .expect("should_wake should succeed");
        assert!(matches!(reason, Some(WakeReason::Timer { .. })));
    }

    #[test]
    fn test_should_wake_for_unread_inbox_event() {
        let temp = tempdir().expect("tempdir should create");
        let config = config_for(&temp);
        let agent_id = "agent";
        std::fs::create_dir_all(inbox_path(&config, agent_id).parent().unwrap()).unwrap();
        std::fs::write(
            inbox_path(&config, agent_id),
            serde_json::to_string(&InboxEvent {
                event_id: "evt-1".to_string(),
                message: "hello".to_string(),
                session_id: None,
                created_at: None,
            })
            .unwrap(),
        )
        .unwrap();
        let background = BackgroundPolicy {
            enabled: true,
            interval_secs: 5,
            mode: BackgroundMode::Deterministic,
            wake_predicates: WakePredicates {
                timer: false,
                new_messages: true,
                ..WakePredicates::default()
            },
        };
        let reason = should_wake(
            &config,
            agent_id,
            "background::agent",
            &background,
            &BackgroundState::default(),
            &ReevaluationState::default(),
            Utc::now(),
        )
        .expect("should_wake should succeed");
        assert!(matches!(reason, Some(WakeReason::NewMessage { .. })));
    }

    #[test]
    fn test_should_wake_for_approved_request_even_without_predicate() {
        let temp = tempdir().expect("tempdir should create");
        let config = config_for(&temp);
        let request_id = "req-1";
        std::fs::create_dir_all(approved_approvals_dir(&config)).unwrap();
        std::fs::write(
            approved_approvals_dir(&config).join(format!("{request_id}.json")),
            serde_json::to_string(&ApprovalDecision {
                request_id: request_id.to_string(),
                agent_id: "agent".to_string(),
                session_id: "background::agent".to_string(),
                action: ScheduledAction::WriteFile {
                    path: "skills/generated.md".to_string(),
                    content: "approved".to_string(),
                    requires_approval: true,
                    evidence_ref: None,
                },
                status: ApprovalStatus::Approved,
                decided_at: Utc::now().to_rfc3339(),
                decided_by: "cli".to_string(),
                reason: None,
            })
            .unwrap(),
        )
        .unwrap();
        let background = BackgroundPolicy {
            enabled: true,
            interval_secs: 5,
            mode: BackgroundMode::Deterministic,
            wake_predicates: WakePredicates {
                approval_resolved: false,
                timer: false,
                ..WakePredicates::default()
            },
        };
        let reevaluation = ReevaluationState {
            open_approval_request_ids: vec![request_id.to_string()],
            ..ReevaluationState::default()
        };
        let reason = should_wake(
            &config,
            "agent",
            "background::agent",
            &background,
            &BackgroundState::default(),
            &reevaluation,
            Utc::now(),
        )
        .expect("should_wake should succeed");
        assert!(matches!(reason, Some(WakeReason::ApprovalResolved { .. })));
    }

    #[tokio::test]
    async fn test_scheduler_idle_cadence_records_timer_wake_without_executable_work() {
        let temp = tempdir().expect("tempdir should create");
        let config = config_for(&temp);
        let agent_id = "idle-agent";
        write_agent_with_policy(
            &temp,
            agent_id,
            "deterministic",
            WakePredicates {
                timer: true,
                ..WakePredicates::default()
            },
        )
        .expect("agent should write");

        let execution = Arc::new(GatewayExecutionService::new(config.clone()));
        let now = Utc::now();
        run_scheduler_tick_at(execution.clone(), now)
            .await
            .expect("first tick should seed due time");
        run_scheduler_tick_at(execution, now + Duration::seconds(61))
            .await
            .expect("second tick should evaluate due timer");

        let session_id = background_session_id(agent_id);
        let state = load_background_state(
            &background_state_path(&config, agent_id),
            agent_id,
            &session_id,
        )
        .expect("background state should load");
        assert!(matches!(
            state.last_wake_reason,
            Some(WakeReason::Timer { .. })
        ));
        assert_eq!(state.last_result.as_deref(), Some("skipped"));

        let entries = read_gateway_entries(&config).expect("gateway entries should load");
        assert!(entries.iter().any(|entry| {
            entry.session_id == session_id
                && entry.action == "background.wake.skipped"
                && entry
                    .payload
                    .as_ref()
                    .and_then(|payload| payload.get("skip_reason"))
                    .and_then(|value| value.as_str())
                    == Some("no_executable_work")
        }));
    }

    #[tokio::test]
    async fn test_scheduler_wakes_on_new_work_and_executes_pending_action() {
        let temp = tempdir().expect("tempdir should create");
        let config = config_for(&temp);
        let agent_id = "new-work-agent";
        let agent_dir = write_agent_with_policy(
            &temp,
            agent_id,
            "deterministic",
            WakePredicates {
                timer: false,
                new_messages: true,
                ..WakePredicates::default()
            },
        )
        .expect("agent should write");
        write_reevaluation_state(
            &agent_dir,
            &ReevaluationState {
                pending_scheduled_action: Some(ScheduledAction::WriteFile {
                    path: "skills/from_inbox.md".to_string(),
                    content: "processed inbox work".to_string(),
                    requires_approval: false,
                    evidence_ref: None,
                }),
                ..ReevaluationState::default()
            },
        )
        .expect("reevaluation state should write");
        let event = append_inbox_event(&config, agent_id, "hello", Some("session-msg"))
            .expect("inbox event should append");

        let execution = Arc::new(GatewayExecutionService::new(config.clone()));
        run_scheduler_tick_at(execution, Utc::now())
            .await
            .expect("tick should process inbox work");

        assert!(agent_dir.join("skills").join("from_inbox.md").exists());
        let session_id = background_session_id(agent_id);
        let state = load_background_state(
            &background_state_path(&config, agent_id),
            agent_id,
            &session_id,
        )
        .expect("background state should load");
        assert!(state.processed_inbox_event_ids.contains(&event.event_id));
        assert!(matches!(
            state.last_wake_reason,
            Some(WakeReason::NewMessage { ref event_id, .. }) if event_id == &event.event_id
        ));
        assert_eq!(state.last_result.as_deref(), Some("executed"));
    }

    #[tokio::test]
    async fn test_scheduler_dedups_rapid_ticks_for_same_new_work_signal() {
        let temp = tempdir().expect("tempdir should create");
        let config = config_for(&temp);
        let agent_id = "dedup-agent";
        let agent_dir = write_agent_with_policy(
            &temp,
            agent_id,
            "deterministic",
            WakePredicates {
                timer: false,
                new_messages: true,
                ..WakePredicates::default()
            },
        )
        .expect("agent should write");
        write_reevaluation_state(
            &agent_dir,
            &ReevaluationState {
                pending_scheduled_action: Some(ScheduledAction::WriteFile {
                    path: "skills/dedup.md".to_string(),
                    content: "run once".to_string(),
                    requires_approval: false,
                    evidence_ref: None,
                }),
                ..ReevaluationState::default()
            },
        )
        .expect("reevaluation state should write");
        append_inbox_event(&config, agent_id, "hello", Some("session-dedup"))
            .expect("inbox event should append");

        let execution = Arc::new(GatewayExecutionService::new(config.clone()));
        let now = Utc::now();
        run_scheduler_tick_at(execution.clone(), now)
            .await
            .expect("first tick should execute work once");
        run_scheduler_tick_at(execution, now + Duration::milliseconds(1))
            .await
            .expect("second tick should not duplicate work");

        let session_id = background_session_id(agent_id);
        let entries = read_gateway_entries(&config).expect("gateway entries should load");
        assert_eq!(
            count_gateway_action(&entries, &session_id, "background.wake.requested"),
            1
        );
        assert_eq!(
            count_gateway_action(&entries, &session_id, "background.wake.completed"),
            1
        );
        assert!(agent_dir.join("skills").join("dedup.md").exists());
    }

    #[tokio::test]
    async fn test_scheduler_pauses_on_approval_without_duplicate_requests() {
        let temp = tempdir().expect("tempdir should create");
        let config = config_for(&temp);
        let agent_id = "approval-agent";
        let agent_dir = write_agent_with_policy(
            &temp,
            agent_id,
            "deterministic",
            WakePredicates {
                timer: false,
                stale_goals: true,
                approval_resolved: true,
                ..WakePredicates::default()
            },
        )
        .expect("agent should write");
        let target_file = agent_dir.join("skills").join("candidate.md");
        write_reevaluation_state(
            &agent_dir,
            &ReevaluationState {
                stale_goal_at: Some((Utc::now() - Duration::seconds(1)).to_rfc3339()),
                pending_scheduled_action: Some(ScheduledAction::WriteFile {
                    path: "skills/candidate.md".to_string(),
                    content: "candidate".to_string(),
                    requires_approval: true,
                    evidence_ref: None,
                }),
                ..ReevaluationState::default()
            },
        )
        .expect("reevaluation state should write");

        let execution = Arc::new(GatewayExecutionService::new(config.clone()));
        let now = Utc::now();
        run_scheduler_tick_at(execution.clone(), now)
            .await
            .expect("first tick should request approval");
        run_scheduler_tick_at(execution, now + Duration::seconds(1))
            .await
            .expect("second tick should remain paused on approval");

        let requests = load_approval_requests(&config).expect("requests should load");
        assert_eq!(requests.len(), 1);
        assert!(!target_file.exists());

        let session_id = background_session_id(agent_id);
        let state = load_background_state(
            &background_state_path(&config, agent_id),
            agent_id,
            &session_id,
        )
        .expect("background state should load");
        assert!(state.approval_blocked);
        assert_eq!(state.pending_approval_request_ids.len(), 1);

        let entries = read_gateway_entries(&config).expect("gateway entries should load");
        assert_eq!(
            count_gateway_action(&entries, &session_id, "background.approval.requested"),
            1
        );
    }

    #[tokio::test]
    async fn test_scheduler_throttles_many_due_agents_per_tick() {
        let temp = tempdir().expect("tempdir should create");
        let mut config = config_for(&temp);
        config.max_background_due_per_tick = 2;
        let agent_ids = ["due-agent-a", "due-agent-b", "due-agent-c"];

        for agent_id in agent_ids {
            let agent_dir = write_agent_with_policy(
                &temp,
                agent_id,
                "deterministic",
                WakePredicates {
                    timer: false,
                    stale_goals: true,
                    ..WakePredicates::default()
                },
            )
            .expect("agent should write");
            write_reevaluation_state(
                &agent_dir,
                &ReevaluationState {
                    stale_goal_at: Some((Utc::now() - Duration::seconds(1)).to_rfc3339()),
                    pending_scheduled_action: Some(ScheduledAction::WriteFile {
                        path: "skills/throttled.md".to_string(),
                        content: format!("output for {agent_id}"),
                        requires_approval: false,
                        evidence_ref: None,
                    }),
                    ..ReevaluationState::default()
                },
            )
            .expect("reevaluation state should write");
        }

        let execution = Arc::new(GatewayExecutionService::new(config.clone()));
        let now = Utc::now();
        run_scheduler_tick_at(execution.clone(), now)
            .await
            .expect("first tick should admit limited due agents");
        assert_eq!(executed_output_count(&temp, &agent_ids, "throttled.md"), 2);

        run_scheduler_tick_at(execution, now + Duration::seconds(1))
            .await
            .expect("second tick should pick up the remaining due agent");
        assert_eq!(executed_output_count(&temp, &agent_ids, "throttled.md"), 3);
    }

    #[tokio::test]
    async fn test_scheduler_evolution_flow_requests_approval_then_executes_approved_artifact_write()
    {
        let temp = tempdir().expect("tempdir should create");
        let config = config_for(&temp);
        let agent_id = write_agent(&temp, "deterministic").expect("agent should write");
        let agent_dir = temp.path().join("agents").join(&agent_id);
        write_reevaluation_state(
            &agent_dir,
            &ReevaluationState {
                stale_goal_at: Some((Utc::now() - Duration::seconds(1)).to_rfc3339()),
                pending_scheduled_action: Some(ScheduledAction::WriteFile {
                    path: "skills/generated_skill.md".to_string(),
                    content: "# candidate".to_string(),
                    requires_approval: true,
                    evidence_ref: None,
                }),
                last_outcome: Some("detected_gap:missing_skill".to_string()),
                ..ReevaluationState::default()
            },
        )
        .expect("reevaluation state should write");
        let generated_skill = agent_dir.join("skills").join("generated_skill.md");

        let execution = Arc::new(GatewayExecutionService::new(config.clone()));
        run_scheduler_tick_at(execution.clone(), Utc::now())
            .await
            .expect("tick should succeed");

        let requests = load_approval_requests(&config).expect("requests should load");
        assert_eq!(requests.len(), 1);
        assert!(!generated_skill.exists());

        let decision = approve_request(&config, &requests[0].request_id, "cli", None)
            .expect("approval should succeed");
        assert!(matches!(decision.status, ApprovalStatus::Approved));

        run_scheduler_tick_at(execution, Utc::now())
            .await
            .expect("second tick should succeed");
        assert!(generated_skill.exists());
        assert_eq!(
            std::fs::read_to_string(&generated_skill).expect("generated skill should read"),
            "# candidate"
        );

        let reevaluation = load_reevaluation_state(&agent_dir).expect("reevaluation should load");
        assert!(reevaluation.pending_scheduled_action.is_none());
        assert!(reevaluation.open_approval_request_ids.is_empty());
        assert_eq!(
            reevaluation.last_outcome.as_deref(),
            Some("background_success")
        );

        let session_id = background_session_id(&agent_id);
        let background_state = load_background_state(
            &background_state_path(&config, &agent_id),
            &agent_id,
            &session_id,
        )
        .expect("background state should load");
        assert!(!background_state.approval_blocked);
        assert!(background_state.pending_approval_request_ids.is_empty());
        assert_eq!(
            background_state.processed_approval_request_ids,
            vec![decision.request_id.clone()]
        );

        let entries = read_gateway_entries(&config).expect("gateway entries should load");
        assert_eq!(
            count_gateway_action(&entries, &session_id, "background.approval.requested"),
            1
        );
        assert_eq!(
            count_gateway_action(&entries, &session_id, "background.approval.approved"),
            1
        );
        assert_eq!(
            count_gateway_action(&entries, &session_id, "background.wake.completed"),
            1
        );
    }

    #[tokio::test]
    async fn test_scheduler_dedups_active_session() {
        let temp = tempdir().expect("tempdir should create");
        let config = config_for(&temp);
        let agent_id = write_agent(&temp, "deterministic").expect("agent should write");
        let state_path = background_state_path(&config, &agent_id);
        save_background_state(
            &state_path,
            &BackgroundState {
                agent_id: agent_id.clone(),
                session_id: background_session_id(&agent_id),
                active_session_ids: vec![background_session_id(&agent_id)],
                next_due_at: Some((Utc::now() - Duration::seconds(1)).to_rfc3339()),
                ..BackgroundState::default()
            },
        )
        .unwrap();

        let execution = Arc::new(GatewayExecutionService::new(config.clone()));
        run_scheduler_tick_at(execution, Utc::now())
            .await
            .expect("tick should succeed");

        let state = load_background_state(
            &background_state_path(&config, &agent_id),
            &agent_id,
            &background_session_id(&agent_id),
        )
        .unwrap();
        assert!(state
            .active_session_ids
            .contains(&background_session_id(&agent_id)));
    }
}
