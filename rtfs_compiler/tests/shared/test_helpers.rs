// Minimal test helpers for RTFS-only tests
// This provides only the essential functions needed for pure RTFS testing

use rtfs_compiler::runtime::{Evaluator, ModuleRegistry};
use rtfs_compiler::runtime::security::RuntimeContext;
use rtfs_compiler::ccos::host::RuntimeHost;
use rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace;
use rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry;
use rtfs_compiler::ccos::causal_chain::CausalChain;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::RwLock;

/// Creates a minimal host for pure RTFS tests
fn create_minimal_host() -> Arc<dyn rtfs_compiler::runtime::host_interface::HostInterface> {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let capability_marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let causal_chain = Arc::new(Mutex::new(CausalChain::new().unwrap()));
    let security_context = RuntimeContext::pure();
    
    Arc::new(RuntimeHost::new(
        causal_chain,
        capability_marketplace,
        security_context,
    ))
}

/// Creates a pure RTFS evaluator without CCOS dependencies
pub fn create_pure_evaluator() -> Evaluator {
    let module_registry = Arc::new(ModuleRegistry::new());
    let security_context = RuntimeContext::pure();
    let host = create_minimal_host();
    Evaluator::new(module_registry, security_context, host)
}

/// Creates a pure RTFS evaluator with custom security context
pub fn create_pure_evaluator_with_context(security_context: RuntimeContext) -> Evaluator {
    let module_registry = Arc::new(ModuleRegistry::new());
    let host = create_minimal_host();
    Evaluator::new(module_registry, security_context, host)
}