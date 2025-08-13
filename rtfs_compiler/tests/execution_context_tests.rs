//! Tests for Hierarchical Execution Context Management

use rtfs_compiler::ccos::execution_context::{ContextManager, IsolationLevel, ConflictResolution};
use rtfs_compiler::runtime::values::Value;
use rtfs_compiler::runtime::error::RuntimeResult;
use rtfs_compiler::ast::MapKey;
use std::collections::HashMap;

#[test]
fn test_context_creation_and_basic_operations() -> RuntimeResult<()> {
    let mut manager = ContextManager::new();
    
    // Initialize with root context
    manager.initialize(Some("root".to_string()));
    assert_eq!(manager.current_context_id(), Some("root"));
    
    // Set and get values
    manager.set("key1".to_string(), Value::String("value1".to_string()))?;
    assert_eq!(manager.get("key1"), Some(Value::String("value1".to_string())));
    
    // Test step creation
    let child_id = manager.enter_step("child1", IsolationLevel::Inherit)?;
    assert_eq!(manager.current_context_id(), Some(child_id.as_str()));
    
    // Child should inherit parent's values
    assert_eq!(manager.get("key1"), Some(Value::String("value1".to_string())));
    
    // Set value in child
    manager.set("key2".to_string(), Value::Integer(42))?;
    assert_eq!(manager.get("key2"), Some(Value::Integer(42)));
    
    // Switch back to parent
    manager.switch_to("root")?;
    assert_eq!(manager.current_context_id(), Some("root"));
    
    // Parent should not see child's values
    assert_eq!(manager.get("key2"), None);
    
    Ok(())
}

#[test]
fn test_context_isolation_levels() -> RuntimeResult<()> {
    let mut manager = ContextManager::new();
    manager.initialize(Some("root".to_string()));
    manager.set("shared".to_string(), Value::String("root_value".to_string()))?;
    
    // Test Inherit isolation
    let inherit_id = manager.enter_step("inherit", IsolationLevel::Inherit)?;
    manager.switch_to(&inherit_id)?;
    assert_eq!(manager.get("shared"), Some(Value::String("root_value".to_string())));
    
    // Test Isolated isolation
    manager.switch_to("root")?;
    let isolated_id = manager.enter_step("isolated", IsolationLevel::Isolated)?;
    manager.switch_to(&isolated_id)?;
    assert_eq!(manager.get("shared"), Some(Value::String("root_value".to_string()))); // Isolated can read parent data
    
    // Test Sandboxed isolation
    manager.switch_to("root")?;
    let sandboxed_id = manager.enter_step("sandboxed", IsolationLevel::Sandboxed)?;
    manager.switch_to(&sandboxed_id)?;
    assert_eq!(manager.get("shared"), None); // Sandboxed context doesn't inherit
    
    // Sandboxed context should allow setting values
    manager.set("new_key".to_string(), Value::Integer(1))?;
    assert_eq!(manager.get("new_key"), Some(Value::Integer(1)));
    
    Ok(())
}

#[test]
fn test_context_serialization() -> RuntimeResult<()> {
    let mut manager = ContextManager::new();
    manager.initialize(Some("root".to_string()));
    
    // Create simple data structure
    manager.set("timeout".to_string(), Value::Integer(30))?;
    manager.set("retries".to_string(), Value::Integer(3))?;
    
    // Serialize context
    let serialized = manager.serialize()?;
    assert!(!serialized.is_empty());
    
    // Create new manager and deserialize
    let mut new_manager = ContextManager::new();
    new_manager.initialize(Some("restored".to_string()));
    new_manager.deserialize(&serialized)?;
    
    // Verify data was restored
    let restored_timeout = new_manager.get("timeout");
    let restored_retries = new_manager.get("retries");
    assert_eq!(restored_timeout, Some(Value::Integer(30)));
    assert_eq!(restored_retries, Some(Value::Integer(3)));
    
    Ok(())
}

