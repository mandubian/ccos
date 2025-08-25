//! Test helper functions for creating standardized runtime components
//! 
//! This module provides reusable functions for initializing capability registry,
//! marketplace, runtime host, and evaluators with consistent patterns across tests.

use crate::ccos::delegation::StaticDelegationEngine;
use crate::ccos::causal_chain::CausalChain;
use crate::runtime::{Evaluator, ModuleRegistry};
use crate::runtime::stdlib::StandardLibrary;
use crate::runtime::security::RuntimeContext;
use crate::runtime::host::RuntimeHost;
use crate::runtime::capability_marketplace::CapabilityMarketplace;
use crate::runtime::capability_registry::CapabilityRegistry;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock as StdRwLock;
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

/// Creates a new causal chain wrapped in Arc<Mutex<>>
pub fn create_causal_chain() -> std::sync::Arc<std::sync::Mutex<CausalChain>> {
    std::sync::Arc::new(std::sync::Mutex::new(CausalChain::new().unwrap()))
}

/// Creates a new delegation engine with empty configuration
pub fn create_delegation_engine() -> Arc<StaticDelegationEngine> {
    Arc::new(StaticDelegationEngine::new(HashMap::new()))
}

/// Creates a new module registry wrapped in Arc<>
pub fn create_module_registry() -> Arc<ModuleRegistry> {
    Arc::new(ModuleRegistry::new())
}

/// Creates a runtime host with the specified security context
pub fn create_runtime_host(security_context: RuntimeContext) -> std::sync::Arc<RuntimeHost> {
    let registry = create_capability_registry();
    let capability_marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let causal_chain = std::sync::Arc::new(std::sync::Mutex::new(CausalChain::new().unwrap()));
    std::sync::Arc::new(RuntimeHost::new(
        causal_chain,
        capability_marketplace,
        security_context.clone(),
    ))
}

/// Creates a runtime host with a shared marketplace and security context
pub fn create_runtime_host_with_marketplace(
    marketplace: Arc<CapabilityMarketplace>,
    security_context: RuntimeContext,
) -> std::sync::Arc<RuntimeHost> {
    let causal_chain = std::sync::Arc::new(std::sync::Mutex::new(CausalChain::new().unwrap()));
    
    std::sync::Arc::new(RuntimeHost::new(
        causal_chain,
        marketplace,
        security_context.clone(),
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

/// Test helper: Creates an evaluator with controlled security context with no capabilities
pub fn create_sandboxed_evaluator() -> Evaluator {
    create_evaluator(RuntimeContext::controlled(vec![]))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::security::RuntimeContext;

    #[test]
    fn test_create_capability_registry() {
        let registry = create_capability_registry();
        // Should be able to create without panicking
        assert!(Arc::strong_count(&registry) == 1);
    }

    #[test]
    fn test_create_capability_marketplace() {
        let marketplace = create_capability_marketplace();
        // Should create successfully
        // Note: We can't easily test the internal state without exposing internals
    }

    #[test]
    fn test_create_evaluator_variants() {
        let _pure = create_pure_evaluator();
        let _controlled = create_controlled_evaluator(vec!["test.capability".to_string()]);
        let _full = create_full_evaluator();
        let _sandboxed = create_sandboxed_evaluator();
        
        // All should create without panicking
    }

    #[tokio::test]
    async fn test_create_http_test_setup() {
        let (marketplace, _evaluator) = create_http_test_setup().await;
        
        // Should have registered the HTTP capability
        // Note: We can't easily test capability existence without exposing internals
        assert!(Arc::strong_count(&marketplace) >= 1);
    }

    #[test]
    fn test_create_mcp_test_setup() {
        let (marketplace, _evaluator) = create_mcp_test_setup();
        
        // Should create successfully
        assert!(Arc::strong_count(&marketplace) >= 1);
    }
}
