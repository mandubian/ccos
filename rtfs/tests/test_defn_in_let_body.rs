#![allow(unused_mut)]
// Comprehensive tests for defn usage within let body (correct pattern)
// Tests both AST evaluator and IR runtime

use rtfs::runtime::evaluator::Evaluator;
use rtfs::runtime::ir_runtime::IrRuntime;
use rtfs::runtime::module_runtime::ModuleRegistry;
use rtfs::runtime::pure_host::create_pure_host;
use rtfs::*;
use std::sync::Arc;

#[test]
fn test_defn_in_let_body_ast_evaluator() {
    // Basic defn in let body using AST evaluator
    let code = r#"
        (let [value 5]
          (defn helper [x] (+ x 1))
          (helper value))
    "#;

    let parsed = parser::parse(code).expect("Should parse successfully");
    let module_registry = Arc::new(ModuleRegistry::new());
    let security_context = rtfs::runtime::security::RuntimeContext::pure();
    let host = create_pure_host();
    let macro_expander = rtfs::compiler::expander::MacroExpander::default();
    let mut evaluator = Evaluator::new(module_registry, security_context, host, macro_expander);

    // Evaluate the expression
    let outcome = if let TopLevel::Expression(expr) = &parsed[0] {
        evaluator.evaluate(expr).expect("Evaluation should succeed")
    } else {
        panic!("Expected expression");
    };

    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        _ => panic!("Unexpected outcome"),
    };

    assert_eq!(result, rtfs::runtime::values::Value::Integer(6));
}

#[test]
fn test_defn_in_let_body_ir_runtime() {
    // Basic defn in let body using IR runtime
    let code = r#"
        (let [value 5]
          (defn helper [x] (+ x 1))
          (helper value))
    "#;

    let parsed = parser::parse(code).expect("Should parse successfully");
    let module_registry = Arc::new(ModuleRegistry::new());
    let security_context = rtfs::runtime::security::RuntimeContext::pure();
    let host = create_pure_host();

    // Load stdlib module for IR runtime
    rtfs::runtime::stdlib::load_stdlib(&module_registry).expect("Should load stdlib");

    // Convert to IR
    let expr = match &parsed[0] {
        TopLevel::Expression(expr) => expr,
        _ => panic!("Expected expression"),
    };

    let mut converter = rtfs::ir::converter::IrConverter::new();

    let ir_node = converter
        .convert_expression(expr.clone())
        .expect("IR conversion should succeed");

    // Execute with IR runtime
    let mut ir_runtime = IrRuntime::new(host, security_context);
    let mut env = rtfs::runtime::environment::IrEnvironment::with_stdlib(&module_registry)
        .expect("Should create environment with stdlib");

    let outcome = ir_runtime
        .execute_node(&ir_node, &mut env, false, &module_registry)
        .expect("IR execution should succeed");

    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        _ => panic!("Unexpected outcome"),
    };

    assert_eq!(result, rtfs::runtime::values::Value::Integer(6));
}

#[test]
fn test_defn_in_let_body_with_closure() {
    // Test that defn in let body captures outer variables (closure)
    let code = r#"
        (let [outer_var 10]
          (defn use_outer [] outer_var)
          (use_outer))
    "#;

    let parsed = parser::parse(code).expect("Should parse successfully");
    let module_registry = Arc::new(ModuleRegistry::new());
    let security_context = rtfs::runtime::security::RuntimeContext::pure();
    let host = create_pure_host();
    let macro_expander = rtfs::compiler::expander::MacroExpander::default();
    let mut evaluator = Evaluator::new(module_registry, security_context, host, macro_expander);

    let outcome = if let TopLevel::Expression(expr) = &parsed[0] {
        evaluator.evaluate(expr).expect("Evaluation should succeed")
    } else {
        panic!("Expected expression");
    };

    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        _ => panic!("Unexpected outcome"),
    };

    assert_eq!(result, rtfs::runtime::values::Value::Integer(10));
}

#[test]
fn test_defn_in_let_body_with_multiple_functions() {
    // Test multiple defn definitions in same let body
    let code = r#"
        (let [a 3
              b 4]
          (defn add [x y] (+ x y))
          (defn multiply [x y] (* x y))
          (+ (add a b) (multiply a b)))
    "#;

    let parsed = parser::parse(code).expect("Should parse successfully");
    let module_registry = Arc::new(ModuleRegistry::new());
    let security_context = rtfs::runtime::security::RuntimeContext::pure();
    let host = create_pure_host();
    let macro_expander = rtfs::compiler::expander::MacroExpander::default();
    let mut evaluator = Evaluator::new(module_registry, security_context, host, macro_expander);

    let outcome = if let TopLevel::Expression(expr) = &parsed[0] {
        evaluator.evaluate(expr).expect("Evaluation should succeed")
    } else {
        panic!("Expected expression");
    };

    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        _ => panic!("Unexpected outcome"),
    };

    // 3+4 + 3*4 = 7 + 12 = 19
    assert_eq!(result, rtfs::runtime::values::Value::Integer(19));
}

