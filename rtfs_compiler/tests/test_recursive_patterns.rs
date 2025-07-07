use rtfs_compiler::runtime::module_runtime::ModuleRegistry;
use std::rc::Rc;
// Test for recursive function patterns
use rtfs_compiler::runtime::evaluator::Evaluator;
use rtfs_compiler::*;

#[test]
fn test_mutual_recursion_pattern() {
    let code = include_str!("rtfs_files/test_mutual_recursion.rtfs");

    let parsed = parser::parse(code).expect("Should parse successfully");
    let module_registry = Rc::new(ModuleRegistry::new());
    let evaluator = Evaluator::new(module_registry, std::sync::Arc::new(rtfs_compiler::ccos::delegation::StaticDelegationEngine::new(std::collections::HashMap::new())));
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
    let module_registry = Rc::new(ModuleRegistry::new());
    let evaluator = Evaluator::new(module_registry, std::sync::Arc::new(rtfs_compiler::ccos::delegation::StaticDelegationEngine::new(std::collections::HashMap::new())));
    let result = evaluator
        .evaluate(&parsed)
        .expect("Should evaluate successfully");

    // Should return a countdown vector [5, 4, 3, 2, 1]
    println!("Nested recursion result: {:?}", result);
}

#[test]
fn test_higher_order_recursion_pattern() {
    let code = include_str!("rtfs_files/test_higher_order_recursion.rtfs");

    let parsed = parser::parse_expression(code).expect("Should parse successfully");
    let module_registry = Rc::new(ModuleRegistry::new());
    let evaluator = Evaluator::new(module_registry, std::sync::Arc::new(rtfs_compiler::ccos::delegation::StaticDelegationEngine::new(std::collections::HashMap::new())));
    let result = evaluator
        .evaluate(&parsed)
        .expect("Should evaluate successfully");

    // Should return squares: [1, 4, 9, 16, 25]
    println!("Higher-order recursion result: {:?}", result);
}

#[test]
fn test_three_way_recursion_pattern() {
    let code = include_str!("rtfs_files/test_three_way_recursion.rtfs");

    let parsed = parser::parse_expression(code).expect("Should parse successfully");
    let module_registry = Rc::new(ModuleRegistry::new());
    let evaluator = Evaluator::new(module_registry, std::sync::Arc::new(rtfs_compiler::ccos::delegation::StaticDelegationEngine::new(std::collections::HashMap::new())));
    let result = evaluator
        .evaluate(&parsed)
        .expect("Should evaluate successfully");

    // Should return cycle results
    println!("Three-way recursion result: {:?}", result);
}
