// Simple test to verify let works
use rtfs::runtime::evaluator::Evaluator;
use rtfs::runtime::module_runtime::ModuleRegistry;
use rtfs::runtime::pure_host::create_pure_host;
use rtfs::*;
use std::sync::Arc;

#[test]
fn test_simple_let() {
    let code = r#"(let [x 5 y 3] (+ x y))"#;

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

    assert_eq!(result, rtfs::runtime::values::Value::Integer(8));
}