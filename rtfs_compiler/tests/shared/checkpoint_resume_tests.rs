use std::sync::{Arc, Mutex};

use rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry;
use rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace;
use rtfs_compiler::ccos::causal_chain::CausalChain;
use rtfs_compiler::ccos::delegation::StaticDelegationEngine;
use rtfs_compiler::ccos::host::RuntimeHost;
use rtfs_compiler::runtime::security::RuntimeContext;
use rtfs_compiler::runtime::values::Value;
use rtfs_compiler::runtime::{Evaluator, ModuleRegistry};
use tokio::sync::RwLock;

#[test]
fn test_checkpoint_store_and_resume_from_disk() {
    // Build orchestrator-like setup
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let capability_marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let causal_chain = Arc::new(Mutex::new(CausalChain::new().expect("cc")));

    let host = Arc::new(RuntimeHost::new(
        causal_chain,
        capability_marketplace,
        RuntimeContext::pure(),
    ));
    let module_registry = Arc::new(ModuleRegistry::new());
    let de = Arc::new(StaticDelegationEngine::new(std::collections::HashMap::new()));
    let evaluator = Evaluator::new(module_registry, RuntimeContext::pure(), host.clone());

    // Initialize context and set a value
    {
        let mut cm = evaluator.context_manager.borrow_mut();
        cm.initialize(Some("root".to_string()));
        cm.set("k".to_string(), Value::Integer(7)).unwrap();
    }

    // Use in-memory archive with durable dir
    let tmpdir = tempfile::tempdir().expect("tmpdir");
    let archive = rtfs_compiler::ccos::checkpoint_archive::CheckpointArchive::new()
        .with_durable_dir(tmpdir.path());

    // Create record and store
    let rec = rtfs_compiler::ccos::checkpoint_archive::CheckpointRecord {
        checkpoint_id: "cp-test-1".to_string(),
        plan_id: "plan1".to_string(),
        intent_id: "i1".to_string(),
        serialized_context: evaluator.context_manager.borrow().serialize().unwrap(),
        created_at: 0,
        metadata: std::collections::HashMap::new(),
    };
    let _ = archive.store(rec).expect("store");

    // Reload from disk and restore into a fresh evaluator
    let rec2 = archive.load_from_disk("cp-test-1").expect("load");
    let registry2 = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let capability_marketplace2 = Arc::new(CapabilityMarketplace::new(registry2));
    let causal_chain2 = Arc::new(Mutex::new(CausalChain::new().expect("cc2")));
    let host2 = Arc::new(RuntimeHost::new(
        causal_chain2,
        capability_marketplace2,
        RuntimeContext::pure(),
    ));
    let module_registry2 = Arc::new(ModuleRegistry::new());
    let de2 = Arc::new(StaticDelegationEngine::new(std::collections::HashMap::new()));
    let evaluator2 = Evaluator::new(module_registry2, RuntimeContext::pure(), host2);

    {
        let mut cm = evaluator2.context_manager.borrow_mut();
        cm.initialize(Some("root2".to_string()));
        cm.deserialize(&rec2.serialized_context).unwrap();
    }
    let val = evaluator2.context_manager.borrow().get("k");
    assert_eq!(val, Some(Value::Integer(7)));
}
