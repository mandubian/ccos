// Comprehensive tests for defn usage within let structures
// Tests both AST evaluator and IR runtime

use rtfs::runtime::evaluator::Evaluator;
use rtfs::runtime::ir_runtime::IrRuntime;
use rtfs::runtime::module_runtime::ModuleRegistry;
use rtfs::runtime::pure_host::create_pure_host;
use rtfs::*;
use std::sync::Arc;

#[test]
fn test_defn_in_let_ast_evaluator() {
    // Basic defn in let using AST evaluator
    let code = r#"
        (let [result 
              (let [(defn helper [x] (+ x 1))
                    value 5]
                (helper value))]
          result)
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
fn test_defn_in_let_ir_runtime() {
    // Basic defn in let using IR runtime
    let code = r#"
        (let [result 
              (let [(defn helper [x] (+ x 1))
                    value 5]
                (helper value))]
          result)
    "#;

    let parsed = parser::parse(code).expect("Should parse successfully");
    let module_registry = Arc::new(ModuleRegistry::new());
    let security_context = rtfs::runtime::security::RuntimeContext::pure();
    let host = create_pure_host();
    
    // Convert to IR
    let mut converter = rtfs::ir::converter::IrConverter::new();
    
    let expr = match &parsed[0] {
        TopLevel::Expression(expr) => expr,
        _ => panic!("Expected expression"),
    };
    
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
fn test_defn_in_let_with_closure() {
    // Test that defn in let captures outer variables (closure)
    let code = r#"
        (let [outer_var 10
              result 
              (let [(defn use_outer [] outer_var)]
                (use_outer))]
          result)
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
fn test_defn_in_let_with_multiple_functions() {
    // Test multiple defn definitions in same let
    let code = r#"
        (let [result 
              (let [(defn add [x y] (+ x y))
                    (defn multiply [x y] (* x y))
                    a 3
                    b 4]
                (+ (add a b) (multiply a b)))]
          result)
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
fn test_defn_in_let_with_recursion() {
    // Test recursive function defined in let
    let code = r#"
        (let [result 
              (let [(defn factorial [n] 
                      (if (= n 0) 1 (* n (factorial (- n 1)))))
                    value 3]
                (factorial value))]
          result)
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
fn test_defn_in_nested_let() {
    // Test defn in nested let structures
    let code = r#"
        (let [outer 100
              result 
              (let [middle 50
                    result 
                    (let [(defn compute [] (+ outer middle))
                          inner 25]
                      (+ (compute) inner))]
                result)]
          result)
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
fn test_defn_in_let_with_type_annotations() {
    // Test defn with type annotations in let
    let code = r#"
        (let [result 
              (let [(defn typed-add [x :int y :int] :int (+ x y))
                    a 7
                    b 8]
                (typed-add a b))]
          result)
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
fn test_defn_in_let_with_variadic_params() {
    // Test defn with variadic parameters in let
    let code = r#"
        (let [result 
              (let [(defn sum [x & more] 
                      (if (empty? more) x (+ x (apply sum more))))
                    nums [1 2 3 4]]
                (apply sum nums))]
          result)
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

    // 1 + 2 + 3 + 4 = 10
    assert_eq!(result, rtfs::runtime::values::Value::Integer(10));
}

#[test]
fn test_defn_in_let_ir_runtime_complex() {
    // Complex test with IR runtime - nested functions and closures
    let code = r#"
        (let [result 
              (let [(defn create-adder [x] 
                      (fn [y] (+ x y)))
                    add5 (create-adder 5)
                    add10 (create-adder 10)]
                (+ (add5 3) (add10 2)))]
          result)
    "#;

    let parsed = parser::parse(code).expect("Should parse successfully");
    let module_registry = Arc::new(ModuleRegistry::new());
    let security_context = rtfs::runtime::security::RuntimeContext::pure();
    let host = create_pure_host();
    let macro_expander = rtfs::compiler::expander::MacroExpander::default();
    
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
    
    let outcome = ir_runtime
        .execute_node(&ir_node, &module_registry)
        .expect("IR execution should succeed");

    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        _ => panic!("Unexpected outcome"),
    };

    // (5+3) + (10+2) = 8 + 12 = 20
    assert_eq!(result, rtfs::runtime::values::Value::Integer(20));
}