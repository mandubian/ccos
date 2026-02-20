use chrono::{DateTime, Utc};
use croner::Cron;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tracing::{error, info, warn};

use crate::chat::gateway::GatewayState;
use crate::chat::run::{RunState, SharedRunStore};

pub struct Scheduler {
    run_store: SharedRunStore,
}

impl Scheduler {
    pub fn new(run_store: SharedRunStore) -> Self {
        Self { run_store }
    }

    pub(crate) async fn start(self: Arc<Self>, state: Arc<GatewayState>) {
        info!("[Scheduler] Starting background scheduler loop...");
        // Keep scheduling precision high enough for second-level cron expressions.
        let mut tick = interval(Duration::from_secs(1));

        loop {
            tick.tick().await;
            self.check_scheduled_runs(&state).await;
        }
    }

    /// Check for scheduled runs that are due to be triggered
    pub(crate) async fn check_scheduled_runs(&self, state: &Arc<GatewayState>) {
        let mut runs_to_kickoff = Vec::new();
        let mut sessions_selected = HashSet::new();

        {
            let store = self.run_store.lock().unwrap();
            let now = Utc::now();

            for run in store.get_all_runs() {
                if matches!(run.state, RunState::Scheduled) {
                    if sessions_selected.contains(&run.session_id) {
                        continue;
                    }

                    if let Some(next_run) = run.next_run_at {
                        if next_run <= now {
                            // SKIP if session is blocked by an Active or PausedApproval run
                            if store
                                .get_blocking_run_for_session(&run.session_id)
                                .is_some()
                            {
                                // We don't log here to avoid log spam every 10s.
                                // It will eventually trigger once the session is free.
                                continue;
                            }

                            runs_to_kickoff.push((
                                run.session_id.clone(),
                                run.id.clone(),
                                run.goal.clone(),
                                Some(run.budget.clone()),
                                run.schedule.clone(),
                                run.trigger_capability_id.clone(),
                                run.trigger_inputs.clone(),
                            ));
                            sessions_selected.insert(run.session_id.clone());
                        }
                    }
                }
            }
        }

        for (session_id, run_id, goal, budget, schedule, trigger_cap, trigger_inputs) in
            runs_to_kickoff
        {
            info!(
                "[Scheduler] Triggering scheduled run {} for session {}",
                run_id, session_id
            );

            // 1. Transition to Active
            // For recurring tasks, we keep the run Active until it completes successfully.
            // The next run will be created only after the current run succeeds.
            // If the run needs approval, it will transition to PausedApproval instead of Done.
            {
                let mut store = self.run_store.lock().unwrap();
                store.update_run_state(&run_id, RunState::Active);

                // Store the schedule info in the run for later use (creating next run on completion)
                if let Some(ref sched) = schedule {
                    if let Some(run) = store.get_run_mut(&run_id) {
                        run.schedule = Some(sched.clone());
                    }
                }
            }

            // 2. Direct Capability Execution or Spawn Agent
            if let Some(cap_id) = trigger_cap {
                info!(
                    "[Scheduler] Executing direct capability {} for run {}",
                    cap_id, run_id
                );

                let inputs = if let Some(json_inputs) = trigger_inputs {
                    crate::utils::value_conversion::json_to_rtfs_value(&json_inputs).unwrap_or(
                        rtfs::runtime::values::Value::Map(std::collections::HashMap::new()),
                    )
                } else {
                    rtfs::runtime::values::Value::Map(std::collections::HashMap::new())
                };

                // Inject session token if it's a python execution
                if cap_id == "ccos.execute.python" {
                    if let Some(_token) = state.session_registry.get_token(&session_id).await {
                        // We could inject it into inputs or rely on the marketplace/executor to pick it up from env
                        // For now, let's just log that we would inject it.
                        // The actual injection happens in the BubblewrapSandbox/SandboxedExecutor if we pass it through.
                        info!(
                            "[Scheduler] Would inject token for session {} into python sandbox",
                            session_id
                        );
                    }
                }

                let marketplace = state.marketplace.clone();
                let run_store = self.run_store.clone();
                let run_id_for_task = run_id.clone();
                let state_for_task = state.clone();
                let session_id_for_task = session_id.clone();
                let cap_id_for_task = cap_id.clone();

                tokio::spawn(async move {
                    let step_id = format!("{}-direct-{}", run_id_for_task, uuid::Uuid::new_v4());
                    match marketplace.execute_capability(&cap_id, &inputs).await {
                        Ok(result) => {
                            info!(
                                "[Scheduler] Direct capability {} for run {} completed: {:?}",
                                cap_id_for_task, run_id_for_task, result
                            );

                            // Extract stdout / success from result for chat notification
                            let stdout = match &result {
                                rtfs::runtime::values::Value::Map(m) => m
                                    .get(&rtfs::ast::MapKey::Keyword(rtfs::ast::Keyword(
                                        "stdout".to_string(),
                                    )))
                                    .or_else(|| {
                                        m.get(&rtfs::ast::MapKey::String("stdout".to_string()))
                                    })
                                    .and_then(|v| v.as_string())
                                    .map(|s| s.to_string()),
                                _ => None,
                            };
                            let success = match &result {
                                rtfs::runtime::values::Value::Map(m) => m
                                    .get(&rtfs::ast::MapKey::Keyword(rtfs::ast::Keyword(
                                        "success".to_string(),
                                    )))
                                    .or_else(|| {
                                        m.get(&rtfs::ast::MapKey::String("success".to_string()))
                                    })
                                    .and_then(|v| match v {
                                        rtfs::runtime::values::Value::Boolean(b) => Some(*b),
                                        _ => None,
                                    })
                                    .unwrap_or(true),
                                _ => true,
                            };

                            // Record capability execution to causal chain (event pane)
                            let mut meta = std::collections::HashMap::new();
                            meta.insert(
                                "capability_id".to_string(),
                                rtfs::runtime::values::Value::String(cap_id_for_task.clone()),
                            );
                            meta.insert(
                                "success".to_string(),
                                rtfs::runtime::values::Value::Boolean(success),
                            );
                            if let Some(ref out) = stdout {
                                meta.insert(
                                    "stdout".to_string(),
                                    rtfs::runtime::values::Value::String(out.clone()),
                                );
                            }
                            let _ = crate::chat::gateway::record_run_event(
                                &state_for_task,
                                &session_id_for_task,
                                &run_id_for_task,
                                &step_id,
                                "capability.direct.complete",
                                meta,
                            )
                            .await;

                            // Send stdout output to chat so it's visible in the messages pane.
                            // Use connector.send() directly (NOT push_message_to_session which
                            // delivers to the agent inbox and wakes up the LLM agent).
                            if let Some(output) = stdout.filter(|s| !s.trim().is_empty()) {
                                let channel_id = session_id_for_task
                                    .split(':')
                                    .nth(1)
                                    .unwrap_or("general")
                                    .to_string();
                                let outbound = crate::chat::connector::OutboundRequest {
                                    channel_id,
                                    content: output.trim().to_string(),
                                    reply_to: None,
                                    metadata: None,
                                };
                                let _ = state_for_task
                                    .connector
                                    .send(&state_for_task.connector_handle, outbound)
                                    .await;
                            }

                            let next_run_id = {
                                let mut store = run_store.lock().unwrap();
                                store.finish_run_and_schedule_next(
                                    &run_id_for_task,
                                    RunState::Done,
                                    |sched| Self::calculate_next_run(sched),
                                )
                            };
                            if let Some(next_run_id) = next_run_id {
                                info!(
                                    "[Scheduler] Created recurring follow-up run {} after successful direct capability run {}",
                                    next_run_id, run_id_for_task
                                );
                            }
                        }
                        Err(e) => {
                            error!(
                                "[Scheduler] Direct capability {} for run {} failed: {}",
                                cap_id_for_task, run_id_for_task, e
                            );
                            // Record failure to causal chain
                            let mut meta = std::collections::HashMap::new();
                            meta.insert(
                                "capability_id".to_string(),
                                rtfs::runtime::values::Value::String(cap_id_for_task.clone()),
                            );
                            meta.insert(
                                "success".to_string(),
                                rtfs::runtime::values::Value::Boolean(false),
                            );
                            meta.insert(
                                "error".to_string(),
                                rtfs::runtime::values::Value::String(e.to_string()),
                            );
                            let _ = crate::chat::gateway::record_run_event(
                                &state_for_task,
                                &session_id_for_task,
                                &run_id_for_task,
                                &step_id,
                                "capability.direct.failed",
                                meta,
                            )
                            .await;
                            let state = RunState::Failed {
                                error: format!(
                                    "direct capability '{}' failed: {}",
                                    cap_id_for_task, e
                                ),
                            };
                            let mut store = run_store.lock().unwrap();
                            let next_run_id = store.finish_run_and_schedule_next(
                                &run_id_for_task,
                                state,
                                |sched| Self::calculate_next_run(sched),
                            );
                            if let Some(next_run_id) = next_run_id {
                                info!(
                                    "[Scheduler] Created recurring follow-up run {} after failed direct capability run {}",
                                    next_run_id, run_id_for_task
                                );
                            }
                        }
                    }
                });
            } else {
                // LLM-based agent spawn (original logic)
                if let Err(e) = state
                    .spawn_agent_for_run(&session_id, &run_id, budget)
                    .await
                {
                    error!(
                        "[Scheduler] Failed to spawn agent for run {}: {}",
                        run_id, e
                    );
                }

                // 4. Push kickoff message to start agent
                let channel_id = session_id
                    .split(':')
                    .nth(1)
                    .unwrap_or("default")
                    .to_string();
                let kickoff_msg = format!("Scheduled run started ({}). Goal:\n{}", run_id, goal);
                if let Err(e) = state
                    .session_registry
                    .push_message_to_session(
                        &session_id,
                        channel_id,
                        kickoff_msg,
                        "system".to_string(),
                        Some(run_id.clone()),
                    )
                    .await
                {
                    error!(
                        "[Scheduler] Failed to push kickoff message for run {}: {}",
                        run_id, e
                    );
                }
            }
        }
    }

