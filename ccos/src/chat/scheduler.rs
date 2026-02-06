use chrono::{DateTime, Utc};
use croner::Cron;
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tracing::{error, info, warn};

use crate::chat::gateway::GatewayState;
use crate::chat::run::{Run, RunState, SharedRunStore};

pub struct Scheduler {
    run_store: SharedRunStore,
}

impl Scheduler {
    pub fn new(run_store: SharedRunStore) -> Self {
        Self { run_store }
    }

    pub async fn start(self: Arc<Self>, state: Arc<GatewayState>) {
        info!("[Scheduler] Starting background scheduler loop...");
        let mut tick = interval(Duration::from_secs(10));

        loop {
            tick.tick().await;
            self.check_scheduled_runs(&state).await;
        }
    }

    /// Check for scheduled runs that are due to be triggered
    async fn check_scheduled_runs(&self, state: &Arc<GatewayState>) {
        let mut runs_to_kickoff = Vec::new();

        {
            let store = self.run_store.lock().unwrap();
            let now = Utc::now();

            for run in store.get_all_runs() {
                if matches!(run.state, RunState::Scheduled) {
                    if let Some(next_run) = run.next_run_at {
                        if next_run <= now {
                            // SKIP if session is already busy (Active or Paused)
                            if store.get_busy_run_for_session(&run.session_id).is_some() {
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
                            ));
                        }
                    }
                }
            }
        }

        for (session_id, run_id, goal, budget, schedule) in runs_to_kickoff {
            info!(
                "[Scheduler] Triggering scheduled run {} for session {}",
                run_id, session_id
            );

            // 1. Transition to Active
            {
                let mut store = self.run_store.lock().unwrap();
                store.update_run_state(&run_id, RunState::Active);
            }

            // 2. If this is a recurring schedule, create a NEW scheduled run for next iteration
            //    and mark the current run as Done (fire-and-forget model for recurring tasks)
            let is_recurring = schedule.as_ref().map(|s| s != "one-off").unwrap_or(false);

            if is_recurring {
                if let Some(ref sched) = schedule {
                    if let Some(next_time) = Self::calculate_next_run(sched) {
                        info!(
                            "[Scheduler] Creating next recurring run for schedule '{}' at {}",
                            sched,
                            next_time.to_rfc3339()
                        );
                        let new_run = Run::new_scheduled(
                            session_id.clone(),
                            goal.clone(),
                            sched.clone(),
                            next_time,
                            budget.clone(),
                        );
                        let mut store = self.run_store.lock().unwrap();
                        store.create_run(new_run);
                        // Mark the current run as Done since it's a fire-and-forget iteration
                        store.update_run_state(&run_id, RunState::Done);
                    }
                }
            }

            // 3. Spawn agent if needed
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
                .push_message_to_session(&session_id, channel_id, kickoff_msg, "system".to_string())
                .await
            {
                error!(
                    "[Scheduler] Failed to push kickoff message for run {}: {}",
                    run_id, e
                );
            }
        }
    }

    /// Parse a cron expression and calculate the next run time
    fn calculate_next_run(schedule: &str) -> Option<DateTime<Utc>> {
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
