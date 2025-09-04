use std::sync::{Arc, Mutex};

use rtfs_compiler::ccos::arbiter::plan_generation::{PlanGenerationProvider, StubPlanGenerationProvider};
use rtfs_compiler::ccos::causal_chain::CausalChain;
use rtfs_compiler::ccos::intent_graph::core::IntentGraph;
use rtfs_compiler::ccos::types::IntentStatus;
use rtfs_compiler::ccos::orchestrator::{self, Orchestrator};
use rtfs_compiler::ccos::types::{Intent, StorableIntent};
use rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace;
use rtfs_compiler::runtime::security::RuntimeContext;
use rtfs_compiler::runtime::stdlib::register_default_capabilities;

#[tokio::test]
async fn stub_plan_generation_and_execution_works() {
    // Ledger + intent store
    let causal_chain = Arc::new(Mutex::new(CausalChain::new().unwrap()));
    let intent_graph = Arc::new(Mutex::new(IntentGraph::new().unwrap()));

    // Marketplace + stdlib
    let registry = Default::default();
    let marketplace = Arc::new(CapabilityMarketplace::with_causal_chain(registry, Some(causal_chain.clone())));
    register_default_capabilities(&marketplace).await.unwrap();

    // Seed an intent in the graph
    let storable_intent = StorableIntent::new("Demo".into());
    let intent_id = storable_intent.intent_id.clone();
    {
        let mut ig = intent_graph.lock().unwrap();
        ig.store_intent(storable_intent.clone()).unwrap();
        ig.set_intent_status(&intent_id, IntentStatus::Active).unwrap();
    }

    // Generate plan via stub provider
    let generator = StubPlanGenerationProvider;
    // Build a lightweight runtime Intent view only for generator API - here Stub doesn't use fields beyond id
    let runtime_intent = Intent {
        intent_id: intent_id.clone(),
        name: Some("Demo".into()),
        original_request: "Demo".into(),
        goal: "Demo".into(),
        constraints: Default::default(),
        preferences: Default::default(),
        success_criteria: None,
        status: IntentStatus::Active,
        created_at: 0,
        updated_at: 0,
        metadata: Default::default(),
    };
    let result = generator.generate_plan(&runtime_intent, marketplace.clone()).await.unwrap();
    // Verify IR JSON is present and shaped as expected
    if let Some(ir) = result.ir_json.clone() {
        let steps = ir.get("steps").and_then(|v| v.as_array()).expect("ir steps array");
        assert_eq!(steps.len(), 2, "expected 2 steps in IR");
        let caps: Vec<String> = steps.iter().map(|s| s.get("capability").and_then(|c| c.as_str()).unwrap_or("").to_string()).collect();
        assert!(caps.contains(&":ccos.echo".to_string()));
        assert!(caps.contains(&":ccos.math.add".to_string()));
    } else {
        panic!("stub provider should include IR JSON");
    }
    if let Some(diag) = result.diagnostics.clone() { assert!(diag.contains("ir-equivalence:"), "expected diagnostics to include ir-equivalence flag, got: {}", diag); }
    let plan = result.plan;

    // Execute plan
    let runtime_ctx = RuntimeContext::controlled(vec![
        "ccos.echo".to_string(),
        "ccos.math.add".to_string(),
    ]);
    
    // let orchestrator = Orchestrator::new(causal_chain.clone(), intent_graph.clone(), marketplace.clone());
    let orchestrator = orchestrator::Orchestrator::new(
        causal_chain.clone(),
        intent_graph.clone(),
        marketplace.clone(),
        Arc::new(rtfs_compiler::ccos::plan_archive::PlanArchive::new()),
    );
    let exec = orchestrator.execute_plan(&plan, &runtime_ctx).await.unwrap();

    // Assert intent transitioned to Completed
    {
        let ig = intent_graph.lock().unwrap();
        let intent_loaded = ig.get_intent(&intent_id).expect("intent exists");
        let st = intent_loaded.status;
        assert_eq!(st, IntentStatus::Completed);
    }

    // Assert causal chain recorded PlanStarted and CapabilityCall entries
    {
        let cc = causal_chain.lock().unwrap();
        let actions = cc.get_all_actions();
        let has_plan_started = actions.iter().any(|a| a.action_type == rtfs_compiler::ccos::types::ActionType::PlanStarted);
        let has_cap_call = actions.iter().any(|a| a.action_type == rtfs_compiler::ccos::types::ActionType::CapabilityCall);
        assert!(has_plan_started, "missing PlanStarted action");
        assert!(has_cap_call, "missing CapabilityCall action");
    }

    // Exec result should be ok
    assert!(exec.success, "execution failed: {:?}", exec.value);
}
