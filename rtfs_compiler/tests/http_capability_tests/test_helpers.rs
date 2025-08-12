//! Test helper functions for creating standardized runtime components
//! 
//! This module provides reusable functions for initializing capability registry,
//! marketplace, runtime host, and evaluators with consistent patterns across tests.

use rtfs_compiler::ccos::delegation::StaticDelegationEngine;
use rtfs_compiler::ccos::causal_chain::CausalChain;
use rtfs_compiler::runtime::{Evaluator, ModuleRegistry};
use rtfs_compiler::runtime::stdlib::StandardLibrary;
use rtfs_compiler::runtime::security::RuntimeContext;
use rtfs_compiler::runtime::host::RuntimeHost;
use rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace;
use rtfs_compiler::runtime::capability_registry::CapabilityRegistry;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::cell::RefCell;
use tokio::sync::RwLock;

/// Creates a new capability registry wrapped in Arc<RwLock<>>
pub fn create_capability_registry() -> Arc<RwLock<CapabilityRegistry>> {
    Arc::new(RwLock::new(CapabilityRegistry::new()))
}

/// Creates a new capability marketplace with a fresh registry
pub fn create_capability_marketplace() -> CapabilityMarketplace {
    let registry = create_capability_registry();
    CapabilityMarketplace::new(registry)
}

/// Creates a new capability marketplace with the provided registry
pub fn create_capability_marketplace_with_registry(
    registry: Arc<RwLock<CapabilityRegistry>>
) -> CapabilityMarketplace {
    CapabilityMarketplace::new(registry)
}

/// Creates a new causal chain wrapped in Rc<RefCell<>>
pub fn create_causal_chain() -> Rc<RefCell<CausalChain>> {
    Rc::new(RefCell::new(CausalChain::new().unwrap()))
}

/// Creates a new delegation engine with empty configuration
pub fn create_delegation_engine() -> Arc<StaticDelegationEngine> {
    Arc::new(StaticDelegationEngine::new(HashMap::new()))
}

/// Creates a new module registry wrapped in Rc<>
pub fn create_module_registry() -> Rc<ModuleRegistry> {
    Rc::new(ModuleRegistry::new())
}

/// Creates a runtime host with the specified security context
pub fn create_runtime_host(security_context: RuntimeContext) -> std::sync::Arc<RuntimeHost> {
    let marketplace = Arc::new(create_capability_marketplace());
    let causal_chain = create_causal_chain();
    
    std::sync::Arc::new(RuntimeHost::new(
        marketplace,
        causal_chain,
        security_context,
    ))
}

/// Creates a runtime host with a shared marketplace and security context
pub fn create_runtime_host_with_marketplace(
    marketplace: Arc<CapabilityMarketplace>,
    security_context: RuntimeContext,
) -> std::sync::Arc<RuntimeHost> {
    let causal_chain = create_causal_chain();
    
    std::sync::Arc::new(RuntimeHost::new(
        marketplace,
        causal_chain,
        security_context,
    ))
}

/// Creates a complete evaluator with the specified security context
pub fn create_evaluator(security_context: RuntimeContext) -> Evaluator {
    let module_registry = create_module_registry();
    let delegation_engine = create_delegation_engine();
    let stdlib_env = StandardLibrary::create_global_environment();
    let host = create_runtime_host(security_context.clone());
    
    Evaluator::with_environment(
        module_registry,
        stdlib_env,
        delegation_engine,
        security_context,
        host,
    )
}

/// Creates a complete evaluator with a shared marketplace and security context
pub fn create_evaluator_with_marketplace(
    marketplace: Arc<CapabilityMarketplace>,
    security_context: RuntimeContext,
) -> Evaluator {
    let module_registry = create_module_registry();
    let delegation_engine = create_delegation_engine();
    let stdlib_env = StandardLibrary::create_global_environment();
    let host = create_runtime_host_with_marketplace(marketplace, security_context.clone());
    
    Evaluator::with_environment(
        module_registry,
        stdlib_env,
        delegation_engine,
        security_context,
        host,
    )
}

/// Test helper: Creates an evaluator with pure security context (no capabilities allowed)
pub fn create_pure_evaluator() -> Evaluator {
    create_evaluator(RuntimeContext::pure())
}

/// Test helper: Creates an evaluator with controlled security context
pub fn create_controlled_evaluator(allowed_capabilities: Vec<String>) -> Evaluator {
    create_evaluator(RuntimeContext::controlled(allowed_capabilities))
}

/// Test helper: Creates an evaluator with full security context (all capabilities allowed)
pub fn create_full_evaluator() -> Evaluator {
    create_evaluator(RuntimeContext::full())
}

/// Creates a shared marketplace and evaluator for testing HTTP capabilities
pub async fn create_http_test_setup() -> (Arc<CapabilityMarketplace>, Evaluator) {
    let marketplace = Arc::new(create_capability_marketplace());
    
    // Register basic HTTP capability
    marketplace.register_http_capability(
        "http.get".to_string(),
        "HTTP GET Request".to_string(),
        "Performs HTTP GET request".to_string(),
        "https://httpbin.org/get".to_string(),
        None,
    ).await.expect("Failed to register HTTP capability");
    
    let security_context = RuntimeContext::controlled(vec!["http.get".to_string()]);
    let evaluator = create_evaluator_with_marketplace(marketplace.clone(), security_context);
    
    (marketplace, evaluator)
}

/// Creates a shared marketplace and evaluator for testing MCP capabilities
pub fn create_mcp_test_setup() -> (Arc<CapabilityMarketplace>, Evaluator) {
    let marketplace = Arc::new(create_capability_marketplace());
    
    // MCP capabilities would be registered here
    // TODO: Add MCP capability registration when MCP implementation is complete
    
    let security_context = RuntimeContext::controlled(vec!["mcp.test".to_string()]);
    let evaluator = create_evaluator_with_marketplace(marketplace.clone(), security_context);
    
    (marketplace, evaluator)
}
