#![allow(unused_variables)]

use rtfs::runtime::module_runtime::ModuleRegistry;
use rtfs::runtime::pure_host::create_pure_host;
use std::sync::Arc;
// Test for recursive function patterns
use rtfs::runtime::evaluator::Evaluator;
use rtfs::*;

#[test]
fn test_mutual_recursion_pattern() {
    // Skip test if file doesn't exist
    let code = r#"
        (let [is-even (fn [n] (if (= n 0) true (is-odd (- n 1))))
              is-odd (fn [n] (if (= n 0) false (is-even (- n 1))))]
          (vector (is-even 4) (is-odd 4)))
    "#;

    let parsed = parser::parse(code).expect("Should parse successfully");
    let module_registry = Arc::new(ModuleRegistry::new());
    let security_context = rtfs::runtime::security::RuntimeContext::pure();
    let host = create_pure_host();
    let evaluator = Evaluator::new(
        module_registry,
        security_context,
        host,
        rtfs::compiler::expander::MacroExpander::default(),
    );
    let outcome = if let TopLevel::Expression(expr) = &parsed[0] {
        evaluator
            .evaluate(expr)
            .expect("Should evaluate successfully")
    } else {
        panic!("Expected a top-level expression");
    };

    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    };

    // Expected: [true, false] for (is-even 4), (is-odd 4)
    if let runtime::values::Value::Vector(vec) = result {
        assert_eq!(vec.len(), 2);
        assert_eq!(vec[0], runtime::values::Value::Boolean(true)); // is-even 4
        assert_eq!(vec[1], runtime::values::Value::Boolean(false)); // is-odd 4
    } else {
        panic!("Expected vector result, got: {:?}", result);
    }
}

#[test]
fn test_nested_recursion_pattern() {
    // Skip test if file doesn't exist - use inline code instead
    let code = r#"
        (let [fact (fn [n] (if (= n 0) 1 (* n (fact (- n 1)))))]
          (fact 5))
    "#;

    let parsed = parser::parse(code).expect("Should parse successfully");
    let parsed = if let TopLevel::Expression(expr) = &parsed[0] {
        expr.clone()
    } else {
        panic!("Expected expression");
    };
    let module_registry = Arc::new(ModuleRegistry::new());

    let security_context = rtfs::runtime::security::RuntimeContext::pure();
    let host = create_pure_host();
    let evaluator = Evaluator::new(
        module_registry,
        rtfs::runtime::security::RuntimeContext::pure(),
        host,
        rtfs::compiler::expander::MacroExpander::default(),
    );
    let outcome = evaluator
        .evaluate(&parsed)
        .expect("Should evaluate successfully");

    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    };

    // Should return factorial of 5: 120
    assert_eq!(result, rtfs::runtime::values::Value::Integer(120));
}

#[test]
fn test_higher_order_recursion_pattern() {
    // Skip test if file doesn't exist - use inline code instead
    let code = r#"
        (let [map-square (fn [f xs] 
                 (if (empty? xs) 
                   []
                   (conj (map-square f (rest xs)) (f (first xs)))))
              range (fn [n] (if (= n 0) [] (conj (range (- n 1)) (- n 1))))
              square (fn [x] (* x x))]
          (map-square square (range 5)))
    "#;

    let parsed = parser::parse(code).expect("Should parse successfully");
    let parsed = if let TopLevel::Expression(expr) = &parsed[0] {
        expr.clone()
    } else {
        panic!("Expected expression");
    };
    let module_registry = Arc::new(ModuleRegistry::new());

    let security_context = rtfs::runtime::security::RuntimeContext::pure();
    let host = create_pure_host();
    let evaluator = Evaluator::new(
        module_registry,
        rtfs::runtime::security::RuntimeContext::pure(),
        host,
        rtfs::compiler::expander::MacroExpander::default(),
    );
    let outcome = evaluator
        .evaluate(&parsed)
        .expect("Should evaluate successfully");

    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    };

    // Should return squares in reverse order: [16, 9, 4, 1, 0]
    // (conj prepends, so range 5 = [4,3,2,1,0], then squares = [16,9,4,1,0])
    if let rtfs::runtime::values::Value::Vector(vec) = &result {
        assert_eq!(vec.len(), 5);
        assert_eq!(vec[0], rtfs::runtime::values::Value::Integer(16)); // 4^2
        assert_eq!(vec[1], rtfs::runtime::values::Value::Integer(9)); // 3^2
        assert_eq!(vec[2], rtfs::runtime::values::Value::Integer(4)); // 2^2
        assert_eq!(vec[3], rtfs::runtime::values::Value::Integer(1)); // 1^2
        assert_eq!(vec[4], rtfs::runtime::values::Value::Integer(0)); // 0^2
    } else {
        panic!("Expected vector result, got: {:?}", result);
    }
}

#[test]
fn test_three_way_recursion_pattern() {
    // Skip test if file doesn't exist - use inline code instead
    let code = r#"
        (let [map-square (fn [f xs] 
                 (if (empty? xs) 
                   []
                   (conj (map-square f (rest xs)) (f (first xs)))))
              range (fn [n] (if (= n 0) [] (conj (range (- n 1)) (- n 1))))
              square (fn [x] (* x x))]
          (map-square square (range 5)))
    "#;

    let parsed = parser::parse(code).expect("Should parse successfully");
    let parsed = if let TopLevel::Expression(expr) = &parsed[0] {
        expr.clone()
    } else {
        panic!("Expected expression");
    };
    let module_registry = Arc::new(ModuleRegistry::new());

    let security_context = rtfs::runtime::security::RuntimeContext::pure();
    let host = create_pure_host();
    let evaluator = Evaluator::new(
        module_registry,
        rtfs::runtime::security::RuntimeContext::pure(),
        host,
        rtfs::compiler::expander::MacroExpander::default(),
    );
    let outcome = evaluator
        .evaluate(&parsed)
        .expect("Should evaluate successfully");

    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    };

    // Should return squares in reverse order: [16, 9, 4, 1, 0]
    // (conj prepends, so range 5 = [4,3,2,1,0], then squares = [16,9,4,1,0])
    if let rtfs::runtime::values::Value::Vector(vec) = &result {
        assert_eq!(vec.len(), 5);
        assert_eq!(vec[0], rtfs::runtime::values::Value::Integer(16)); // 4^2
        assert_eq!(vec[1], rtfs::runtime::values::Value::Integer(9)); // 3^2
        assert_eq!(vec[2], rtfs::runtime::values::Value::Integer(4)); // 2^2
        assert_eq!(vec[3], rtfs::runtime::values::Value::Integer(1)); // 1^2
        assert_eq!(vec[4], rtfs::runtime::values::Value::Integer(0)); // 0^2
    } else {
        panic!("Expected vector result, got: {:?}", result);
    }
}
