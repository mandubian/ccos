use chrono::{Duration, Utc};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration as StdDuration;
use tempfile::tempdir;
use tokio::time::sleep;

use ccos::capabilities::registry::CapabilityRegistry;
use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::causal_chain::CausalChain;
use ccos::chat::gateway::GatewayState;
use ccos::chat::run::{BudgetContext, RunState, RunStore, SharedRunStore};
use ccos::chat::scheduler::Scheduler;
use ccos::chat::session::SessionRegistry;
use rtfs::runtime::values::Value;
use tokio::sync::RwLock;

#[tokio::test]
async fn test_compiled_scheduled_task_execution() {
    let run_store = Arc::new(Mutex::new(RunStore::new()));
    let session_registry = Arc::new(SessionRegistry::new());
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry.clone()));
    let causal_chain = Arc::new(Mutex::new(CausalChain::new().expect("causal chain")));

    // Create session
    session_registry
        .create_session("test:test-session", "test-user")
        .await
        .expect("session");

    let gateway_state = Arc::new(GatewayState::new(
        run_store.clone(),
        session_registry.clone(),
        marketplace.clone(),
        causal_chain.clone(),
        Arc::new(RwLock::new(None)), // session_pool
        ccos::chat::new_shared_resource_store(),
        None, // approval_queue
        ccos::config::types::ChatGatewayConfig::default(),
        ccos::config::types::SandboxConfig::default(),
        ccos::config::types::CodingAgentsConfig::default(),
    ));

    let scheduler = Scheduler::new(run_store.clone());

    // 1. Create a scheduled run with a direct capability trigger
    let session_id = "test:test-session";
    let trigger_cap = "ccos.memory.store";
    let trigger_inputs = serde_json::json!({
        "key": "test_key",
        "value": "test_value"
    });

    let next_run = Utc::now() - Duration::seconds(1); // Due now
    let run_id = {
        let mut store = run_store.lock().unwrap();
        let run = ccos::chat::run::Run::new_scheduled(
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
    scheduler.check_scheduled_runs(&gateway_state).await;

    // 3. Verify state transition to Active
    {
        let store = run_store.lock().unwrap();
        let run = store.get_run(&run_id).expect("run exists");
        assert_eq!(run.state, RunState::Active);
    }
}
