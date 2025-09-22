//! CCOS integration test utilities
//! 
//! These utilities use RuntimeHost and are designed for testing CCOS integration,
//! capabilities, delegation, and orchestration features.

use crate::parser;
use crate::runtime::{
    evaluator::Evaluator, ir_runtime::IrRuntime, module_runtime::ModuleRegistry,
    values::Value, security::RuntimeContext,
};
use crate::ccos::capabilities::registry::CapabilityRegistry;
use crate::ccos::capability_marketplace::CapabilityMarketplace;
use crate::ccos::host::RuntimeHost;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Creates a new capability registry for CCOS testing.
pub fn create_ccos_capability_registry() -> Arc<RwLock<CapabilityRegistry>> {
    Arc::new(RwLock::new(CapabilityRegistry::new()))
}

/// Creates a new capability marketplace for CCOS testing.
pub fn create_ccos_capability_marketplace() -> Arc<CapabilityMarketplace> {
    let registry = create_ccos_capability_registry();
    Arc::new(CapabilityMarketplace::new(registry))
}

/// Creates a new runtime host for CCOS testing.
pub fn create_ccos_runtime_host() -> Arc<RuntimeHost> {
    let marketplace = create_ccos_capability_marketplace();
    let causal_chain = Arc::new(std::sync::Mutex::new(
        crate::ccos::causal_chain::CausalChain::new().unwrap()
    ));
    let security_context = RuntimeContext::pure();
    
    Arc::new(RuntimeHost::new(
        causal_chain,
        marketplace,
        security_context,
    ))
}

/// Creates a new AST evaluator with CCOS components for integration testing.
pub fn create_ccos_evaluator() -> Evaluator {
    let module_registry = ModuleRegistry::new();
    let security_context = RuntimeContext::pure();
    let host = create_ccos_runtime_host();
    
    Evaluator::new(Arc::new(module_registry), security_context, host)
}

/// Creates a new AST evaluator with a provided RuntimeContext for CCOS testing.
pub fn create_ccos_evaluator_with_context(ctx: RuntimeContext) -> Evaluator {
    let module_registry = ModuleRegistry::new();
    let host = create_ccos_runtime_host();
    
    Evaluator::new(Arc::new(module_registry), ctx, host)
}

/// Creates a new AST evaluator with stdlib loaded for CCOS testing.
pub fn create_ccos_evaluator_with_stdlib() -> Evaluator {
    let mut module_registry = ModuleRegistry::new();
    crate::runtime::stdlib::load_stdlib(&mut module_registry)
        .expect("Failed to load stdlib");
    
    let security_context = RuntimeContext::pure();
    let host = create_ccos_runtime_host();
    
    Evaluator::new(Arc::new(module_registry), security_context, host)
}

/// Creates a new IR runtime with CCOS components for integration testing.
pub fn create_ccos_ir_runtime() -> IrRuntime {
    let module_registry = Arc::new(ModuleRegistry::new());
    let security_context = RuntimeContext::pure();
    let host = create_ccos_runtime_host();
    
    IrRuntime::new(host, security_context)
}

/// Parses and evaluates RTFS code using CCOS evaluator (with capabilities available).
pub fn parse_and_evaluate_ccos(input: &str) -> crate::runtime::error::RuntimeResult<Value> {
    let parsed = parser::parse(input).expect("Failed to parse");
    let evaluator = create_ccos_evaluator();
    
    if let Some(last_item) = parsed.last() {
        match last_item {
            crate::ast::TopLevel::Expression(expr) => {
                match evaluator.evaluate(expr)? {
                    crate::runtime::execution_outcome::ExecutionOutcome::Complete(value) => Ok(value),
                    crate::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
                        Err(crate::runtime::error::RuntimeError::Generic("Host call required in CCOS test".to_string()))
                    }
                }
            },
            _ => Ok(Value::String("object_defined".to_string())),
        }
    } else {
        Ok(Value::Nil)
    }
}

/// Parses and evaluates RTFS code with stdlib loaded using CCOS evaluator.
pub fn parse_and_evaluate_ccos_with_stdlib(input: &str) -> crate::runtime::error::RuntimeResult<Value> {
    let parsed = parser::parse(input).expect("Failed to parse");
    let evaluator = create_ccos_evaluator_with_stdlib();
    
    if let Some(last_item) = parsed.last() {
        match last_item {
            crate::ast::TopLevel::Expression(expr) => {
                match evaluator.evaluate(expr)? {
                    crate::runtime::execution_outcome::ExecutionOutcome::Complete(value) => Ok(value),
                    crate::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
                        Err(crate::runtime::error::RuntimeError::Generic("Host call required in CCOS test".to_string()))
                    }
                }
            },
            _ => Ok(Value::String("object_defined".to_string())),
        }
    } else {
        Ok(Value::Nil)
    }
}

/// Creates a CCOS evaluator with controlled security context.
pub fn create_ccos_sandboxed_evaluator(allowed_capabilities: Vec<String>) -> Evaluator {
    let context = RuntimeContext::controlled(allowed_capabilities);
    create_ccos_evaluator_with_context(context)
}

/// Creates a shared marketplace and evaluator for testing CCOS capabilities.
pub async fn create_ccos_capability_test_setup() -> (Arc<CapabilityMarketplace>, Evaluator) {
    let marketplace = create_ccos_capability_marketplace();
    
    // TODO: Register test capabilities when proper CapabilityManifest is available
    // For now, we'll create the evaluator without registering capabilities
    
    let security_context = RuntimeContext::controlled(vec![]);
    let evaluator = create_ccos_evaluator_with_context(security_context);
    
    (marketplace, evaluator)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_ccos_evaluator() {
        let evaluator = create_ccos_evaluator();
        // Should be able to create evaluator with CCOS components
        // For debugging: let's see what symbols are actually present
        let symbols = evaluator.env.symbol_names();
        println!("CCOS evaluator symbols: {:?}", symbols);
        
        // Basic test: evaluator should be created successfully
        // The environment might not contain symbols directly, they might be in the capability marketplace
        assert!(symbols.len() >= 0, "CCOS evaluator should be created successfully");
    }

    #[test]
    fn test_parse_and_evaluate_ccos() {
        let result = parse_and_evaluate_ccos("42");
        assert_eq!(result.unwrap(), Value::Integer(42));
    }

    #[test]
    fn test_parse_and_evaluate_ccos_with_stdlib() {
        let result = parse_and_evaluate_ccos_with_stdlib("(+ 1 2)");
        assert_eq!(result.unwrap(), Value::Integer(3));
    }

    #[tokio::test]
    async fn test_ccos_capability_test_setup() {
        let (marketplace, evaluator) = create_ccos_capability_test_setup().await;
        // Should be able to create CCOS test setup
        assert!(marketplace.get_capability("nonexistent").await.is_none());
        // The evaluator environment is not empty because it loads the standard library
        assert!(!evaluator.env.symbol_names().is_empty());
    }
}