#[test]
fn test_defn_in_let_body_with_recursion() {
    // Test recursive function defined in let body
    let code = r#"
        (let [value 3]
          (defn factorial [n] 
            (if (= n 0) 1 (* n (factorial (- n 1)))))
          (factorial value))
    "#;

    let parsed = parser::parse(code).expect("Should parse successfully");
    let module_registry = Arc::new(ModuleRegistry::new());
    let security_context = rtfs::runtime::security::RuntimeContext::pure();
    let host = create_pure_host();
    let macro_expander = rtfs::compiler::expander::MacroExpander::default();
    let mut evaluator = Evaluator::new(module_registry, security_context, host, macro_expander);

    let outcome = if let TopLevel::Expression(expr) = &parsed[0] {
        evaluator.evaluate(expr).expect("Evaluation should succeed")
    } else {
        panic!("Expected expression");
    };

    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        _ => panic!("Unexpected outcome"),
    };

    assert_eq!(result, rtfs::runtime::values::Value::Integer(6));
}

#[test]
fn test_defn_in_nested_let_body() {
    // Test defn in nested let body structures
    let code = r#"
        (let [outer 100]
          (let [middle 50]
            (defn compute [] (+ outer middle))
            (let [inner 25]
              (+ (compute) inner))))
    "#;

    let parsed = parser::parse(code).expect("Should parse successfully");
    let module_registry = Arc::new(ModuleRegistry::new());
    let security_context = rtfs::runtime::security::RuntimeContext::pure();
    let host = create_pure_host();
    let macro_expander = rtfs::compiler::expander::MacroExpander::default();
    let mut evaluator = Evaluator::new(module_registry, security_context, host, macro_expander);

    let outcome = if let TopLevel::Expression(expr) = &parsed[0] {
        evaluator.evaluate(expr).expect("Evaluation should succeed")
    } else {
        panic!("Expected expression");
    };

    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        _ => panic!("Unexpected outcome"),
    };

    // 100 + 50 + 25 = 175
    assert_eq!(result, rtfs::runtime::values::Value::Integer(175));
}

#[test]
fn test_defn_in_let_body_with_type_annotations() {
    // Test defn with type annotations in let body
    let code = r#"
        (let [a 7
              b 8]
          (defn typed-add [x :int y :int] :int (+ x y))
          (typed-add a b))
    "#;

    let parsed = parser::parse(code).expect("Should parse successfully");
    let module_registry = Arc::new(ModuleRegistry::new());
    let security_context = rtfs::runtime::security::RuntimeContext::pure();
    let host = create_pure_host();
    let macro_expander = rtfs::compiler::expander::MacroExpander::default();
    let mut evaluator = Evaluator::new(module_registry, security_context, host, macro_expander);

    let outcome = if let TopLevel::Expression(expr) = &parsed[0] {
        evaluator.evaluate(expr).expect("Evaluation should succeed")
    } else {
        panic!("Expected expression");
    };

    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        _ => panic!("Unexpected outcome"),
    };

    assert_eq!(result, rtfs::runtime::values::Value::Integer(15));
}

#[test]
fn test_defn_in_let_body_with_variadic_params() {
    // Test defn with variadic parameters in let body
    let code = r#"
        (let []
          (defn simple-variadic [x & more] 
            (count more))
          (simple-variadic 1 2 3))
    "#;

    let parsed = parser::parse(code).expect("Should parse successfully");
    let module_registry = Arc::new(ModuleRegistry::new());
    let security_context = rtfs::runtime::security::RuntimeContext::pure();
    let host = create_pure_host();
    let macro_expander = rtfs::compiler::expander::MacroExpander::default();
    let mut evaluator = Evaluator::new(module_registry, security_context, host, macro_expander);

    let outcome = if let TopLevel::Expression(expr) = &parsed[0] {
        evaluator.evaluate(expr).expect("Evaluation should succeed")
    } else {
        panic!("Expected expression");
    };

    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        _ => panic!("Unexpected outcome"),
    };

    // Should have 2 variadic args: 2, 3
    assert_eq!(result, rtfs::runtime::values::Value::Integer(2));
}