    /// Parse a cron expression and calculate the next run time
    pub fn calculate_next_run(schedule: &str) -> Option<DateTime<Utc>> {
        // Try to parse as a cron expression with optional seconds support
        match Cron::new(schedule).with_seconds_optional().parse() {
            Ok(cron) => {
                // Find the next occurrence after now
                let now = Utc::now();
                match cron.find_next_occurrence(&now, false) {
                    Ok(next) => Some(next),
                    Err(e) => {
                        warn!(
                            "[Scheduler] Failed to find next occurrence for cron '{}': {}",
                            schedule, e
                        );
                        None
                    }
                }
            }
            Err(e) => {
                warn!(
                    "[Scheduler] Failed to parse cron expression '{}': {}",
                    schedule, e
                );
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::registry::CapabilityRegistry;
    use crate::capability_marketplace::CapabilityMarketplace;
    use crate::causal_chain::CausalChain;
    use crate::chat::connector::{
        ChatConnector, ConnectionHandle, EnvelopeCallback, HealthStatus, OutboundRequest,
        SendResult,
    };
    use crate::chat::gateway::GatewayState;
    use crate::chat::quarantine::InMemoryQuarantineStore;
    use crate::chat::run::{new_shared_run_store, BudgetContext, Run, RunState};
    use crate::chat::spawner::LogOnlySpawner;
    use crate::chat::Scheduler as SchedulerType;
    use crate::chat::{new_shared_resource_store, InMemoryCheckpointStore, SessionRegistry};
    use crate::chat::{MessageEnvelope, RealTimeTrackingSink};
    use async_trait::async_trait;
    use rtfs::runtime::error::RuntimeResult;
    use rtfs::runtime::values::Value;
    use std::collections::{HashMap, VecDeque};
    use std::sync::{Arc, Mutex};
    use tokio::sync::RwLock;
    use tokio::time::{sleep, Duration};

    #[derive(Debug)]
    struct DummyConnector;

    #[async_trait]
    impl ChatConnector for DummyConnector {
        async fn connect(&self) -> RuntimeResult<ConnectionHandle> {
            Ok(ConnectionHandle {
                id: "dummy-conn".to_string(),
                bind_addr: "dummy".to_string(),
            })
        }

        async fn disconnect(&self, _handle: &ConnectionHandle) -> RuntimeResult<()> {
            Ok(())
        }

        async fn subscribe(
            &self,
            _handle: &ConnectionHandle,
            _callback: EnvelopeCallback,
        ) -> RuntimeResult<()> {
            Ok(())
        }

        async fn send(
            &self,
            _handle: &ConnectionHandle,
            _outbound: OutboundRequest,
        ) -> RuntimeResult<SendResult> {
            Ok(SendResult {
                success: true,
                message_id: Some("dummy-msg".to_string()),
                error: None,
            })
        }

        async fn health(&self, _handle: &ConnectionHandle) -> RuntimeResult<HealthStatus> {
            Ok(HealthStatus {
                ok: true,
                details: None,
            })
        }
    }

    async fn build_test_gateway_state() -> Arc<GatewayState> {
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));
        let run_store = new_shared_run_store();

        marketplace
            .register_local_capability(
                "test.cap.ok".to_string(),
                "Test Capability".to_string(),
                "Returns success".to_string(),
                Arc::new(|_inputs: &Value| {
                    Ok(Value::Map(HashMap::from([(
                        rtfs::ast::MapKey::String("ok".to_string()),
                        Value::Boolean(true),
                    )])))
                }),
            )
            .await
            .expect("register test capability");

        Arc::new(GatewayState {
            marketplace,
            quarantine: Arc::new(InMemoryQuarantineStore::new()),
            chain: Arc::new(Mutex::new(CausalChain::new().expect("chain"))),
            _approvals: None,
            connector: Arc::new(DummyConnector),
            connector_handle: ConnectionHandle {
                id: "dummy".to_string(),
                bind_addr: "dummy".to_string(),
            },
            inbox: Mutex::new(VecDeque::<MessageEnvelope>::new()),
            policy_pack_version: "test".to_string(),
            session_registry: SessionRegistry::new(),
            spawner: Arc::new(LogOnlySpawner::new()),
            run_store: run_store.clone(),
            resource_store: new_shared_resource_store(),
            scheduler: Arc::new(SchedulerType::new(run_store.clone())),
            checkpoint_store: Arc::new(InMemoryCheckpointStore::new()),
            internal_api_secret: "test-secret".to_string(),
            realtime_sink: Arc::new(RealTimeTrackingSink::new(16)),
            admin_tokens: vec![],
            approval_ui_url: "http://localhost:3000".to_string(),
            spawn_llm_profile: Arc::new(RwLock::new(None)),
            session_llm_profiles: Arc::new(RwLock::new(HashMap::new())),
            working_memory: Arc::new(Mutex::new(crate::working_memory::WorkingMemory::new(
                Box::new(crate::working_memory::InMemoryJsonlBackend::new(
                    None, None, None,
                )),
            ))),
        })
    }

    #[tokio::test]
    async fn direct_scheduled_run_completes_and_creates_next_recurring_run() {
        let state = build_test_gateway_state().await;
        let scheduler = Scheduler::new(state.run_store.clone());

        let session_id = "sched:test-session".to_string();
        let run_id = {
            let mut store = state.run_store.lock().expect("run store lock");
            let run = Run::new_scheduled(
                session_id.clone(),
                "compute next fibonacci".to_string(),
                "*/10 * * * * *".to_string(),
                Utc::now() - chrono::Duration::seconds(1),
                Some(BudgetContext::default()),
                Some("test.cap.ok".to_string()),
                Some(serde_json::json!({ "n": 1 })),
            );
            store.create_run(run)
        };

        scheduler.check_scheduled_runs(&state).await;

        // Direct capability path is async-spawned; allow a short convergence window.
        for _ in 0..20 {
            let converged = {
                let store = state.run_store.lock().expect("run store lock");
                let current = store.get_run(&run_id).expect("current run exists");
                let has_next = store
                    .get_runs_for_session(&session_id)
                    .iter()
                    .any(|r| r.id != run_id && matches!(r.state, RunState::Scheduled));
                matches!(current.state, RunState::Done) && has_next
            };
            if converged {
                break;
            }
            sleep(Duration::from_millis(25)).await;
        }

        let store = state.run_store.lock().expect("run store lock");
        let current = store.get_run(&run_id).expect("current run exists");
        assert!(matches!(current.state, RunState::Done));

        let runs = store.get_runs_for_session(&session_id);
        let next = runs
            .iter()
            .find(|r| r.id != run_id && matches!(r.state, RunState::Scheduled))
            .expect("next recurring run created");
        assert_eq!(next.schedule.as_deref(), Some("*/10 * * * * *"));
        assert_eq!(
            next.metadata
                .get("scheduler_run_number")
                .and_then(|v| v.as_u64()),
            Some(2)
        );
        assert_eq!(next.trigger_capability_id.as_deref(), Some("test.cap.ok"));
    }

    #[tokio::test]
    async fn test_compiled_scheduled_task_execution() {
        let state = build_test_gateway_state().await;
        let scheduler = Scheduler::new(state.run_store.clone());

        // 1. Create a scheduled run with a direct capability trigger
        let session_id = "test:test-session";
        let trigger_cap = "test.cap.ok";
        let trigger_inputs = serde_json::json!({
            "n": 1
        });

        let next_run = Utc::now() - chrono::Duration::seconds(1); // Due now
        let run_id = {
            let mut store = state.run_store.lock().unwrap();
            let run = crate::chat::run::Run::new_scheduled(
                session_id.to_string(),
                "test direct cap".to_string(),
                "*/1 * * * * *".to_string(),
                next_run,
                Some(BudgetContext::default()),
                Some(trigger_cap.to_string()),
                Some(trigger_inputs.clone()),
            );
            store.create_run(run)
        };

        // 2. Trigger the scheduler once
        scheduler.check_scheduled_runs(&state).await;

        // Wait a brief moment for the async direct capability execution to complete
        for _ in 0..20 {
            let done = {
                let store = state.run_store.lock().unwrap();
                let current = store.get_run(&run_id).expect("run exists");
                matches!(current.state, RunState::Done)
            };
            if done {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        // 3. Verify state transition to Active (or Done if it completed super fast)
        {
            let store = state.run_store.lock().unwrap();
            let run = store.get_run(&run_id).expect("run exists");
            assert!(matches!(run.state, RunState::Active) || matches!(run.state, RunState::Done));
        }
    }
}
