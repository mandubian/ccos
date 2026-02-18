//! Run Lifecycle State Machine
//!
//! Manages autonomous run lifecycle with states, budgets, and persistence.

use chrono::{DateTime, Utc};
use log;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RunState {
    /// Run is actively executing
    Active,
    /// Run completed successfully
    Done,
    /// Run paused waiting for human approval
    PausedApproval { reason: String },
    /// Run paused waiting for external event
    PausedExternalEvent { event_type: String },
    /// Run paused and checkpointed (can be resumed later)
    PausedCheckpoint {
        reason: String,
        checkpoint_id: String,
    },
    /// Run failed with error
    Failed { error: String },
    /// Run is scheduled to run in the future
    Scheduled,
    /// Run was cancelled
    Cancelled,
}

impl RunState {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            RunState::Done | RunState::Failed { .. } | RunState::Cancelled
        )
    }

    pub fn is_paused(&self) -> bool {
        matches!(
            self,
            RunState::PausedApproval { .. }
                | RunState::PausedExternalEvent { .. }
                | RunState::PausedCheckpoint { .. }
        )
    }

    pub fn is_busy(&self) -> bool {
        matches!(self, RunState::Active) || self.is_paused()
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "Active" => Some(RunState::Active),
            "Done" => Some(RunState::Done),
            "Scheduled" => Some(RunState::Scheduled),
            "Cancelled" => Some(RunState::Cancelled),
            s if s.starts_with("PausedApproval") => {
                // Simplified: assuming format "PausedApproval { reason: \"...\" }" or similar
                // For now, let's just use a default reason if we can't parse it perfectly
                Some(RunState::PausedApproval {
                    reason: "Restored from chain".to_string(),
                })
            }
            s if s.starts_with("PausedExternalEvent") => Some(RunState::PausedExternalEvent {
                event_type: "Restored from chain".to_string(),
            }),
            s if s.starts_with("PausedCheckpoint") => Some(RunState::PausedCheckpoint {
                reason: "Restored from chain".to_string(),
                checkpoint_id: "unknown".to_string(),
            }),
            s if s.starts_with("Failed") => Some(RunState::Failed {
                error: "Restored from chain".to_string(),
            }),
            _ => None,
        }
    }
}

/// Budget limits for a run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetContext {
    /// Maximum number of steps allowed
    pub max_steps: u32,
    /// Maximum wall-clock duration in seconds
    pub max_duration_secs: u64,
    /// Maximum tokens consumed (if LLM-based)
    pub max_tokens: Option<u64>,
    /// Maximum retries per step
    pub max_retries_per_step: u32,
    /// Maximum network egress bytes
    pub max_network_egress_bytes: Option<u64>,
    /// Maximum network ingress bytes
    pub max_network_ingress_bytes: Option<u64>,
}

impl Default for BudgetContext {
    fn default() -> Self {
        Self {
            max_steps: 50,
            max_duration_secs: 300, // 5 minutes
            max_tokens: None,
            max_retries_per_step: 3,
            max_network_egress_bytes: None,
            max_network_ingress_bytes: None,
        }
    }
}

/// Current budget consumption
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BudgetConsumption {
    pub steps_taken: u32,
    pub tokens_consumed: u64,
    pub network_egress_bytes: u64,
    pub network_ingress_bytes: u64,
    pub retries_by_step: HashMap<String, u32>,
}

impl BudgetConsumption {
    pub fn check_budget(
        &self,
        budget: &BudgetContext,
        elapsed_secs: u64,
    ) -> Option<BudgetExceeded> {
        if self.steps_taken >= budget.max_steps {
            return Some(BudgetExceeded::MaxSteps {
                limit: budget.max_steps,
                current: self.steps_taken,
            });
        }
        if elapsed_secs >= budget.max_duration_secs {
            return Some(BudgetExceeded::MaxDuration {
                limit_secs: budget.max_duration_secs,
                elapsed_secs,
            });
        }
        if let Some(max_tokens) = budget.max_tokens {
            if self.tokens_consumed >= max_tokens {
                return Some(BudgetExceeded::MaxTokens {
                    limit: max_tokens,
                    current: self.tokens_consumed,
                });
            }
        }
        if let Some(max_egress) = budget.max_network_egress_bytes {
            if self.network_egress_bytes >= max_egress {
                return Some(BudgetExceeded::MaxNetworkEgress {
                    limit: max_egress,
                    current: self.network_egress_bytes,
                });
            }
        }
        if let Some(max_ingress) = budget.max_network_ingress_bytes {
            if self.network_ingress_bytes >= max_ingress {
                return Some(BudgetExceeded::MaxNetworkIngress {
                    limit: max_ingress,
                    current: self.network_ingress_bytes,
                });
            }
        }
        None
    }
}

