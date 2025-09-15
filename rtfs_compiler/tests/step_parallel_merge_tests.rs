use std::rc::Rc;
use std::sync::{Arc, Mutex};

use rtfs_compiler::parser;
use rtfs_compiler::runtime::{Evaluator, ModuleRegistry};
use rtfs_compiler::runtime::security::RuntimeContext;
use rtfs_compiler::ccos::host::RuntimeHost;
use rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace;
use rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry;
use tokio::sync::RwLock;
use rtfs_compiler::ccos::causal_chain::CausalChain;
use rtfs_compiler::runtime::delegation::StaticDelegationEngine;
use rtfs_compiler::runtime::values::Value;

#[test]
fn test_step_parallel_deep_merge_policy() {
    // Build evaluator
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let capability_marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let causal_chain = Arc::new(Mutex::new(CausalChain::new().expect("cc")));
    let host = Arc::new(RuntimeHost::new(causal_chain, capability_marketplace, RuntimeContext::pure()));
    let module_registry = Arc::new(ModuleRegistry::new());
    let de = Arc::new(StaticDelegationEngine::new_empty());
    let evaluator = Evaluator::new(module_registry, de, RuntimeContext::pure(), host.clone());

    // Initialize root context and seed complex map
    {
        let mut cm = evaluator.context_manager.borrow_mut();
        cm.initialize(Some("root".to_string()));
        cm.set("data".to_string(), Value::Map([
            (rtfs_compiler::ast::MapKey::String("a".into()), Value::Map(vec![].into_iter().collect())),
            (rtfs_compiler::ast::MapKey::String("list".into()), Value::Vector(vec![Value::Integer(1)])),
        ].into_iter().collect())).unwrap();
    }

    // Simulate two branches that both update nested maps and vectors, with :merge policy
    // Note: integration via context manager since (set-context ...) is not a real form here
    {
        use rtfs_compiler::ccos::execution_context::{ConflictResolution};
        let mut cm = evaluator.context_manager.borrow_mut();
        let b1 = cm.create_parallel_context(Some("b1".to_string())).unwrap();
        cm.switch_to(&b1).unwrap();
        cm.set("data".to_string(), Value::Map([
            (rtfs_compiler::ast::MapKey::String("a".into()), Value::Map([
                (rtfs_compiler::ast::MapKey::String("x".into()), Value::Integer(10))
            ].into_iter().collect())),
            (rtfs_compiler::ast::MapKey::String("list".into()), Value::Vector(vec![Value::Integer(2)])),
        ].into_iter().collect())).unwrap();
        cm.merge_child_to_parent(&b1, ConflictResolution::Merge).unwrap();
        cm.switch_to("root").unwrap();

        let b2 = cm.create_parallel_context(Some("b2".to_string())).unwrap();
        cm.switch_to(&b2).unwrap();
        cm.set("data".to_string(), Value::Map([
            (rtfs_compiler::ast::MapKey::String("a".into()), Value::Map([
                (rtfs_compiler::ast::MapKey::String("y".into()), Value::Integer(20))
            ].into_iter().collect())),
            (rtfs_compiler::ast::MapKey::String("list".into()), Value::Vector(vec![Value::Integer(3)])),
        ].into_iter().collect())).unwrap();
        cm.merge_child_to_parent(&b2, ConflictResolution::Merge).unwrap();
        cm.switch_to("root").unwrap();
    }

    // Validate deep-merged result: nested maps merged and vectors concatenated
    let merged = {
        let cm = evaluator.context_manager.borrow();
        cm.get("data").unwrap()
    };

    match merged {
        Value::Map(m) => {
            // expect data.a has both x and y
            if let Some(Value::Map(a)) = m.get(&rtfs_compiler::ast::MapKey::String("a".into())) {
                assert!(a.get(&rtfs_compiler::ast::MapKey::String("x".into())).is_some());
                assert!(a.get(&rtfs_compiler::ast::MapKey::String("y".into())).is_some());
            } else { panic!("missing nested map a"); }

            if let Some(Value::Vector(v)) = m.get(&rtfs_compiler::ast::MapKey::String("list".into())) {
                // initial [1], then branch adds [2], then [3] => at least length 3
                assert!(v.len() >= 3);
            } else { panic!("missing vector list"); }
        }
        other => panic!("unexpected merged type: {:?}", other),
    }
}


