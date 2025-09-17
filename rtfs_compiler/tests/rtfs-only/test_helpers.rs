//! Test helper functions for creating standardized runtime components
//! 
//! This module provides reusable functions for initializing capability registry,
//! marketplace, runtime host, and evaluators with consistent patterns across tests.

use rtfs_compiler::ccos::delegation::StaticDelegationEngine;
use rtfs_compiler::ccos::causal_chain::CausalChain;
use rtfs_compiler::runtime::{Evaluator, ModuleRegistry};
use rtfs_compiler::runtime::stdlib::{StandardLibrary, register_default_capabilities};
use rtfs_compiler::runtime::security::RuntimeContext;
use rtfs_compiler::ccos::host::RuntimeHost;
use rtfs_compiler::runtime::host_interface::HostInterface;
use rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace;
use rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::cell::RefCell;
use tokio::sync::RwLock;

/// Creates a new capability registry wrapped in Arc<RwLock<>>
pub fn create_capability_registry() -> Arc<RwLock<CapabilityRegistry>> {
    Arc::new(RwLock::new(CapabilityRegistry::new()))
}

/// Creates a new capability marketplace with basic registry
pub fn create_capability_marketplace() -> CapabilityMarketplace {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    CapabilityMarketplace::new(registry)
}

/// Creates a populated capability marketplace with default capabilities (async)
pub async fn create_populated_capability_marketplace() -> CapabilityMarketplace {
    let marketplace = create_capability_marketplace();
    register_default_capabilities(&marketplace).await
        .expect("Failed to register default capabilities");
    marketplace
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
    Arc::new(StaticDelegationEngine::new(std::collections::HashMap::new()))
}

/// Creates a new module registry wrapped in Rc<>
pub fn create_module_registry() -> std::sync::Arc<ModuleRegistry> {
    std::sync::Arc::new(ModuleRegistry::new())
}

/// Creates a runtime host with the specified security context
pub fn create_runtime_host(security_context: RuntimeContext) -> std::sync::Arc<RuntimeHost> {
    let marketplace = Arc::new(create_capability_marketplace());
    let causal_chain = create_causal_chain();
    
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry::new()));
    let capability_marketplace = std::sync::Arc::new(rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace::new(registry));
    let causal_chain = std::sync::Arc::new(std::sync::Mutex::new(rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap()));
    let security_context = rtfs_compiler::runtime::security::RuntimeContext::pure();
    let host = std::sync::Arc::new(rtfs_compiler::ccos::host::RuntimeHost::new(
        causal_chain,
        capability_marketplace,
        security_context.clone(),
    ));
    return host;
}

/// Creates a runtime host with a shared marketplace and security context
pub fn create_runtime_host_with_marketplace(
    capability_marketplace: Arc<CapabilityMarketplace>,
    security_context: RuntimeContext,
) -> std::sync::Arc<RuntimeHost> {
    
    let causal_chain = std::sync::Arc::new(std::sync::Mutex::new(rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap()));
    let host = std::sync::Arc::new(rtfs_compiler::ccos::host::RuntimeHost::new(
        causal_chain,
        capability_marketplace,
        security_context.clone(),
    ));
    return host;
}

/// Creates a complete evaluator with the specified security context
pub fn create_evaluator(security_context: RuntimeContext) -> Evaluator {
    let module_registry = create_module_registry();
    let stdlib_env = StandardLibrary::create_global_environment();
    let host = create_runtime_host(security_context.clone());
    
    Evaluator::with_environment(
        module_registry,
        stdlib_env,
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
    let stdlib_env = StandardLibrary::create_global_environment();
    let host = create_runtime_host_with_marketplace(marketplace, security_context.clone());
    
    Evaluator::with_environment(
        module_registry,
        stdlib_env,
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

/// Async test helper: Creates an evaluator with pure security context (no capabilities allowed)
pub async fn create_pure_evaluator_async() -> Evaluator {
    let marketplace = Arc::new(create_populated_capability_marketplace().await);
    create_evaluator_with_marketplace(marketplace, RuntimeContext::pure())
}

/// Async test helper: Creates an evaluator with controlled security context
pub async fn create_controlled_evaluator_async(allowed_capabilities: Vec<String>) -> Evaluator {
    let marketplace = Arc::new(create_populated_capability_marketplace().await);
    create_evaluator_with_marketplace(marketplace, RuntimeContext::controlled(allowed_capabilities))
}

/// Async test helper: Creates an evaluator with full security context (all capabilities allowed)
pub async fn create_full_evaluator_async() -> Evaluator {
    let marketplace = Arc::new(create_populated_capability_marketplace().await);
    create_evaluator_with_marketplace(marketplace, RuntimeContext::full())
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
    let evaluator = create_evaluator_with_marketplace(
        marketplace.clone(),
        RuntimeContext::controlled(vec!["mcp".to_string()])
    );
    
    (marketplace, evaluator)
}

/// Helper to set up execution context for testing (required before running RTFS code with capabilities)
pub fn setup_execution_context(host: &dyn HostInterface) {
    host.set_execution_context(
        "test_plan_id".to_string(), 
        vec!["test_intent_id".to_string()],
        "test_parent_action_id".to_string()
    );
}

/// Helper to clean up execution context after testing
pub fn cleanup_execution_context(host: &dyn HostInterface) {
    host.clear_execution_context();
}