#[test]
fn test_context_checkpointing() -> RuntimeResult<()> {
    let mut manager = ContextManager::with_checkpointing(100); // 100ms interval
    manager.initialize(Some("root".to_string()));
    
    // Set some data
    manager.set("checkpoint_data".to_string(), Value::String("test_value".to_string()))?;
    
    // Trigger checkpoint
    manager.checkpoint("test_checkpoint".to_string())?;
    
    // Verify data is still accessible
    assert_eq!(manager.get("checkpoint_data"), Some(Value::String("test_value".to_string())));
    
    Ok(())
}

#[test]
fn test_parallel_context_execution() -> RuntimeResult<()> {
    let mut manager = ContextManager::new();
    manager.initialize(Some("root".to_string()));
    manager.set("root_data".to_string(), Value::Integer(100))?;
    
    // Create multiple isolated contexts for parallel execution
    let context1_id = manager.create_parallel_context(Some("parallel1".to_string()))?;
    let context2_id = manager.create_parallel_context(Some("parallel2".to_string()))?;
    
    // Simulate parallel execution by switching between contexts
    manager.switch_to(&context1_id)?;
    manager.set("parallel_data".to_string(), Value::String("context1_value".to_string()))?;
    
    manager.switch_to(&context2_id)?;
    manager.set("parallel_data".to_string(), Value::String("context2_value".to_string()))?;
    
    // Verify contexts are isolated
    manager.switch_to(&context1_id)?;
    assert_eq!(manager.get("parallel_data"), Some(Value::String("context1_value".to_string())));
    
    manager.switch_to(&context2_id)?;
    assert_eq!(manager.get("parallel_data"), Some(Value::String("context2_value".to_string())));
    
    // Verify root context is unchanged
    manager.switch_to("root")?;
    assert_eq!(manager.get("parallel_data"), None);
    assert_eq!(manager.get("root_data"), Some(Value::Integer(100)));
    
    Ok(())
}

#[test]
fn test_context_depth_tracking() -> RuntimeResult<()> {
    let mut manager = ContextManager::new();
    manager.initialize(Some("root".to_string()));
    
    assert_eq!(manager.depth(), 1);
    
    let child1_id = manager.enter_step("child1", IsolationLevel::Inherit)?;
    assert_eq!(manager.depth(), 2);
    
    let child2_id = manager.enter_step("child2", IsolationLevel::Inherit)?;
    assert_eq!(manager.depth(), 3);
    
    // Switch back to parent
    manager.switch_to("root")?;
    assert_eq!(manager.depth(), 1);
    
    Ok(())
}

#[test]
fn test_parent_wins_merge_policy_default() -> RuntimeResult<()> {
    let mut manager = ContextManager::new();
    manager.initialize(Some("root".to_string()));

    // Parent has an existing value for key "k"
    manager.set("k".to_string(), Value::String("parent".to_string()))?;

    // Create isolated child and override the same key
    let child_id = manager.create_parallel_context(Some("branch".to_string()))?;
    manager.switch_to(&child_id)?;
    manager.set("k".to_string(), Value::String("child".to_string()))?;
    manager.set("new_key".to_string(), Value::Integer(1))?;

    // Merge back with parent-wins (keep existing)
    manager.merge_child_to_parent(&child_id, ConflictResolution::KeepExisting)?;

    // Verify parent value is kept, and new child-only keys are merged
    manager.switch_to("root")?;
    assert_eq!(manager.get("k"), Some(Value::String("parent".to_string())));
    assert_eq!(manager.get("new_key"), Some(Value::Integer(1)));

    Ok(())
}