/// Reason for budget being exceeded
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BudgetExceeded {
    MaxSteps { limit: u32, current: u32 },
    MaxDuration { limit_secs: u64, elapsed_secs: u64 },
    MaxTokens { limit: u64, current: u64 },
    MaxNetworkEgress { limit: u64, current: u64 },
    MaxNetworkIngress { limit: u64, current: u64 },
}

/// A run representing an autonomous goal execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub session_id: String,
    pub goal: String,
    pub state: RunState,
    pub budget: BudgetContext,
    pub consumption: BudgetConsumption,
    /// Optional predicate to determine completion
    pub completion_predicate: Option<crate::chat::Predicate>,
    pub created_at: DateTime<Utc>,
    /// Budget window start time (for duration/step budgets). This can be reset on resume.
    pub budget_started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Correlation ID for tracing
    pub correlation_id: String,
    /// Current step ID
    pub current_step_id: Option<String>,
    /// Cron-like schedule (if any)
    pub schedule: Option<String>,
    /// Next scheduled execution time
    pub next_run_at: Option<DateTime<Utc>>,
    /// ID of the latest checkpoint
    pub latest_checkpoint_id: Option<String>,
    /// Metadata for extensibility
    pub metadata: HashMap<String, serde_json::Value>,
    /// Optional capability ID to trigger instead of goal-based LLM kickoff
    pub trigger_capability_id: Option<String>,
    /// Optional inputs for the trigger capability
    pub trigger_inputs: Option<serde_json::Value>,
}

impl Run {
    pub fn new(session_id: String, goal: String, budget: Option<BudgetContext>) -> Self {
        let id = format!("run-{}", uuid::Uuid::new_v4());
        let now = Utc::now();
        Self {
            id: id.clone(),
            session_id,
            goal,
            state: RunState::Active,
            budget: budget.unwrap_or_default(),
            consumption: BudgetConsumption::default(),
            completion_predicate: None,
            created_at: now,
            budget_started_at: now,
            updated_at: now,
            correlation_id: format!("corr-{}", uuid::Uuid::new_v4()),
            current_step_id: None,
            schedule: None,
            next_run_at: None,
            latest_checkpoint_id: None,
            metadata: HashMap::new(),
            trigger_capability_id: None,
            trigger_inputs: None,
        }
    }

    pub fn new_scheduled(
        session_id: String,
        goal: String,
        schedule: String,
        next_run_at: DateTime<Utc>,
        budget: Option<BudgetContext>,
        trigger_capability_id: Option<String>,
        trigger_inputs: Option<serde_json::Value>,
    ) -> Self {
        let mut run = Self::new(session_id, goal, budget);
        run.state = RunState::Scheduled;
        run.schedule = Some(schedule);
        run.next_run_at = Some(next_run_at);
        run.trigger_capability_id = trigger_capability_id;
        run.trigger_inputs = trigger_inputs;
        // Stable group ID — links all recurrences of the same scheduled task together
        run.metadata.insert(
            "schedule_group_id".to_string(),
            serde_json::Value::String(format!("sched-{}", uuid::Uuid::new_v4())),
        );
        run
    }

    pub fn new_ephemeral(session_id: String, goal: String, budget: Option<BudgetContext>) -> Self {
        let mut run = Self::new(session_id, goal, budget);
        run.metadata
            .insert("ephemeral".to_string(), serde_json::json!("true"));
        run
    }

    pub fn transition(&mut self, new_state: RunState) {
        self.state = new_state;
        self.updated_at = Utc::now();
    }

    pub fn increment_step(&mut self) -> String {
        self.consumption.steps_taken += 1;
        let step_id = format!("{}-step-{}", self.id, self.consumption.steps_taken);
        self.current_step_id = Some(step_id.clone());
        self.updated_at = Utc::now();
        step_id
    }

    pub fn add_tokens(&mut self, tokens: u64) {
        self.consumption.tokens_consumed += tokens;
        self.updated_at = Utc::now();
    }

