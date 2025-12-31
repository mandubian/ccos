// Runtime evaluation tests for destructuring patterns
// Goal: ensure destructuring that parses correctly also evaluates correctly (no panics, correct bindings).

use rtfs::parser::parse;
use rtfs::runtime::evaluator::Evaluator;
use rtfs::runtime::execution_outcome::ExecutionOutcome;
use rtfs::runtime::module_runtime::ModuleRegistry;
use rtfs::runtime::pure_host::create_pure_host;
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::values::Value;
use std::sync::Arc;

fn create_test_evaluator() -> Evaluator {
    let module_registry = Arc::new(ModuleRegistry::new());
    let security_context = RuntimeContext::pure();
    let host = create_pure_host();
    Evaluator::new(
        module_registry,
        security_context,
        host,
        rtfs::compiler::expander::MacroExpander::default(),
    )
}

fn eval_first_expr(code: &str) -> ExecutionOutcome {
    let parsed = parse(code).expect("Should parse");
    let expr = match &parsed[0] {
        rtfs::ast::TopLevel::Expression(e) => e.clone(),
        other => panic!("Expected expression, got {:?}", other),
    };
    create_test_evaluator()
        .evaluate(&expr)
        .expect("Should evaluate")
}

#[test]
fn test_deeply_nested_vector_destructuring_in_let() {
    let code = r#"
        (let [[[a b] [c d]] [[1 2] [3 4]]]
          (+ a b c d))
    "#;

    match eval_first_expr(code) {
        ExecutionOutcome::Complete(Value::Integer(10)) => {}
        other => panic!("Expected 10, got {:?}", other),
    }
}

#[test]
fn test_map_destructuring_keys_in_let() {
    let code = r#"
        (let [{:keys [name age]} {:name "John" :age 30}]
          (+ age 1))
    "#;

    match eval_first_expr(code) {
        ExecutionOutcome::Complete(Value::Integer(31)) => {}
        other => panic!("Expected 31, got {:?}", other),
    }
}

#[test]
fn test_vector_destructuring_rest_binding_in_let() {
    let code = r#"
        (let [[x & rest] [1 2 3 4]]
          (count rest))
    "#;

    match eval_first_expr(code) {
        ExecutionOutcome::Complete(Value::Integer(3)) => {}
        other => panic!("Expected 3, got {:?}", other),
    }
}


