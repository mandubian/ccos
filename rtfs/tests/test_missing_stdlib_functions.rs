use rtfs::parser::parse_expression;
use rtfs::runtime::evaluator::Evaluator;
use rtfs::runtime::module_runtime::ModuleRegistry;
use rtfs::runtime::pure_host::create_pure_host;
use rtfs::runtime::stdlib::StandardLibrary;
use rtfs::runtime::values::Value;
use std::sync::Arc;

#[test]
fn test_missing_stdlib_functions() {
    let env = StandardLibrary::create_global_environment();
    let module_registry = Arc::new(ModuleRegistry::new());
    let security_context = rtfs::runtime::security::RuntimeContext::pure();
    let host = create_pure_host();
    let evaluator = Evaluator::new(
        module_registry,
        security_context,
        host,
        rtfs::compiler::expander::MacroExpander::default(),
    );

    // Test empty?
    let expr = parse_expression("(empty? [])").expect("Parse failed");
    let result = evaluator.evaluate(&expr).expect("Evaluation failed");
    match result {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(Value::Boolean(true)) => {}
        _ => panic!("Expected Complete(Boolean(true)) result"),
    }

    let expr = parse_expression("(empty? [1 2 3])").expect("Parse failed");
    let result = evaluator.evaluate(&expr).expect("Evaluation failed");
    match result {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(Value::Boolean(false)) => {}
        _ => panic!("Expected Complete(Boolean(false)) result"),
    }

    // Test cons
    let expr = parse_expression("(cons 1 [2 3])").expect("Parse failed");
    let result = evaluator.evaluate(&expr).expect("Evaluation failed");
    match result {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(Value::Vector(v)) => {
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
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(Value::Integer(1)) => {}
        _ => panic!("Expected Complete(Integer(1)) result"),
    }

    let expr = parse_expression("(first [])").expect("Parse failed");
    let result = evaluator.evaluate(&expr).expect("Evaluation failed");
    match result {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(Value::Nil) => {}
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call")
        }
        _ => panic!("Unexpected result: {:?}", result),
    }

    // Test rest
    let expr = parse_expression("(rest [1 2 3])").expect("Parse failed");
    let result = evaluator.evaluate(&expr).expect("Evaluation failed");
    match result {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(Value::Vector(v)) => {
            assert_eq!(v.len(), 2);
            assert_eq!(v[0], Value::Integer(2));
            assert_eq!(v[1], Value::Integer(3));
        }
        _ => panic!("Expected Complete(Vector) result from rest"),
    }

    let expr = parse_expression("(rest [])").expect("Parse failed");
    let result = evaluator.evaluate(&expr).expect("Evaluation failed");
    match result {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(Value::Vector(v)) => {
            assert_eq!(v.len(), 0);
        }
        _ => panic!("Expected Complete(Vector) result from rest"),
    }

    println!("All missing stdlib functions are working correctly!");
}
