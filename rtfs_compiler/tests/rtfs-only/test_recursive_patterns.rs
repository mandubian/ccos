use rtfs_compiler::ccos::host::RuntimeHost;
use rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace;
use rtfs_compiler::ccos::causal_chain::CausalChain;
use rtfs_compiler::runtime::security::RuntimeContext;
use rtfs_compiler::runtime::module_runtime::ModuleRegistry;
use std::sync::Arc;
// Test for recursive function patterns
use rtfs_compiler::runtime::evaluator::Evaluator;
use rtfs_compiler::*;

#[test]
fn test_mutual_recursion_pattern() {
    let code = include_str!("rtfs_files/test_mutual_recursion.rtfs");

    let parsed = parser::parse(code).expect("Should parse successfully");
    let module_registry = Arc::new(ModuleRegistry::new());
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry::new()));
    let capability_marketplace = std::sync::Arc::new(rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace::new(registry));
    let causal_chain = std::sync::Arc::new(std::sync::Mutex::new(rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap()));
    let security_context = rtfs_compiler::runtime::security::RuntimeContext::pure();
    let host = std::sync::Arc::new(rtfs_compiler::ccos::host::RuntimeHost::new(
        causal_chain,
        capability_marketplace,
        security_context.clone(),
    ));
    let evaluator = Evaluator::new(
        module_registry,
        std::sync::Arc::new(rtfs_compiler::ccos::delegation::StaticDelegationEngine::new(std::collections::HashMap::new())),
        rtfs_compiler::runtime::security::RuntimeContext::pure(),
        host,
    );
    let result = if let TopLevel::Expression(expr) = &parsed[0] {
        evaluator
            .evaluate(expr)
            .expect("Should evaluate successfully")
    } else {
        panic!("Expected a top-level expression");
    };

    // Expected: [true, false, false, true] for (is-even 4), (is-odd 4), (is-even 7), (is-odd 7)
    if let runtime::values::Value::Vector(vec) = result {
        assert_eq!(vec.len(), 4);
        assert_eq!(vec[0], runtime::values::Value::Boolean(true)); // is-even 4
        assert_eq!(vec[1], runtime::values::Value::Boolean(false)); // is-odd 4
        assert_eq!(vec[2], runtime::values::Value::Boolean(false)); // is-even 7
        assert_eq!(vec[3], runtime::values::Value::Boolean(true)); // is-odd 7
    } else {
        panic!("Expected vector result, got: {:?}", result);
    }
}

#[test]
fn test_nested_recursion_pattern() {
    let code = include_str!("rtfs_files/test_nested_recursion.rtfs");

    let parsed = parser::parse_expression(code).expect("Should parse successfully");
    let module_registry = Arc::new(ModuleRegistry::new());
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry::new()));
    let capability_marketplace = std::sync::Arc::new(rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace::new(registry));
    let causal_chain = std::sync::Arc::new(std::sync::Mutex::new(rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap()));
    let security_context = rtfs_compiler::runtime::security::RuntimeContext::pure();
    let host = std::sync::Arc::new(rtfs_compiler::ccos::host::RuntimeHost::new(
        causal_chain,
        capability_marketplace,
        security_context.clone(),
    ));
    let evaluator = Evaluator::new(
        module_registry,
        std::sync::Arc::new(rtfs_compiler::ccos::delegation::StaticDelegationEngine::new(std::collections::HashMap::new())),
        rtfs_compiler::runtime::security::RuntimeContext::pure(),
        host,
    );
    let result = evaluator
        .evaluate(&parsed)
        .expect("Should evaluate successfully");

    // Should return a countdown vector [5, 4, 3, 2, 1]
    println!("Nested recursion result: {}", result);
}

#[test]
fn test_higher_order_recursion_pattern() {
    let code = include_str!("rtfs_files/test_higher_order_recursion.rtfs");

    let parsed = parser::parse_expression(code).expect("Should parse successfully");
    let module_registry = Arc::new(ModuleRegistry::new());
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry::new()));
    let capability_marketplace = std::sync::Arc::new(rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace::new(registry));
    let causal_chain = std::sync::Arc::new(std::sync::Mutex::new(rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap()));
    let security_context = rtfs_compiler::runtime::security::RuntimeContext::pure();
    let host = std::sync::Arc::new(rtfs_compiler::ccos::host::RuntimeHost::new(
        causal_chain,
        capability_marketplace,
        security_context.clone(),
    ));
    let evaluator = Evaluator::new(
        module_registry,
        std::sync::Arc::new(rtfs_compiler::ccos::delegation::StaticDelegationEngine::new(std::collections::HashMap::new())),
        rtfs_compiler::runtime::security::RuntimeContext::pure(),
        host,
    );
    let result = evaluator
        .evaluate(&parsed)
        .expect("Should evaluate successfully");

    // Should return squares: [1, 4, 9, 16, 25]
    println!("Higher-order recursion result: {}", result);
}

#[test]
fn test_three_way_recursion_pattern() {
    let code = include_str!("rtfs_files/test_three_way_recursion.rtfs");

    let parsed = parser::parse_expression(code).expect("Should parse successfully");
    let module_registry = Arc::new(ModuleRegistry::new());
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry::new()));
    let capability_marketplace = std::sync::Arc::new(rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace::new(registry));
    let causal_chain = std::sync::Arc::new(std::sync::Mutex::new(rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap()));
    let security_context = rtfs_compiler::runtime::security::RuntimeContext::pure();
    let host = std::sync::Arc::new(rtfs_compiler::ccos::host::RuntimeHost::new(
        causal_chain,
        capability_marketplace,
        security_context.clone(),
    ));
    let evaluator = Evaluator::new(
        module_registry,
        std::sync::Arc::new(rtfs_compiler::ccos::delegation::StaticDelegationEngine::new(std::collections::HashMap::new())),
        rtfs_compiler::runtime::security::RuntimeContext::pure(),
        host,
    );
    let result = evaluator
        .evaluate(&parsed)
        .expect("Should evaluate successfully");

    // Should return cycle results
    println!("Three-way recursion result: {}", result);
}
