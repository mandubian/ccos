// Minimal test helpers for RTFS-only tests
// This provides only the essential functions needed for pure RTFS testing

use rtfs::runtime::pure_host::create_pure_host;
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::{Evaluator, ModuleRegistry};
use std::sync::Arc;

/// Creates a pure RTFS evaluator without CCOS dependencies
pub fn create_pure_evaluator() -> Evaluator {
    let module_registry = Arc::new(ModuleRegistry::new());
    let security_context = RuntimeContext::pure();
    let host = create_pure_host();
    Evaluator::new(module_registry, security_context, host)
}

/// Creates a pure RTFS evaluator with custom security context
pub fn create_pure_evaluator_with_context(security_context: RuntimeContext) -> Evaluator {
    let module_registry = Arc::new(ModuleRegistry::new());
    let host = create_pure_host();
    Evaluator::new(module_registry, security_context, host)
}
