use std::sync::{Arc, Mutex};

use rtfs_compiler::ccos::causal_chain::CausalChain;
use rtfs_compiler::ccos::orchestrator::Orchestrator;
use rtfs_compiler::ccos::types::{IntentStatus, Plan, StorableIntent};
use rtfs_compiler::runtime::security::RuntimeContext;

// Helper to set env so host fallback context (if needed) won't panic in tests
fn ensure_test_env() {
    std::env::set_var("CCOS_TEST_FALLBACK_CONTEXT", "1");
}

#[test]
fn test_status_transition_success() {
    ensure_test_env();
    let causal_chain = Arc::new(Mutex::new(CausalChain::new().unwrap()));
    // Create an IntentGraph that writes status-change events into our CausalChain
    let sink =
        rtfs_compiler::ccos::event_sink::CausalChainIntentEventSink::new(Arc::clone(&causal_chain));
    let mut intent_graph =
        rtfs_compiler::ccos::intent_graph::IntentGraph::with_event_sink(Arc::new(sink)).unwrap();
    let mut intent = StorableIntent::new("Simple addition goal".to_string());
    let intent_id = intent.intent_id.clone();
    intent.status = IntentStatus::Active; // initial
    intent_graph.store_intent(intent).unwrap();
    let intent_graph = Arc::new(Mutex::new(intent_graph));
    let capability_marketplace =
        rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace::new(Arc::new(
            tokio::sync::RwLock::new(
                rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry::new(),
            ),
        ));
    let plan_archive = Arc::new(rtfs_compiler::ccos::plan_archive::PlanArchive::new());
    let orchestrator = Orchestrator::new(
        causal_chain.clone(),
        intent_graph.clone(),
        Arc::new(capability_marketplace),
        plan_archive,
    );

    let plan = Plan::new_rtfs("(+ 1 2)".to_string(), vec![intent_id.clone()]);
    let ctx = RuntimeContext::pure();
    let result =
        futures::executor::block_on(async { orchestrator.execute_plan(&plan, &ctx).await })
            .unwrap();
    assert!(result.success);

    // Verify final status
    let graph_locked = intent_graph.lock().unwrap();
    let stored = graph_locked.get_intent(&intent_id).unwrap();
    assert_eq!(stored.status, IntentStatus::Completed);

    // Verify causal chain contains an IntentStatusChanged for this intent
    let chain_locked = causal_chain.lock().unwrap();
    let actions = chain_locked.get_actions_for_intent(&intent_id);
    let mut found = false;
    for a in actions {
        if a.action_type == rtfs_compiler::ccos::types::ActionType::IntentStatusChanged {
            found = true;
            break;
        }
    }
    assert!(
        found,
        "Expected an IntentStatusChanged action in causal chain for success case"
    );
}

#[test]
fn test_status_transition_failure() {
    ensure_test_env();
    let causal_chain = Arc::new(Mutex::new(CausalChain::new().unwrap()));
    // Create an IntentGraph that writes status-change events into our CausalChain
    let sink =
        rtfs_compiler::ccos::event_sink::CausalChainIntentEventSink::new(Arc::clone(&causal_chain));
    let mut intent_graph =
        rtfs_compiler::ccos::intent_graph::IntentGraph::with_event_sink(Arc::new(sink)).unwrap();
    let mut intent = StorableIntent::new("Failing goal".to_string());
    let intent_id = intent.intent_id.clone();
    intent.status = IntentStatus::Active;
    intent_graph.store_intent(intent).unwrap();
    let intent_graph = Arc::new(Mutex::new(intent_graph));
    let capability_marketplace =
        rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace::new(Arc::new(
            tokio::sync::RwLock::new(
                rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry::new(),
            ),
        ));
    let plan_archive = Arc::new(rtfs_compiler::ccos::plan_archive::PlanArchive::new());
    let orchestrator = Orchestrator::new(
        causal_chain.clone(),
        intent_graph.clone(),
        Arc::new(capability_marketplace),
        plan_archive,
    );
    // Minimal plan and evaluator; use invalid RTFS to force a parse/runtime error
    let plan = Plan::new_rtfs("(this is not valid".to_string(), vec![intent_id.clone()]);
    let ctx = RuntimeContext::pure();
    let result =
        futures::executor::block_on(async { orchestrator.execute_plan(&plan, &ctx).await });
    assert!(result.is_err(), "Plan should fail");

    let graph_locked = intent_graph.lock().unwrap();
    let stored = graph_locked.get_intent(&intent_id).unwrap();
    assert_eq!(stored.status, IntentStatus::Failed);

    // Verify causal chain contains an IntentStatusChanged for this intent
    let chain_locked = causal_chain.lock().unwrap();
    let actions = chain_locked.get_actions_for_intent(&intent_id);
    let mut found = false;
    for a in actions {
        if a.action_type == rtfs_compiler::ccos::types::ActionType::IntentStatusChanged {
            found = true;
            break;
        }
    }
    assert!(
        found,
        "Expected an IntentStatusChanged action in causal chain for failure case"
    );
}
