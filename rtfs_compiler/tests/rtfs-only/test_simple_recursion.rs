use rtfs_compiler::runtime::module_runtime::ModuleRegistry;
// Simple test for basic recursion functionality
use rtfs_compiler::runtime::evaluator::Evaluator;
use rtfs_compiler::*;

#[test]
fn test_simple_mutual_recursion() {
    let code = r#"(let [is-even (fn [n]
                (if (= n 0)
                  true
                  (is-odd (- n 1))))
      is-odd (fn [n]
               (if (= n 0)
                 false
                 (is-even (- n 1))))]
  (vector (is-even 4) (is-odd 4) (is-even 7) (is-odd 7)))"#;

    let parsed = parser::parse_expression(code).expect("Should parse successfully");
    let module_registry = std::sync::Arc::new(ModuleRegistry::new());
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
    let outcome = evaluator
        .evaluate(&parsed)
        .expect("Should evaluate successfully");

    let result = match outcome {
        rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
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
fn test_simple_factorial() {
    let code = r#"(let [fact (fn [n]
                     (if (= n 0)
                       1
                       (* n (fact (- n 1)))))]
  (fact 5))"#;

    let parsed = parser::parse_expression(code).expect("Should parse successfully");
    let module_registry = std::sync::Arc::new(ModuleRegistry::new());
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
    let result = evaluator
        .evaluate(&parsed)
        .expect("Should evaluate successfully");

    // Expected: 120 (5!)
    match result {
        rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::Complete(
            runtime::values::Value::Integer(120),
        ) => {}
        _ => panic!("Expected Complete(Integer(120)) result"),
    }
}
