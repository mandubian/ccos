use rtfs_compiler::ccos::causal_chain::CausalChain;
use rtfs_compiler::ccos::intent_graph::core::IntentGraph;
use rtfs_compiler::ccos::orchestrator::Orchestrator;
use rtfs_compiler::ccos::types::Plan;
use rtfs_compiler::runtime::security::RuntimeContext;
use rtfs_compiler::runtime::module_runtime::ModuleRegistry;
use std::sync::{Arc, Mutex};

#[test]
fn test_checkpoint_and_resume_helpers() {
    let causal_chain = Arc::new(Mutex::new(CausalChain::new().unwrap()));
    let intent_graph = Arc::new(Mutex::new(IntentGraph::new().unwrap()));
    let capability_marketplace = rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace::new(
        Arc::new(tokio::sync::RwLock::new(rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry::new()))
    );
    let plan_archive = Arc::new(rtfs_compiler::ccos::plan_archive::PlanArchive::new());
    let orchestrator = Orchestrator::new(
        causal_chain.clone(),
        intent_graph.clone(),
        Arc::new(capability_marketplace),
        plan_archive,
    );
    // Minimal plan and evaluator
    let plan = Plan::new_rtfs("(+ 1 1)".to_string(), vec!["intent-1".to_string()]);
    let runtime_context = RuntimeContext::pure();
    let host = std::sync::Arc::new(rtfs_compiler::ccos::host::RuntimeHost::new(
        causal_chain.clone(),
        Arc::new(rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace::new(
            Arc::new(tokio::sync::RwLock::new(rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry::new()))
        )),
        runtime_context.clone(),
    ));
    let module_registry = std::sync::Arc::new(ModuleRegistry::new());
    let evaluator = rtfs_compiler::runtime::evaluator::Evaluator::new(
        module_registry,
        runtime_context,
        host,
    );

    // Initialize context for checkpoint
    {
        let mut mgr = evaluator.context_manager.borrow_mut();
        mgr.initialize(Some("root".to_string()));
        mgr.set("x".to_string(), rtfs_compiler::runtime::values::Value::Integer(1)).unwrap();
    }

    let (checkpoint_id, serialized) = orchestrator
        .checkpoint_plan(&plan.plan_id, &plan.intent_ids[0], &evaluator)
        .expect("checkpoint should succeed");
    assert!(checkpoint_id.starts_with("cp-"));
    assert!(!serialized.is_empty());

    // Mutate context and then restore via resume
    {
        let mut mgr = evaluator.context_manager.borrow_mut();
        mgr.set("x".to_string(), rtfs_compiler::runtime::values::Value::Integer(99)).unwrap();
    }
    orchestrator
        .resume_plan(&plan.plan_id, &plan.intent_ids[0], &evaluator, &serialized)
        .expect("resume should succeed");

    // Also resume via checkpoint id persisted in archive
    orchestrator
        .resume_plan_from_checkpoint(&plan.plan_id, &plan.intent_ids[0], &evaluator, &checkpoint_id)
        .expect("resume by id should succeed");

    // After resume, context should be restored from serialized snapshot
    let val = evaluator
        .context_manager
        .borrow()
        .get("x")
        .expect("value should exist");
    assert_eq!(val, rtfs_compiler::runtime::values::Value::Integer(1));
}


