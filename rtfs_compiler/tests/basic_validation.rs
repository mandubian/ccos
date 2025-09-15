// Test file to validate basic functionality without tokio timeout dependency
use rtfs_compiler::runtime::module_runtime::ModuleRegistry;
use rtfs_compiler::parser;
use rtfs_compiler::runtime::Evaluator;
use rtfs_compiler::runtime::RuntimeResult;
use rtfs_compiler::runtime::Value;
use rtfs_compiler::runtime::delegation::StaticDelegationEngine;
use std::collections::HashMap;
use std::sync::Arc;
// use std::rc::Rc; // legacy

fn parse_and_evaluate(input: &str) -> RuntimeResult<Value> {
    let parsed = parser::parse(input).expect("Failed to parse");
    let mut module_registry = ModuleRegistry::new();
    // Load stdlib to get basic functions
    rtfs_compiler::runtime::stdlib::load_stdlib(&mut module_registry).expect("Failed to load stdlib");
    let de = Arc::new(StaticDelegationEngine::new_empty());
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry::new()));
    let capability_marketplace = std::sync::Arc::new(rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace::new(registry));
    let causal_chain = std::sync::Arc::new(std::sync::Mutex::new(rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap()));
    let security_context = rtfs_compiler::runtime::security::RuntimeContext::pure();
    let host = std::sync::Arc::new(rtfs_compiler::ccos::host::RuntimeHost::new(
        causal_chain,
        capability_marketplace,
        security_context.clone(),
    ));
    let mut evaluator = Evaluator::new(Arc::new(module_registry), de, security_context, host);
    evaluator.eval_toplevel(&parsed)
}

#[cfg(test)]
mod basic_tests {
    use super::*;

    #[test]
    fn test_basic_arithmetic() {
        let code = "(+ 1 2)";
        match parse_and_evaluate(code) {
            Ok(result) => {
                println!("Test result: {}", result);
                // Basic validation that something was computed
                assert!(matches!(result, Value::Integer(3)), "Expected 3, got {:?}", result);
            }
            Err(e) => {
                println!("Error: {:?}", e);
                assert!(false, "Basic arithmetic should work");
            }
        }
    }

    #[test]
    fn test_simple_literal() {
        let code = "42";
        match parse_and_evaluate(code) {
            Ok(result) => {
                println!("Literal result: {}", result);
                assert!(matches!(result, Value::Integer(42)), "Expected 42, got {:?}", result);
            }
            Err(e) => {
                println!("Error: {:?}", e);
                assert!(false, "Simple literal should work");
            }
        }
    }
}
