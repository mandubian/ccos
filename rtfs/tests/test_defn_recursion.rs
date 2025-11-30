use rtfs::runtime::evaluator::Evaluator;
use rtfs::runtime::module_runtime::ModuleRegistry;
use rtfs::runtime::pure_host::create_pure_host;
use rtfs::*;
use std::sync::Arc;

#[test]
fn test_defn_recursion_persistence() {
    let code = r#"
        (defn factorial [n] 
            (if (= n 0) 
                1 
                (* n (factorial (- n 1)))))
        (factorial 5)
    "#;

    let parsed = parser::parse(code).expect("Should parse successfully");
    let module_registry = Arc::new(ModuleRegistry::new());
    let security_context = rtfs::runtime::security::RuntimeContext::pure();
    let host = create_pure_host();
    let mut evaluator = Evaluator::new(module_registry, security_context, host);

    // Evaluate the definition using evaluate_with_env to persist it
    if let TopLevel::Expression(expr) = &parsed[0] {
        // We need to use the evaluator's environment
        // Since we can't borrow evaluator immutably and its env mutably, we swap
        let mut env = std::mem::replace(
            &mut evaluator.env,
            rtfs::runtime::environment::Environment::new(),
        );
        evaluator
            .evaluate_with_env(expr, &mut env)
            .expect("Definition should succeed");
        evaluator.env = env;
    }

    // Evaluate the call
    let outcome = if let TopLevel::Expression(expr) = &parsed[1] {
        // Same here, although for the call it matters less if we don't define new things,
        // but we need the env to contain 'factorial'
        let mut env = std::mem::replace(
            &mut evaluator.env,
            rtfs::runtime::environment::Environment::new(),
        );
        let res = evaluator
            .evaluate_with_env(expr, &mut env)
            .expect("Call should succeed");
        evaluator.env = env;
        res
    } else {
        panic!("Expected expression");
    };

    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        _ => panic!("Unexpected outcome"),
    };

    assert_eq!(result, rtfs::runtime::values::Value::Integer(120));
}
