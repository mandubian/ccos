use autonoetic_gateway::execution::{gateway_causal_path, GatewayExecutionService};
use autonoetic_gateway::scheduler::{
    append_inbox_event, approve_request, background_state_path, load_approval_requests,
    run_scheduler_tick,
};
use autonoetic_types::background::{
    BackgroundState, ReevaluationState, ScheduledAction, WakeReason,
};
use autonoetic_types::causal_chain::CausalChainEntry;
use autonoetic_types::config::GatewayConfig;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::tempdir;

fn write_background_agent(
    agents_dir: &Path,
    agent_id: &str,
    wake_block: &str,
) -> anyhow::Result<PathBuf> {
    let agent_dir = agents_dir.join(agent_id);
    std::fs::create_dir_all(agent_dir.join("state"))?;
    std::fs::create_dir_all(agent_dir.join("skills"))?;
    let skill = format!(
        "---\nversion: \"1.0\"\nruntime:\n  engine: \"autonoetic\"\n  gateway_version: \"0.1.0\"\n  sdk_version: \"0.1.0\"\n  type: \"stateful\"\n  sandbox: \"bubblewrap\"\n  runtime_lock: \"runtime.lock\"\nagent:\n  id: \"{agent_id}\"\n  name: \"{agent_id}\"\n  description: \"Background integration test agent\"\ncapabilities:\n  - type: BackgroundReevaluation\n    min_interval_secs: 5\n    allow_reasoning: false\n  - type: MemoryWrite\n    scopes: [\"skills/*\", \"state/*\"]\nbackground:\n  enabled: true\n  interval_secs: 5\n  mode: deterministic\n  wake_predicates:\n{wake_block}---\n# Instructions\nBackground integration agent.\n",
    );
    std::fs::write(agent_dir.join("SKILL.md"), skill)?;
    Ok(agent_dir)
}

fn write_background_state(
    config: &GatewayConfig,
    agent_id: &str,
    state: &BackgroundState,
) -> anyhow::Result<()> {
    let path = background_state_path(config, agent_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(state)?)?;
    Ok(())
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

fn read_jsonl_entries(path: &Path) -> anyhow::Result<Vec<CausalChainEntry>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let body = std::fs::read_to_string(path)?;
    body.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(anyhow::Error::from))
        .collect()
}

fn count_action(entries: &[CausalChainEntry], session_id: &str, action: &str) -> usize {
    entries
        .iter()
        .filter(|entry| entry.session_id == session_id && entry.action == action)
        .count()
}

fn background_session(agent_id: &str) -> String {
    format!("background::{agent_id}")
}

#[tokio::test]
async fn test_background_scheduler_idle_timer_through_public_api() -> anyhow::Result<()> {
    let temp = tempdir()?;
    let agents_dir = temp.path().join("agents");
    let config = GatewayConfig {
        agents_dir: agents_dir.clone(),
        background_scheduler_enabled: true,
        ..GatewayConfig::default()
    };
    let agent_id = "idle-public-agent";
    write_background_agent(
        &agents_dir,
        agent_id,
        "    timer: true\n    new_messages: false\n    task_completions: false\n    queued_work: false\n    stale_goals: false\n    retryable_failures: false\n    approval_resolved: false\n",
    )?;

    let session_id = background_session(agent_id);
    write_background_state(
        &config,
        agent_id,
        &BackgroundState {
            agent_id: agent_id.to_string(),
            session_id: session_id.clone(),
            next_due_at: Some((chrono::Utc::now() - chrono::Duration::seconds(1)).to_rfc3339()),
            ..BackgroundState::default()
        },
    )?;

    let execution = Arc::new(GatewayExecutionService::new(config.clone()));
    run_scheduler_tick(execution).await?;

    let state: BackgroundState = serde_json::from_str(&std::fs::read_to_string(
        background_state_path(&config, agent_id),
    )?)?;
    assert!(matches!(
        state.last_wake_reason,
        Some(WakeReason::Timer { .. })
    ));
    assert_eq!(state.last_result.as_deref(), Some("skipped"));

    let gateway_entries = read_jsonl_entries(&gateway_causal_path(&config))?;
    assert_eq!(
        count_action(&gateway_entries, &session_id, "background.should_wake"),
        1
    );
    assert_eq!(
        count_action(&gateway_entries, &session_id, "background.wake.skipped"),
        1
    );
    Ok(())
}