#[test]
fn test_defn_in_let_body_ir_runtime_complex() {
    // Complex test with IR runtime - nested functions and closures
    let code = r#"
        (let []
          (defn create-adder [x] 
            (fn [y] (+ x y)))
          (let [add5 (create-adder 5)
                add10 (create-adder 10)]
            (+ (add5 3) (add10 2))))
    "#;

    let parsed = parser::parse(code).expect("Should parse successfully");
    let module_registry = Arc::new(ModuleRegistry::new());
    let security_context = rtfs::runtime::security::RuntimeContext::pure();
    let host = create_pure_host();

    // Load stdlib module for IR runtime
    rtfs::runtime::stdlib::load_stdlib(&module_registry).expect("Should load stdlib");

    // Convert to IR
    let expr = match &parsed[0] {
        TopLevel::Expression(expr) => expr,
        _ => panic!("Expected expression"),
    };

    let mut converter = rtfs::ir::converter::IrConverter::new();

    let ir_node = converter
        .convert_expression(expr.clone())
        .expect("IR conversion should succeed");

    // Execute with IR runtime
    let mut ir_runtime = IrRuntime::new(host, security_context);
    let mut env = rtfs::runtime::environment::IrEnvironment::with_stdlib(&module_registry)
        .expect("Should create environment with stdlib");

    let outcome = ir_runtime
        .execute_node(&ir_node, &mut env, false, &module_registry)
        .expect("IR execution should succeed");

    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        _ => panic!("Unexpected outcome"),
    };

    // (5+3) + (10+2) = 8 + 12 = 20
    assert_eq!(result, rtfs::runtime::values::Value::Integer(20));
}

#[test]
fn test_meta_planner_pattern() {
    // Test the exact pattern used in meta-planner
    let code = r#"
        (let [goal "test goal"
              max-depth 3]
          (defn resolve-or-decompose [intent depth]
            (if (<= depth 0)
              {:resolved false :error "Max recursion depth reached" :intent intent}
              {:resolved true :intent intent :depth depth}))
          (let [root-intent {:description goal :id "root"}]
            (resolve-or-decompose root-intent max-depth)))
    "#;

    let parsed = parser::parse(code).expect("Should parse successfully");
    let module_registry = Arc::new(ModuleRegistry::new());
    let security_context = rtfs::runtime::security::RuntimeContext::pure();
    let host = create_pure_host();
    let macro_expander = rtfs::compiler::expander::MacroExpander::default();
    let mut evaluator = Evaluator::new(module_registry, security_context, host, macro_expander);

    let outcome = if let TopLevel::Expression(expr) = &parsed[0] {
        evaluator.evaluate(expr).expect("Evaluation should succeed")
    } else {
        panic!("Expected expression");
    };

    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        _ => panic!("Unexpected outcome"),
    };

    // Should return the resolved result
    assert!(matches!(result, rtfs::runtime::values::Value::Map(_)));
}

/// Test mutual recursion (is-even / is-odd) via IR runtime trampoline.
/// This validates that the trampoline handles mutually recursive functions without stack overflow.
#[test]
fn test_mutual_recursion_ir_runtime() {
    let code = r#"
        (let [is-even (fn [n] (if (= n 0) true (is-odd (- n 1))))
              is-odd  (fn [n] (if (= n 0) false (is-even (- n 1))))]
          (vector (is-even 4) (is-odd 4) (is-even 7) (is-odd 7)))
    "#;

    let parsed = parser::parse(code).expect("Should parse successfully");
    let module_registry = Arc::new(ModuleRegistry::new());
    let security_context = rtfs::runtime::security::RuntimeContext::pure();
    let host = create_pure_host();

    // Load stdlib module for IR runtime
    rtfs::runtime::stdlib::load_stdlib(&module_registry).expect("Should load stdlib");

    // Convert to IR
    let expr = match &parsed[0] {
        TopLevel::Expression(expr) => expr,
        _ => panic!("Expected expression"),
    };

    let mut converter = rtfs::ir::converter::IrConverter::new();

    let ir_node = converter
        .convert_expression(expr.clone())
        .expect("IR conversion should succeed");

    // Execute with IR runtime (trampoline-based, no stack overflow)
    let mut ir_runtime = IrRuntime::new(host, security_context);
    let mut env = rtfs::runtime::environment::IrEnvironment::with_stdlib(&module_registry)
        .expect("Should create environment with stdlib");

    let outcome = ir_runtime
        .execute_node(&ir_node, &mut env, false, &module_registry)
        .expect("IR execution should succeed");

    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        _ => panic!("Unexpected outcome"),
    };

    // Expected: [true, false, false, true] for (is-even 4), (is-odd 4), (is-even 7), (is-odd 7)
    if let rtfs::runtime::values::Value::Vector(vec) = result {
        assert_eq!(vec.len(), 4);
        assert_eq!(vec[0], rtfs::runtime::values::Value::Boolean(true)); // is-even 4
        assert_eq!(vec[1], rtfs::runtime::values::Value::Boolean(false)); // is-odd 4
        assert_eq!(vec[2], rtfs::runtime::values::Value::Boolean(false)); // is-even 7
        assert_eq!(vec[3], rtfs::runtime::values::Value::Boolean(true)); // is-odd 7
    } else {
        panic!("Expected vector result, got: {:?}", result);
    }
}