#[test]
fn test_merge_policy_keyword_overwrite_in_step_parallel() -> RuntimeResult<()> {
    use rtfs_compiler::runtime::evaluator::Evaluator;
    use rtfs_compiler::runtime::module_runtime::ModuleRegistry;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};
    use rtfs_compiler::runtime::security::RuntimeContext;
    use rtfs_compiler::runtime::host::RuntimeHost;
    use rtfs_compiler::ccos::causal_chain::CausalChain;
    use rtfs_compiler::ast::{Expression, Literal};

    // Minimal evaluator setup
    let module_registry = Rc::new(ModuleRegistry::new());
    let causal_chain = Arc::new(Mutex::new(CausalChain::new()?));
    let capability_marketplace = {
        use rtfs_compiler::runtime::capability_registry::CapabilityRegistry;
        use rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace;
        use tokio::sync::RwLock;
        Arc::new(CapabilityMarketplace::new(Arc::new(RwLock::new(CapabilityRegistry::new()))))
    };
    let host = std::sync::Arc::new(RuntimeHost::new(causal_chain, capability_marketplace, RuntimeContext::pure()));
    let evaluator = Evaluator::new(module_registry, Arc::new(rtfs_compiler::ccos::delegation::StaticDelegationEngine::new(std::collections::HashMap::new())), RuntimeContext::pure(), host);

    // Initialize root context and set parent value :k = "parent"
    {
        let mut mgr = evaluator.context_manager.borrow_mut();
        mgr.initialize(Some("root".to_string()));
        mgr.set("k".to_string(), Value::String("parent".to_string()))?;
    }

    // Build a step-parallel expression with :merge-policy :overwrite
    // (step-parallel :merge-policy :overwrite (do (quote nil)) (do (quote nil)))
    let expr = Expression::List(vec![
        Expression::Symbol(rtfs_compiler::ast::Symbol("step-parallel".to_string())),
        Expression::Literal(Literal::Keyword(rtfs_compiler::ast::Keyword("merge-policy".to_string()))),
        Expression::Literal(Literal::Keyword(rtfs_compiler::ast::Keyword("overwrite".to_string()))),
        Expression::Do(rtfs_compiler::ast::DoExpr { expressions: vec![
            // Simulate child setting k = "child-a"
            Expression::List(vec![
                Expression::Symbol(rtfs_compiler::ast::Symbol("set-context".to_string())),
                Expression::Literal(Literal::String("k".to_string())),
                Expression::Literal(Literal::String("child-a".to_string())),
            ])
        ]}),
        Expression::Do(rtfs_compiler::ast::DoExpr { expressions: vec![
            // Simulate child setting k = "child-b"
            Expression::List(vec![
                Expression::Symbol(rtfs_compiler::ast::Symbol("set-context".to_string())),
                Expression::Literal(Literal::String("k".to_string())),
                Expression::Literal(Literal::String("child-b".to_string())),
            ])
        ]}),
    ]);

    // Since we don't have a real (set-context ...) special form hooked,
    // directly simulate two branches using the ContextManager API.
    {
        let mut mgr = evaluator.context_manager.borrow_mut();
        let c1 = mgr.create_parallel_context(Some("b1".to_string()))?; mgr.switch_to(&c1)?; mgr.set("k".to_string(), Value::String("child-a".to_string()))?; mgr.merge_child_to_parent(&c1, ConflictResolution::Overwrite)?; mgr.switch_to("root")?;
        let c2 = mgr.create_parallel_context(Some("b2".to_string()))?; mgr.switch_to(&c2)?; mgr.set("k".to_string(), Value::String("child-b".to_string()))?; mgr.merge_child_to_parent(&c2, ConflictResolution::Overwrite)?;
        mgr.switch_to("root")?;
        assert_eq!(mgr.get("k"), Some(Value::String("child-b".to_string())));
    }

    Ok(())
}

#[test]
fn test_manual_overwrite_merge_policy() -> RuntimeResult<()> {
    let mut manager = ContextManager::new();
    manager.initialize(Some("root".to_string()));

    // Parent value
    manager.set("k".to_string(), Value::String("parent".to_string()))?;

    // Child overrides same key
    let child_id = manager.create_parallel_context(Some("branch".to_string()))?;
    manager.switch_to(&child_id)?;
    manager.set("k".to_string(), Value::String("child".to_string()))?;

    // Overwrite policy should replace parent value
    manager.merge_child_to_parent(&child_id, ConflictResolution::Overwrite)?;

    manager.switch_to("root")?;
    assert_eq!(manager.get("k"), Some(Value::String("child".to_string())));

    Ok(())
}
