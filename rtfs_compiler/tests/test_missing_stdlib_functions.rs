use rtfs_compiler::runtime::module_runtime::ModuleRegistry;
use std::rc::Rc;
use std::sync::Arc;
use rtfs_compiler::runtime::stdlib::StandardLibrary;
use rtfs_compiler::runtime::evaluator::Evaluator;
use rtfs_compiler::parser::parse_expression;
use rtfs_compiler::runtime::values::Value;

#[test]
fn test_missing_stdlib_functions() {
    let mut env = StandardLibrary::create_global_environment();
    let module_registry = Rc::new(ModuleRegistry::new());
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(rtfs_compiler::runtime::capability_registry::CapabilityRegistry::new()));
    let capability_marketplace = std::sync::Arc::new(rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace::new(registry));
    let causal_chain = std::sync::Arc::new(std::sync::Mutex::new(rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap()));
    let security_context = rtfs_compiler::runtime::security::RuntimeContext::pure();
    let host = std::rc::Rc::new(rtfs_compiler::runtime::host::RuntimeHost::new(
        causal_chain,
        capability_marketplace,
        security_context.clone(),
    ));
    let evaluator = Evaluator::new(module_registry, std::sync::Arc::new(rtfs_compiler::ccos::delegation::StaticDelegationEngine::new(std::collections::HashMap::new())), rtfs_compiler::runtime::security::RuntimeContext::pure(), host);

    // Test empty?
    let expr = parse_expression("(empty? [])").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    assert_eq!(result, Value::Boolean(true));

    let expr = parse_expression("(empty? [1 2 3])").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    assert_eq!(result, Value::Boolean(false));

    // Test cons
    let expr = parse_expression("(cons 1 [2 3])").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    if let Value::Vector(v) = result {
        assert_eq!(v.len(), 3);
        assert_eq!(v[0], Value::Integer(1));
        assert_eq!(v[1], Value::Integer(2));
        assert_eq!(v[2], Value::Integer(3));
    } else {
        panic!("Expected vector result from cons");
    }

    // Test first
    let expr = parse_expression("(first [1 2 3])").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    assert_eq!(result, Value::Integer(1));

    let expr = parse_expression("(first [])").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    assert_eq!(result, Value::Nil);

    // Test rest
    let expr = parse_expression("(rest [1 2 3])").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    if let Value::Vector(v) = result {
        assert_eq!(v.len(), 2);
        assert_eq!(v[0], Value::Integer(2));
        assert_eq!(v[1], Value::Integer(3));
    } else {
        panic!("Expected vector result from rest");
    }

    let expr = parse_expression("(rest [])").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    if let Value::Vector(v) = result {
        assert_eq!(v.len(), 0);
    } else {
        panic!("Expected empty vector result from rest");
    }

    println!("All missing stdlib functions are working correctly!");
}
