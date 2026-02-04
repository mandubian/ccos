//! Run Lifecycle State Machine
//!
//! Manages autonomous run lifecycle with states, budgets, and persistence.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// State of a run
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
    /// Run failed with error
    Failed { error: String },
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
            RunState::PausedApproval { .. } | RunState::PausedExternalEvent { .. }
        )
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
}

impl Default for BudgetContext {
    fn default() -> Self {
        Self {
            max_steps: 50,
            max_duration_secs: 300, // 5 minutes
            max_tokens: None,
            max_retries_per_step: 3,
        }
    }
}

/// Current budget consumption
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BudgetConsumption {
    pub steps_taken: u32,
    pub tokens_consumed: u64,
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
        None
    }
}

/// Reason for budget being exceeded
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BudgetExceeded {
    MaxSteps { limit: u32, current: u32 },
    MaxDuration { limit_secs: u64, elapsed_secs: u64 },
    MaxTokens { limit: u64, current: u64 },
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
    pub completion_predicate: Option<String>,
    pub created_at: DateTime<Utc>,
    /// Budget window start time (for duration/step budgets). This can be reset on resume.
    pub budget_started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Correlation ID for tracing
    pub correlation_id: String,
    /// Current step ID
    pub current_step_id: Option<String>,
    /// Metadata for extensibility
    pub metadata: HashMap<String, serde_json::Value>,
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
            metadata: HashMap::new(),
        }
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
}

/// In-memory run store (for MVP; should be persisted to causal chain in production)
#[derive(Debug, Default)]
pub struct RunStore {
    runs: HashMap<String, Run>,
    runs_by_session: HashMap<String, Vec<String>>,
}

impl RunStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create_run(&mut self, run: Run) -> String {
        let run_id = run.id.clone();
        let session_id = run.session_id.clone();

        self.runs_by_session
            .entry(session_id)
            .or_default()
            .push(run_id.clone());
        self.runs.insert(run_id.clone(), run);

        run_id
    }

    pub fn get_run(&self, run_id: &str) -> Option<&Run> {
        self.runs.get(run_id)
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

    pub fn get_latest_paused_external_run_id_for_session(&self, session_id: &str) -> Option<String> {
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

    pub fn get_latest_paused_approval_run_id_for_session(&self, session_id: &str) -> Option<String> {
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

    pub fn update_run_state(&mut self, run_id: &str, new_state: RunState) -> bool {
        if let Some(run) = self.runs.get_mut(run_id) {
            run.transition(new_state);
            true
        } else {
            false
        }
    }

    pub fn cancel_run(&mut self, run_id: &str) -> bool {
        self.update_run_state(run_id, RunState::Cancelled)
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
}
