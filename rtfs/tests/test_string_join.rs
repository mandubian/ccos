use rtfs::parser::parse_expression;
use rtfs::runtime::evaluator::Evaluator;
use rtfs::runtime::module_runtime::ModuleRegistry;
use rtfs::runtime::pure_host::create_pure_host;
use rtfs::runtime::values::Value;
use std::sync::Arc;

#[test]
fn test_join_vector_of_strings() {
    let module_registry = Arc::new(ModuleRegistry::new());
    let security_context = rtfs::runtime::security::RuntimeContext::pure();
    let host = create_pure_host();
    let evaluator = Evaluator::new(
        module_registry,
        security_context,
        host,
        rtfs::compiler::expander::MacroExpander::default(),
    );

    let expr = parse_expression("(join \" \" [\"a\" \"b\" \"c\"])"
    ).expect("Parse failed");

    let result = evaluator.evaluate(&expr).expect("Evaluation failed");
    match result {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(Value::String(s)) => {
            assert_eq!(s, "a b c");
        }
        _ => panic!("Expected Complete(String) result"),
    }
}

#[test]
fn test_string_join_alias() {
    let module_registry = Arc::new(ModuleRegistry::new());
    let security_context = rtfs::runtime::security::RuntimeContext::pure();
    let host = create_pure_host();
    let evaluator = Evaluator::new(
        module_registry,
        security_context,
        host,
        rtfs::compiler::expander::MacroExpander::default(),
    );

    let expr = parse_expression("(string-join \",\" [1 2 3])").expect("Parse failed");
    let result = evaluator.evaluate(&expr).expect("Evaluation failed");

    match result {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(Value::String(s)) => {
            assert_eq!(s, "1,2,3");
        }
        _ => panic!("Expected Complete(String) result"),
    }
}