    pub fn elapsed_secs(&self) -> u64 {
        let elapsed = Utc::now().signed_duration_since(self.created_at);
        elapsed.num_seconds().max(0) as u64
    }

    pub fn budget_elapsed_secs(&self) -> u64 {
        let elapsed = Utc::now().signed_duration_since(self.budget_started_at);
        elapsed.num_seconds().max(0) as u64
    }

    pub fn check_budget(&self) -> Option<BudgetExceeded> {
        self.consumption
            .check_budget(&self.budget, self.budget_elapsed_secs())
    }

    pub fn reset_budget_window(&mut self) {
        self.consumption = BudgetConsumption::default();
        self.current_step_id = None;
        self.budget_started_at = Utc::now();
        self.updated_at = Utc::now();
    }

    pub fn checkpoint(&mut self, reason: String) -> String {
        let checkpoint_id = format!("chk-{}", uuid::Uuid::new_v4());
        self.transition(RunState::PausedCheckpoint {
            reason,
            checkpoint_id: checkpoint_id.clone(),
        });
        checkpoint_id
    }

    pub fn resume(&mut self) {
        if self.state.is_paused() {
            self.transition(RunState::Active);
            // Give a fresh duration window on resume
            self.budget_started_at = Utc::now();
            self.updated_at = self.budget_started_at;
        }
    }

    pub fn from_action(action: &crate::types::Action) -> Option<Self> {
        let meta = &action.metadata;
        let run_id = meta.get("run_id").and_then(|v| v.as_string())?;
        let session_id = meta.get("session_id").and_then(|v| v.as_string())?;
        let goal = meta
            .get("goal")
            .and_then(|v| v.as_string())
            .unwrap_or_default();
        let correlation_id = meta
            .get("correlation_id")
            .and_then(|v| v.as_string())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("corr-{}", uuid::Uuid::new_v4()));

        let budget = BudgetContext {
            max_steps: meta
                .get("budget_max_steps")
                .and_then(|v| v.as_integer())
                .unwrap_or(100) as u32,
            max_duration_secs: meta
                .get("budget_max_duration_secs")
                .and_then(|v| v.as_integer())
                .unwrap_or(3600) as u64,
            max_tokens: meta
                .get("budget_max_tokens")
                .and_then(|v| v.as_integer())
                .map(|v| v as u64),
            max_retries_per_step: 3,
            max_network_egress_bytes: meta
                .get("budget_max_network_egress_bytes")
                .and_then(|v| v.as_integer())
                .map(|v| v as u64),
            max_network_ingress_bytes: meta
                .get("budget_max_network_ingress_bytes")
                .and_then(|v| v.as_integer())
                .map(|v| v as u64),
        };

        let created_at =
            DateTime::from_timestamp(action.timestamp as i64 / 1000, 0).unwrap_or_else(Utc::now);

        let mut metadata = HashMap::new();
        if let Some(max_runs) = meta
            .get("max_run")
            .or_else(|| meta.get("scheduler_max_runs"))
            .and_then(|v| v.as_integer())
        {
            metadata.insert(
                "scheduler_max_runs".to_string(),
                serde_json::Value::from(max_runs),
            );
        }
        if let Some(run_number) = meta
            .get("run_number")
            .or_else(|| meta.get("scheduler_run_number"))
            .and_then(|v| v.as_integer())
        {
            metadata.insert(
                "scheduler_run_number".to_string(),
                serde_json::Value::from(run_number),
            );
        }

        Some(Self {
            id: run_id.to_string(),
            session_id: session_id.to_string(),
            goal: goal.to_string(),
            state: RunState::Active, // Default to Active on creation
            budget,
            consumption: BudgetConsumption::default(),
            completion_predicate: meta.get("completion_predicate").and_then(|v| {
                let json = crate::utils::value_conversion::rtfs_value_to_json(v).ok()?;
                serde_json::from_value(json).ok()
            }),
            created_at,
            budget_started_at: created_at,
            updated_at: created_at,
            correlation_id: correlation_id.to_string(),
            current_step_id: None,
            schedule: meta
                .get("schedule")
                .and_then(|v| v.as_string())
                .map(|s| s.to_string()),
            next_run_at: meta
                .get("next_run_at")
                .and_then(|v| v.as_string())
                .and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .ok()
                        .map(|dt| dt.with_timezone(&Utc))
                }),
            latest_checkpoint_id: meta
                .get("latest_checkpoint_id")
                .and_then(|v| v.as_string())
                .map(|s| s.to_string()),
            metadata,
            trigger_capability_id: meta
                .get("trigger_capability_id")
                .and_then(|v| v.as_string())
                .map(|s| s.to_string()),
            trigger_inputs: meta
                .get("trigger_inputs")
                .and_then(|v| crate::utils::value_conversion::rtfs_value_to_json(v).ok()),
        })
    }
}

