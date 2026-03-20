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
