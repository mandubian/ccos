use std::sync::Arc;

use autonoetic_gateway::execution::GatewayExecutionService;
use autonoetic_gateway::scheduler::{process_runnable_workflow_tasks, workflow_store};
use autonoetic_types::config::GatewayConfig;
use autonoetic_types::workflow::{
    QueuedTaskRun, TaskRun, TaskRunStatus, WorkflowRun, WorkflowRunStatus,
};
use tempfile::tempdir;

#[tokio::test]
async fn test_runnable_task_refreshes_stale_queue_message_from_approval_checkpoint(
) -> anyhow::Result<()> {
    let temp = tempdir()?;
    let agents_dir = temp.path().join("agents");
    let gateway_dir = agents_dir.join(".gateway");
    std::fs::create_dir_all(&gateway_dir)?;

    let config = GatewayConfig {
        agents_dir: agents_dir.clone(),
        background_scheduler_enabled: true,
        ..GatewayConfig::default()
    };

    let store = Arc::new(autonoetic_gateway::scheduler::gateway_store::GatewayStore::open(
        &gateway_dir,
    )?);

    let workflow_id = "wf-testresume".to_string();
    let task_id = "task-testresume".to_string();
    let child_session_id = "demo-session/coder.default-abc123".to_string();

    let workflow = WorkflowRun {
        workflow_id: workflow_id.clone(),
        root_session_id: "demo-session".to_string(),
        lead_agent_id: "planner.default".to_string(),
        status: WorkflowRunStatus::WaitingChildren,
        created_at: chrono::Utc::now().to_rfc3339(),
        updated_at: chrono::Utc::now().to_rfc3339(),
        active_task_ids: vec![],
        blocked_task_ids: vec![],
        pending_approval_ids: vec![],
        queued_task_ids: vec![],
        join_policy: Default::default(),
        join_task_ids: vec![task_id.clone()],
    };
    workflow_store::save_workflow_run(&config, Some(store.as_ref()), &workflow)?;

    let task_run = TaskRun {
        task_id: task_id.clone(),
        workflow_id: workflow_id.clone(),
        agent_id: "coder.default".to_string(),
        session_id: child_session_id.clone(),
        parent_session_id: "demo-session".to_string(),
        status: TaskRunStatus::Runnable,
        created_at: chrono::Utc::now().to_rfc3339(),
        updated_at: chrono::Utc::now().to_rfc3339(),
        source_agent_id: Some("planner.default".to_string()),
        result_summary: Some("approval_approved".to_string()),
        join_group: None,
        message: Some("ORIGINAL TASK MESSAGE".to_string()),
        metadata: None,
    };
    workflow_store::save_task_run(&config, Some(store.as_ref()), &task_run)?;

    let resume_message = serde_json::json!({
        "type": "approval_resolved",
        "request_id": "apr-resume1234",
        "status": "approved",
        "message": "Sandbox execution completed successfully.",
    })
    .to_string();

    workflow_store::checkpoint_task(
        &config,
        Some(store.as_ref()),
        &workflow_id,
        &task_id,
        "approval_resolved".to_string(),
        serde_json::json!({
            "request_id": "apr-resume1234",
            "status": "approved",
            "resume_message": resume_message,
        }),
    )?;

    // Seed a stale queued task carrying the old message to simulate
    // crash/recovery where queue already existed before checkpoint-based resume.
    let stale_queued = QueuedTaskRun {
        task_id: task_id.clone(),
        workflow_id: workflow_id.clone(),
        agent_id: "coder.default".to_string(),
        message: "ORIGINAL TASK MESSAGE".to_string(),
        child_session_id: child_session_id.clone(),
        parent_session_id: "demo-session".to_string(),
        source_agent_id: "planner.default".to_string(),
        metadata: None,
        join_group: None,
        blocks_planner: true,
        enqueued_at: chrono::Utc::now().to_rfc3339(),
    };
    workflow_store::enqueue_task(&config, Some(store.as_ref()), &stale_queued)?;

    let execution = Arc::new(GatewayExecutionService::new(config.clone(), Some(store.clone())));
    process_runnable_workflow_tasks(execution).await?;

    let queued_after = workflow_store::load_queued_tasks(&config, Some(store.as_ref()), &workflow_id)?;
    assert_eq!(queued_after.len(), 1);
    assert_eq!(queued_after[0].task_id, task_id);

    let queued_payload: serde_json::Value = serde_json::from_str(&queued_after[0].message)?;
    assert_eq!(queued_payload.get("type").and_then(|v| v.as_str()), Some("approval_resolved"));
    assert_eq!(queued_payload.get("request_id").and_then(|v| v.as_str()), Some("apr-resume1234"));

    Ok(())
}