/// Serialization snapshot for RunStore persistence
#[derive(Debug, Serialize, Deserialize)]
struct RunStoreSnapshot {
    runs: HashMap<String, Run>,
    runs_by_session: HashMap<String, Vec<String>>,
}

/// In-memory run store with optional JSON persistence
#[derive(Debug, Default)]
pub struct RunStore {
    runs: HashMap<String, Run>,
    runs_by_session: HashMap<String, Vec<String>>,
    /// Optional path for JSON persistence across restarts
    persist_path: Option<PathBuf>,
}

impl RunStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the persist path and immediately save the current state.
    pub fn set_persist_path(&mut self, path: PathBuf) {
        self.persist_path = Some(path);
        self.save_to_disk();
    }

    /// Persist current run state to JSON file (no-op if persist_path is None).
    pub fn save_to_disk(&self) {
        let path = match &self.persist_path {
            Some(p) => p,
            None => return,
        };
        let snapshot = RunStoreSnapshot {
            runs: self.runs.clone(),
            runs_by_session: self.runs_by_session.clone(),
        };
        match serde_json::to_string_pretty(&snapshot) {
            Ok(json) => {
                if let Err(e) = std::fs::write(path, &json) {
                    log::warn!("[RunStore] Failed to persist runs to {}: {}", path.display(), e);
                }
            }
            Err(e) => log::warn!("[RunStore] Failed to serialize runs: {}", e),
        }
    }

    /// Load run store from JSON file. Returns None if file doesn't exist or fails to parse.
    pub fn load_from_disk(path: &std::path::Path) -> Option<RunStore> {
        let data = std::fs::read_to_string(path).ok()?;
        let snapshot: RunStoreSnapshot = serde_json::from_str(&data).ok()?;
        log::info!(
            "[RunStore] Loaded {} runs from {}",
            snapshot.runs.len(),
            path.display()
        );
        Some(RunStore {
            runs: snapshot.runs,
            runs_by_session: snapshot.runs_by_session,
            persist_path: None, // caller sets this after loading
        })
    }

    pub fn create_run(&mut self, run: Run) -> String {
        let run_id = run.id.clone();
        let session_id = run.session_id.clone();

        self.runs_by_session
            .entry(session_id)
            .or_default()
            .push(run_id.clone());
        self.runs.insert(run_id.clone(), run);

        self.save_to_disk();
        run_id
    }

    pub fn get_run(&self, run_id: &str) -> Option<&Run> {
        self.runs.get(run_id)
    }

    pub fn get_all_runs(&self) -> Vec<&Run> {
        self.runs.values().collect()
    }

    pub fn get_run_mut(&mut self, run_id: &str) -> Option<&mut Run> {
        self.runs.get_mut(run_id)
    }

    pub fn get_runs_for_session(&self, session_id: &str) -> Vec<&Run> {
        self.runs_by_session
            .get(session_id)
            .map(|ids| ids.iter().filter_map(|id| self.runs.get(id)).collect())
            .unwrap_or_default()
    }

    pub fn get_active_run_for_session(&self, session_id: &str) -> Option<&Run> {
        self.get_runs_for_session(session_id)
            .into_iter()
            .find(|r| matches!(r.state, RunState::Active))
    }

    pub fn get_busy_run_for_session(&self, session_id: &str) -> Option<&Run> {
        self.get_runs_for_session(session_id)
            .into_iter()
            .find(|r| r.state.is_busy())
    }

    pub fn get_latest_paused_external_run_id_for_session(
        &self,
        session_id: &str,
    ) -> Option<String> {
        let ids = self.runs_by_session.get(session_id)?;
        for id in ids.iter().rev() {
            if let Some(run) = self.runs.get(id) {
                if matches!(run.state, RunState::PausedExternalEvent { .. }) {
                    return Some(id.clone());
                }
            }
        }
        None
    }

    pub fn get_latest_paused_approval_run_id_for_session(
        &self,
        session_id: &str,
    ) -> Option<String> {
        let ids = self.runs_by_session.get(session_id)?;
        for id in ids.iter().rev() {
            if let Some(run) = self.runs.get(id) {
                if matches!(run.state, RunState::PausedApproval { .. }) {
                    return Some(id.clone());
                }
            }
        }
        None
    }

    pub fn get_latest_checkpoint_run_id_for_session(&self, session_id: &str) -> Option<String> {
        let ids = self.runs_by_session.get(session_id)?;
        for id in ids.iter().rev() {
            if let Some(run) = self.runs.get(id) {
                if matches!(run.state, RunState::PausedCheckpoint { .. }) {
                    return Some(id.clone());
                }
            }
        }
        None
    }

    /// Find a scheduled run that matches the request for deduplication.
    /// Returns the run_id if a matching scheduled run exists.
    pub fn find_similar_scheduled_run(
        &self,
        session_id: &str,
        goal: &str,
        schedule: Option<&str>,
        next_run_at: Option<DateTime<Utc>>,
        trigger_capability_id: Option<&str>,
        trigger_inputs: Option<&serde_json::Value>,
    ) -> Option<String> {
        let normalized_goal = Self::normalize_goal(goal);
        let normalized_schedule = schedule.unwrap_or("one-off");
        let ids = self.runs_by_session.get(session_id)?;
        for id in ids.iter().rev() {
            if let Some(run) = self.runs.get(id) {
                if matches!(run.state, RunState::Scheduled) {
                    let normalized_existing = Self::normalize_goal(&run.goal);
                    if normalized_goal != normalized_existing {
                        continue;
                    }

                    let existing_schedule = run.schedule.as_deref().unwrap_or("one-off");
                    if existing_schedule != normalized_schedule {
                        continue;
                    }

                    // For one-off runs, dedup only if the exact scheduled instant also matches.
                    if normalized_schedule == "one-off" && run.next_run_at != next_run_at {
                        continue;
                    }

                    if run.trigger_capability_id.as_deref() != trigger_capability_id {
                        continue;
                    }

                    if run.trigger_inputs.as_ref() != trigger_inputs {
                        continue;
                    }

                    return Some(id.clone());
                }
            }
        }
        None
    }

    /// Normalize a goal string for comparison (lowercase, trim, collapse whitespace)
    fn normalize_goal(goal: &str) -> String {
        goal.to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn update_run_state(&mut self, run_id: &str, new_state: RunState) -> bool {
        if let Some(run) = self.runs.get_mut(run_id) {
            let old_state = run.state.clone();
            match (&run.state, &new_state) {
                // Special case: resuming a paused run resets the budget window
                (_, RunState::Active) if run.state.is_paused() => run.resume(),
                // Scheduled -> Active: direct transition (no budget reset needed)
                (RunState::Scheduled, RunState::Active) => run.transition(RunState::Active),
                // All other transitions
                _ => run.transition(new_state),
            }
            log::info!(
                "[RunStore] Transitioned run {} from {:?} to {:?}",
                run_id,
                old_state,
                run.state
            );
            self.save_to_disk();
            true
        } else {
            false
        }
    }

    pub fn cancel_run(&mut self, run_id: &str) -> bool {
        self.update_run_state(run_id, RunState::Cancelled)
    }

    /// Complete a run and create the next recurring run if applicable.
    /// Returns the ID of the newly created run if this was a recurring task.
    pub fn complete_run_and_schedule_next(
        &mut self,
        run_id: &str,
        calculate_next: impl Fn(&str) -> Option<chrono::DateTime<chrono::Utc>>,
    ) -> Option<String> {
        // Get the run's schedule before transitioning to Done
        let (session_id, goal, schedule, budget, metadata) = {
            let run = self.runs.get(run_id)?;
            (
                run.session_id.clone(),
                run.goal.clone(),
                run.schedule.clone(),
                run.budget.clone(),
                run.metadata.clone(),
            )
        };

        let max_runs = metadata
            .get("scheduler_max_runs")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        let current_run_number = metadata
            .get("scheduler_run_number")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .unwrap_or(1);

        // Transition to Done
        self.update_run_state(run_id, RunState::Done);

        // Check if this is a recurring task
        if let Some(ref sched) = schedule {
            if sched != "one-off" {
                if let Some(max_runs) = max_runs {
                    if current_run_number >= max_runs {
                        log::info!(
                            "[RunStore] Reached max runs ({}) for recurring schedule '{}'; not creating next run",
                            max_runs,
                            sched
                        );
                        return None;
                    }
                }

                if let Some(next_time) = calculate_next(sched) {
                    log::info!(
                        "[RunStore] Creating next recurring run for schedule '{}' at {}",
                        sched,
                        next_time.to_rfc3339()
                    );
                    let mut new_run = Run::new_scheduled(
                        session_id,
                        goal,
                        sched.clone(),
                        next_time,
                        Some(budget),
                        None,
                        None,
                    );
                    new_run.metadata = metadata;
                    new_run.metadata.insert(
                        "scheduler_run_number".to_string(),
                        serde_json::Value::from((current_run_number + 1) as u64),
                    );
                    if let Some(max_runs) = max_runs {
                        new_run.metadata.insert(
                            "scheduler_max_runs".to_string(),
                            serde_json::Value::from(max_runs as u64),
                        );
                    }
                    let new_run_id = new_run.id.clone();
                    new_run.trigger_capability_id = self
                        .runs
                        .get(run_id)
                        .and_then(|r| r.trigger_capability_id.clone());
                    new_run.trigger_inputs =
                        self.runs.get(run_id).and_then(|r| r.trigger_inputs.clone());
                    self.create_run(new_run);
                    return Some(new_run_id);
                }
            }
        }

        None
    }

    pub fn rebuild_from_chain(actions: &[crate::types::Action]) -> Self {
        let mut store = Self::new();
        log::info!(
            "[RunStore] Rebuilding run state from causal chain ({} actions)",
            actions.len()
        );

        // Group actions by run_id to process them chronologically per run
        let mut runs_map: HashMap<String, Vec<&crate::types::Action>> = HashMap::new();

        for action in actions {
            let func = match &action.function_name {
                Some(f) => f,
                None => continue,
            };

            if func.starts_with("chat.audit.run.") {
                if let Some(run_id) = action.metadata.get("run_id").and_then(|v| v.as_string()) {
                    runs_map
                        .entry(run_id.to_string())
                        .or_insert_with(Vec::new)
                        .push(action);
                }
            }
        }

        // Process each run's events
        for (run_id, run_events) in runs_map {
            for action in run_events {
                let func = action.function_name.as_deref().unwrap_or("");
                match func {
                    "chat.audit.run.create" => {
                        if let Some(run) = Run::from_action(action) {
                            log::debug!("[RunStore] Replayed run.create: {}", run.id);
                            store.create_run(run);
                        }
                    }
                    "chat.audit.run.transition" => {
                        let new_state_str =
                            action.metadata.get("new_state").and_then(|v| v.as_string());

                        if let Some(state_str) = new_state_str {
                            if let Some(state) = RunState::parse(state_str) {
                                log::debug!(
                                    "[RunStore] Replayed run.transition: {} -> {:?}",
                                    run_id,
                                    state
                                );
                                store.update_run_state(&run_id, state);
                            }
                        }
                    }
                    "chat.audit.run.checkpoint" => {
                        let reason = action
                            .metadata
                            .get("reason")
                            .and_then(|v| v.as_string())
                            .unwrap_or("Checkpoint");
                        let checkpoint_id = action
                            .metadata
                            .get("checkpoint_id")
                            .and_then(|v| v.as_string())
                            .unwrap_or("unknown");

                        log::debug!("[RunStore] Replayed run.checkpoint: {}", run_id);
                        store.update_run_state(
                            &run_id,
                            RunState::PausedCheckpoint {
                                reason: reason.to_string(),
                                checkpoint_id: checkpoint_id.to_string(),
                            },
                        );
                    }
                    "chat.audit.run.cancel" => {
                        log::debug!("[RunStore] Replayed run.cancel: {}", run_id);
                        store.cancel_run(&run_id);
                    }
                    _ => {}
                }
            }
        }
        store
    }
}