#[tokio::test]
async fn test_background_scheduler_wake_on_new_work_through_public_api() -> anyhow::Result<()> {
    let temp = tempdir()?;
    let agents_dir = temp.path().join("agents");
    let config = GatewayConfig {
        agents_dir: agents_dir.clone(),
        background_scheduler_enabled: true,
        ..GatewayConfig::default()
    };
    let agent_id = "new-work-public-agent";
    let agent_dir = write_background_agent(
        &agents_dir,
        agent_id,
        "    timer: false\n    new_messages: true\n    task_completions: false\n    queued_work: false\n    stale_goals: false\n    retryable_failures: false\n    approval_resolved: false\n",
    )?;
    write_reevaluation_state(
        &agent_dir,
        &ReevaluationState {
            pending_scheduled_action: Some(ScheduledAction::WriteFile {
                path: "skills/from_public_inbox.md".to_string(),
                content: "processed via public tick".to_string(),
                requires_approval: false,
                evidence_ref: None,
            }),
            ..ReevaluationState::default()
        },
    )?;
    append_inbox_event(&config, agent_id, "hello", Some("public-msg"))?;

    let execution = Arc::new(GatewayExecutionService::new(config.clone()));
    run_scheduler_tick(execution).await?;

    assert!(agent_dir
        .join("skills")
        .join("from_public_inbox.md")
        .exists());
    let session_id = background_session(agent_id);
    let gateway_entries = read_jsonl_entries(&gateway_causal_path(&config))?;
    assert_eq!(
        count_action(&gateway_entries, &session_id, "background.wake.requested"),
        1
    );
    assert_eq!(
        count_action(&gateway_entries, &session_id, "background.wake.completed"),
        1
    );

    let state: BackgroundState = serde_json::from_str(&std::fs::read_to_string(
        background_state_path(&config, agent_id),
    )?)?;
    assert_eq!(state.last_result.as_deref(), Some("executed"));
    Ok(())
}

#[tokio::test]
async fn test_background_scheduler_evolution_flow_through_public_api() -> anyhow::Result<()> {
    let temp = tempdir()?;
    let agents_dir = temp.path().join("agents");
    let config = GatewayConfig {
        agents_dir: agents_dir.clone(),
        background_scheduler_enabled: true,
        ..GatewayConfig::default()
    };
    let agent_id = "evolution-public-agent";
    let agent_dir = write_background_agent(
        &agents_dir,
        agent_id,
        "    timer: false\n    new_messages: false\n    task_completions: false\n    queued_work: false\n    stale_goals: true\n    retryable_failures: false\n    approval_resolved: true\n",
    )?;
    write_reevaluation_state(
        &agent_dir,
        &ReevaluationState {
            stale_goal_at: Some((chrono::Utc::now() - chrono::Duration::seconds(1)).to_rfc3339()),
            last_outcome: Some("detected_gap:missing_skill".to_string()),
            pending_scheduled_action: Some(ScheduledAction::WriteFile {
                path: "skills/generated_public_skill.md".to_string(),
                content: "# generated via public flow".to_string(),
                requires_approval: true,
                evidence_ref: None,
            }),
            ..ReevaluationState::default()
        },
    )?;

    let execution = Arc::new(GatewayExecutionService::new(config.clone()));
    run_scheduler_tick(execution.clone()).await?;

    let requests = load_approval_requests(&config)?;
    assert_eq!(requests.len(), 1);
    assert!(!agent_dir
        .join("skills")
        .join("generated_public_skill.md")
        .exists());

    let decision = approve_request(&config, &requests[0].request_id, "integration-test", None)?;
    run_scheduler_tick(execution).await?;

    let generated = agent_dir.join("skills").join("generated_public_skill.md");
    assert!(generated.exists());
    assert_eq!(
        std::fs::read_to_string(generated)?,
        "# generated via public flow"
    );

    let reevaluation: ReevaluationState = serde_json::from_str(&std::fs::read_to_string(
        agent_dir.join("state").join("reevaluation.json"),
    )?)?;
    assert!(reevaluation.pending_scheduled_action.is_none());
    assert!(reevaluation.open_approval_request_ids.is_empty());
    assert_eq!(
        reevaluation.last_outcome.as_deref(),
        Some("background_success")
    );

    let session_id = background_session(agent_id);
    let gateway_entries = read_jsonl_entries(&gateway_causal_path(&config))?;
    assert_eq!(
        count_action(
            &gateway_entries,
            &session_id,
            "background.approval.requested"
        ),
        1
    );
    assert_eq!(
        count_action(
            &gateway_entries,
            &session_id,
            "background.approval.approved"
        ),
        1
    );
    assert_eq!(
        count_action(&gateway_entries, &session_id, "background.wake.completed"),
        1
    );
    assert_eq!(decision.agent_id, agent_id);
    Ok(())
}
