use rtfs_compiler::parser::parse_expression;
use rtfs_compiler::runtime::evaluator::Evaluator;
use rtfs_compiler::runtime::module_runtime::ModuleRegistry;
use rtfs_compiler::runtime::stdlib::StandardLibrary;
use rtfs_compiler::runtime::values::Value;
use std::sync::Arc;

#[test]
fn test_missing_stdlib_functions() {
    let env = StandardLibrary::create_global_environment();
    let module_registry = Arc::new(ModuleRegistry::new());
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(
        rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry::new(),
    ));
    let capability_marketplace = std::sync::Arc::new(
        rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace::new(registry),
    );
    let causal_chain = std::sync::Arc::new(std::sync::Mutex::new(
        rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap(),
    ));
    let security_context = rtfs_compiler::runtime::security::RuntimeContext::pure();
    let host = std::sync::Arc::new(rtfs_compiler::ccos::host::RuntimeHost::new(
        causal_chain,
        capability_marketplace,
        security_context.clone(),
    ));
    let evaluator = Evaluator::new(
        module_registry,
        rtfs_compiler::runtime::security::RuntimeContext::pure(),
        host,
    );

    // Test empty?
    let expr = parse_expression("(empty? [])").expect("Parse failed");
    let result = evaluator.evaluate(&expr).expect("Evaluation failed");
    match result {
        rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::Complete(Value::Boolean(
            true,
        )) => {}
        _ => panic!("Expected Complete(Boolean(true)) result"),
    }

    let expr = parse_expression("(empty? [1 2 3])").expect("Parse failed");
    let result = evaluator.evaluate(&expr).expect("Evaluation failed");
    match result {
        rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::Complete(Value::Boolean(
            false,
        )) => {}
        _ => panic!("Expected Complete(Boolean(false)) result"),
    }

    // Test cons
    let expr = parse_expression("(cons 1 [2 3])").expect("Parse failed");
    let result = evaluator.evaluate(&expr).expect("Evaluation failed");
    match result {
        rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::Complete(Value::Vector(v)) => {
            assert_eq!(v.len(), 3);
            assert_eq!(v[0], Value::Integer(1));
            assert_eq!(v[1], Value::Integer(2));
            assert_eq!(v[2], Value::Integer(3));
        }
        _ => panic!("Expected Complete(Vector) result from cons"),
    }

    // Test first
    let expr = parse_expression("(first [1 2 3])").expect("Parse failed");
    let result = evaluator.evaluate(&expr).expect("Evaluation failed");
    match result {
        rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::Complete(Value::Integer(
            1,
        )) => {}
        _ => panic!("Expected Complete(Integer(1)) result"),
    }

    let expr = parse_expression("(first [])").expect("Parse failed");
    let result = evaluator.evaluate(&expr).expect("Evaluation failed");
    match result {
        rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::Complete(Value::Nil) => {}
        rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call")
        }
        _ => panic!("Unexpected result: {:?}", result),
    }

    // Test rest
    let expr = parse_expression("(rest [1 2 3])").expect("Parse failed");
    let result = evaluator.evaluate(&expr).expect("Evaluation failed");
    match result {
        rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::Complete(Value::Vector(v)) => {
            assert_eq!(v.len(), 2);
            assert_eq!(v[0], Value::Integer(2));
            assert_eq!(v[1], Value::Integer(3));
        }
        _ => panic!("Expected Complete(Vector) result from rest"),
    }

    let expr = parse_expression("(rest [])").expect("Parse failed");
    let result = evaluator.evaluate(&expr).expect("Evaluation failed");
    match result {
        rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::Complete(Value::Vector(v)) => {
            assert_eq!(v.len(), 0);
        }
        _ => panic!("Expected Complete(Vector) result from rest"),
    }

    println!("All missing stdlib functions are working correctly!");
}