/// Thread-safe run store wrapper
pub type SharedRunStore = Arc<Mutex<RunStore>>;

pub fn new_shared_run_store() -> SharedRunStore {
    Arc::new(Mutex::new(RunStore::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtfs::runtime::values::Value;

    #[test]
    fn test_run_lifecycle() {
        let mut run = Run::new("session-1".to_string(), "test goal".to_string(), None);
        assert!(matches!(run.state, RunState::Active));

        run.transition(RunState::PausedApproval {
            reason: "need approval".to_string(),
        });
        assert!(run.state.is_paused());

        run.transition(RunState::Done);
        assert!(run.state.is_terminal());
    }

    #[test]
    fn test_budget_enforcement() {
        let mut run = Run::new(
            "session-1".to_string(),
            "test goal".to_string(),
            Some(BudgetContext {
                max_steps: 5,
                max_duration_secs: 60,
                max_tokens: Some(1000),
                max_retries_per_step: 3,
                max_network_egress_bytes: None,
                max_network_ingress_bytes: None,
            }),
        );

        for _ in 0..4 {
            run.increment_step();
        }
        assert!(run.check_budget().is_none());

        run.increment_step();
        assert!(matches!(
            run.check_budget(),
            Some(BudgetExceeded::MaxSteps { .. })
        ));
    }

    #[test]
    fn test_reset_budget_window() {
        let mut run = Run::new(
            "session-1".to_string(),
            "test goal".to_string(),
            Some(BudgetContext {
                max_steps: 2,
                max_duration_secs: 60,
                max_tokens: None,
                max_retries_per_step: 3,
                max_network_egress_bytes: None,
                max_network_ingress_bytes: None,
            }),
        );

        run.increment_step();
        run.increment_step();
        assert!(matches!(
            run.check_budget(),
            Some(BudgetExceeded::MaxSteps { .. })
        ));

        run.reset_budget_window();
        assert!(run.check_budget().is_none());
        assert_eq!(run.consumption.steps_taken, 0);
    }

    #[test]
    fn test_run_store() {
        let mut store = RunStore::new();

        let run = Run::new("session-1".to_string(), "goal 1".to_string(), None);
        let run_id = store.create_run(run);

        assert!(store.get_run(&run_id).is_some());
        assert!(store.get_active_run_for_session("session-1").is_some());

        store.cancel_run(&run_id);
        assert!(store.get_active_run_for_session("session-1").is_none());
    }

    #[test]
    fn test_scheduled_dedup_matches_schedule_and_trigger() {
        let mut store = RunStore::new();
        let session_id = "session-1".to_string();
        let next_at = Utc::now();
        let run = Run::new_scheduled(
            session_id.clone(),
            "Compute next fibonacci".to_string(),
            "*/10 * * * * *".to_string(),
            next_at,
            None,
            Some("ccos.execute.python".to_string()),
            Some(serde_json::json!({"code":"print('ok')"})),
        );
        let run_id = store.create_run(run);

        let found = store.find_similar_scheduled_run(
            &session_id,
            "compute   NEXT fibonacci",
            Some("*/10 * * * * *"),
            None,
            Some("ccos.execute.python"),
            Some(&serde_json::json!({"code":"print('ok')"})),
        );
        assert_eq!(found, Some(run_id));

        let not_found = store.find_similar_scheduled_run(
            &session_id,
            "compute next fibonacci",
            Some("*/5 * * * * *"),
            None,
            Some("ccos.execute.python"),
            Some(&serde_json::json!({"code":"print('ok')"})),
        );
        assert!(not_found.is_none());
    }

    #[test]
    fn test_scheduled_dedup_one_off_requires_same_time() {
        let mut store = RunStore::new();
        let session_id = "session-1".to_string();
        let next_at = Utc::now();
        let run = Run::new_scheduled(
            session_id.clone(),
            "do once".to_string(),
            "one-off".to_string(),
            next_at,
            None,
            None,
            None,
        );
        let _ = store.create_run(run);

        let found_same_time = store.find_similar_scheduled_run(
            &session_id,
            "do once",
            None,
            Some(next_at),
            None,
            None,
        );
        assert!(found_same_time.is_some());

        let found_other_time = store.find_similar_scheduled_run(
            &session_id,
            "do once",
            None,
            Some(next_at + chrono::Duration::seconds(5)),
            None,
            None,
        );
        assert!(found_other_time.is_none());
    }

    #[test]
    fn test_rebuild_from_chain() {
        use crate::types::{Action, ActionType};

        let mut actions = Vec::new();

        // 1. Create run
        let mut meta1 = HashMap::new();
        meta1.insert("run_id".to_string(), Value::String("run-1".to_string()));
        meta1.insert(
            "session_id".to_string(),
            Value::String("session-1".to_string()),
        );
        meta1.insert("goal".to_string(), Value::String("test goal".to_string()));

        actions.push(Action {
            action_id: "a1".to_string(),
            parent_action_id: None,
            session_id: Some("session-1".to_string()),
            plan_id: "p1".to_string(),
            intent_id: "i1".to_string(),
            action_type: ActionType::InternalStep,
            function_name: Some("chat.audit.run.create".to_string()),
            arguments: None,
            result: None,
            cost: None,
            duration_ms: None,
            timestamp: 1000000,
            metadata: meta1,
        });

        // 2. Transition
        let mut meta2 = HashMap::new();
        meta2.insert("run_id".to_string(), Value::String("run-1".to_string()));
        meta2.insert(
            "new_state".to_string(),
            Value::String("PausedApproval".to_string()),
        );

        actions.push(Action {
            action_id: "a2".to_string(),
            parent_action_id: None,
            session_id: Some("session-1".to_string()),
            plan_id: "p1".to_string(),
            intent_id: "i1".to_string(),
            action_type: ActionType::InternalStep,
            function_name: Some("chat.audit.run.transition".to_string()),
            arguments: None,
            result: None,
            cost: None,
            duration_ms: None,
            timestamp: 2000000,
            metadata: meta2,
        });

        let store = RunStore::rebuild_from_chain(&actions);
        let run = store.get_run("run-1").expect("Run should be rebuilt");
        assert_eq!(run.session_id, "session-1");
        assert_eq!(run.goal, "test goal");
        assert!(matches!(run.state, RunState::PausedApproval { .. }));
    }

    #[test]
    fn test_checkpoint_and_resume() {
        let mut run = Run::new("session-1".to_string(), "test goal".to_string(), None);
        assert_eq!(run.state, RunState::Active);

        run.checkpoint("milestone 1".to_string());
        assert!(matches!(
            run.state,
            RunState::PausedCheckpoint {
                ref reason,
                ..
            } if reason == "milestone 1"
        ));

        run.resume();
        assert_eq!(run.state, RunState::Active);
    }

    #[test]
    fn test_rebuild_from_checkpoint() {
        let mut actions = Vec::new();
        let run_id = "run-1".to_string();
        let session_id = "session-1".to_string();
        let now = Utc::now().timestamp_millis() as u64;

        // 1. Create
        let mut meta1 = HashMap::new();
        meta1.insert("run_id".to_string(), Value::String(run_id.clone()));
        meta1.insert("session_id".to_string(), Value::String(session_id.clone()));
        meta1.insert("goal".to_string(), Value::String("test goal".to_string()));
        actions.push(crate::types::Action {
            action_id: "a1".to_string(),
            parent_action_id: None,
            plan_id: "p1".to_string(),
            intent_id: "i1".to_string(),
            action_type: crate::types::ActionType::CapabilityCall,
            function_name: Some("chat.audit.run.create".to_string()),
            timestamp: now,
            duration_ms: None,
            arguments: None,
            result: None,
            cost: None,
            session_id: Some(session_id.clone()),
            metadata: meta1,
        });

        // 2. Checkpoint
        let mut meta2 = HashMap::new();
        meta2.insert("run_id".to_string(), Value::String(run_id.clone()));
        meta2.insert("reason".to_string(), Value::String("milestone".to_string()));
        meta2.insert(
            "checkpoint_id".to_string(),
            Value::String("chk-1".to_string()),
        );
        actions.push(crate::types::Action {
            action_id: "a2".to_string(),
            parent_action_id: None,
            plan_id: "p1".to_string(),
            intent_id: "i1".to_string(),
            action_type: crate::types::ActionType::CapabilityCall,
            function_name: Some("chat.audit.run.checkpoint".to_string()),
            timestamp: now + 1000,
            duration_ms: None,
            arguments: None,
            result: None,
            cost: None,
            session_id: Some(session_id.clone()),
            metadata: meta2,
        });

        let store = RunStore::rebuild_from_chain(&actions);
        let run = store.get_run(&run_id).expect("Run should be rebuilt");
        assert!(matches!(
            run.state,
            RunState::PausedCheckpoint {
                ref reason,
                ..
            } if reason == "milestone"
        ));
    }
}
