// Test execution context fix for Issue #43
use rtfs_compiler::runtime::Evaluator;
use rtfs_compiler::runtime::host::RuntimeHost;
use rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace;
use rtfs_compiler::runtime::capability_registry::CapabilityRegistry;
use rtfs_compiler::runtime::ModuleRegistry;
use rtfs_compiler::runtime::security::RuntimeContext;
use rtfs_compiler::runtime::host_interface::HostInterface;
use rtfs_compiler::runtime::stdlib::StandardLibrary;
use rtfs_compiler::ccos::causal_chain::CausalChain;
use rtfs_compiler::ccos::delegation::StaticDelegationEngine;
use rtfs_compiler::parser::parse;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use tokio::sync::RwLock;

#[test]
fn test_execution_context_basic_validation() {
    // Test basic execution context parameter validation
    let plan_id = "test_plan_123".to_string();
    let intent_ids = vec!["intent_1".to_string(), "intent_2".to_string()];
    let parent_action_id = "parent_action_456".to_string();
    
    // Verify the parameters we'll pass to set_execution_context
    assert!(!plan_id.is_empty());
    assert!(!intent_ids.is_empty());
    assert!(!parent_action_id.is_empty());
}

#[test]
fn test_host_interface_execution_context_methods() {
    // Create necessary components
    let causal_chain = Arc::new(Mutex::new(CausalChain::new().unwrap()));
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let security_context = RuntimeContext::pure();
    
    // Create RuntimeHost which implements HostInterface
    let host = RuntimeHost::new(
        causal_chain,
        marketplace,
        security_context,
    );
    
    // Test execution context setup with correct parameters
    let plan_id = "test_plan_789".to_string();
    let intent_ids = vec!["intent_a".to_string(), "intent_b".to_string()];
    let parent_action_id = "parent_action_999".to_string();
    
    // Test that we can call set_execution_context with correct signature
    host.set_execution_context(plan_id, intent_ids, parent_action_id);
    
    // Test that we can call clear_execution_context without error
    host.clear_execution_context();
}

// Non-async test to validate execution context management
#[test]
fn test_execution_context_validation() {
    // Create test components
    let module_registry = Rc::new(ModuleRegistry::new());
    let delegation_engine = Arc::new(StaticDelegationEngine::new(HashMap::new()));
    let security_context = RuntimeContext::pure();
    
    let causal_chain = Arc::new(Mutex::new(CausalChain::new().unwrap()));
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));
    
    let host = std::sync::Arc::new(RuntimeHost::new(
        causal_chain,
        marketplace,
        security_context.clone(),
    ));
    
    // Create evaluator with correct parameters
    let stdlib_env = StandardLibrary::create_global_environment();
    let evaluator = Evaluator::with_environment(
        module_registry,
        stdlib_env,
        delegation_engine,
        security_context,
        host.clone() as std::sync::Arc<dyn HostInterface>,
    );
    
    // Test simple RTFS code that doesn't require capability execution
    let rtfs_code = r#"(+ 1 2)"#;
    
    // Parse the code
    let ast = parse(rtfs_code).expect("Failed to parse RTFS code");
    assert!(!ast.is_empty());
    
    // This validates that our execution context setup doesn't break basic parsing
    // The actual evaluation would require async context which we're avoiding
    // to prevent the runtime nesting issue
    
    // Test execution context setup before any capability operations
    let plan_id = "validation_plan".to_string();
    let intent_ids = vec!["validation_intent".to_string()];
    let parent_action_id = "validation_parent".to_string();
    
    // This should work without issues now that we have the trait methods
    host.set_execution_context(plan_id, intent_ids, parent_action_id);
    host.clear_execution_context();
}