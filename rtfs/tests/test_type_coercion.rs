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
    Evaluator::new(module_registry, security_context, host)
}

#[test]
fn test_int_plus_float() {
    let code = r#"(+ 1 2.5)"#;
    let parsed = parse(code).expect("Should parse");
    let expr = if let rtfs::ast::TopLevel::Expression(e) = &parsed[0] {
        e.clone()
    } else {
        panic!("Expected expression")
    };

    let mut eval = create_test_evaluator();
    let outcome = eval.evaluate(&expr).expect("Should evaluate");

    match outcome {
        ExecutionOutcome::Complete(Value::Float(f)) => assert!((f - 3.5).abs() < 1e-10),
        other => panic!("Expected 3.5, got {:?}", other),
    }
}

#[test]
fn test_float_plus_int() {
    let code = r#"(+ 2.5 1)"#;
    let parsed = parse(code).expect("Should parse");
    let expr = if let rtfs::ast::TopLevel::Expression(e) = &parsed[0] {
        e.clone()
    } else {
        panic!("Expected expression")
    };

    let mut eval = create_test_evaluator();
    let outcome = eval.evaluate(&expr).expect("Should evaluate");

    match outcome {
        ExecutionOutcome::Complete(Value::Float(f)) => assert!((f - 3.5).abs() < 1e-10),
        other => panic!("Expected 3.5, got {:?}", other),
    }
}

#[test]
fn test_int_multiplication_with_float() {
    let code = r#"(* 3 2.5)"#;
    let parsed = parse(code).expect("Should parse");
    let expr = if let rtfs::ast::TopLevel::Expression(e) = &parsed[0] {
        e.clone()
    } else {
        panic!("Expected expression")
    };

    let mut eval = create_test_evaluator();
    let outcome = eval.evaluate(&expr).expect("Should evaluate");

    match outcome {
        ExecutionOutcome::Complete(Value::Float(f)) => assert!((f - 7.5).abs() < 1e-10),
        other => panic!("Expected 7.5, got {:?}", other),
    }
}

#[test]
fn test_float_division_with_int() {
    let code = r#"(/ 7.5 3)"#;
    let parsed = parse(code).expect("Should parse");
    let expr = if let rtfs::ast::TopLevel::Expression(e) = &parsed[0] {
        e.clone()
    } else {
        panic!("Expected expression")
    };

    let mut eval = create_test_evaluator();
    let outcome = eval.evaluate(&expr).expect("Should evaluate");

    match outcome {
        ExecutionOutcome::Complete(Value::Float(f)) => assert!((f - 2.5).abs() < 1e-10),
        other => panic!("Expected 2.5, got {:?}", other),
    }
}

#[test]
fn test_all_int_returns_int() {
    let code = r#"(+ 1 2 3)"#;
    let parsed = parse(code).expect("Should parse");
    let expr = if let rtfs::ast::TopLevel::Expression(e) = &parsed[0] {
        e.clone()
    } else {
        panic!("Expected expression")
    };

    let mut eval = create_test_evaluator();
    let outcome = eval.evaluate(&expr).expect("Should evaluate");

    match outcome {
        ExecutionOutcome::Complete(Value::Integer(6)) => (),
        other => panic!("Expected 6, got {:?}", other),
    }
}

#[test]
fn test_all_float_returns_float() {
    let code = r#"(+ 1.5 2.5 3.5)"#;
    let parsed = parse(code).expect("Should parse");
    let expr = if let rtfs::ast::TopLevel::Expression(e) = &parsed[0] {
        e.clone()
    } else {
        panic!("Expected expression")
    };

    let mut eval = create_test_evaluator();
    let outcome = eval.evaluate(&expr).expect("Should evaluate");

    match outcome {
        ExecutionOutcome::Complete(Value::Float(f)) => assert!((f - 7.5).abs() < 1e-10),
        other => panic!("Expected 7.5, got {:?}", other),
    }
}

#[test]
fn test_mixed_arithmetic() {
    let code = r#"(- (* 2.5 4) 3)"#;
    let parsed = parse(code).expect("Should parse");
    let expr = if let rtfs::ast::TopLevel::Expression(e) = &parsed[0] {
        e.clone()
    } else {
        panic!("Expected expression")
    };

    let mut eval = create_test_evaluator();
    let outcome = eval.evaluate(&expr).expect("Should evaluate");

    match outcome {
        ExecutionOutcome::Complete(Value::Float(f)) => assert!((f - 7.0).abs() < 1e-10),
        other => panic!("Expected 7.0, got {:?}", other),
    }
}

#[test]
fn test_division_that_results_in_int() {
    let code = r#"(/ 6.0 2)"#;
    let parsed = parse(code).expect("Should parse");
    let expr = if let rtfs::ast::TopLevel::Expression(e) = &parsed[0] {
        e.clone()
    } else {
        panic!("Expected expression")
    };

    let mut eval = create_test_evaluator();
    let outcome = eval.evaluate(&expr).expect("Should evaluate");

    match outcome {
        ExecutionOutcome::Complete(Value::Integer(3)) => (),
        other => panic!("Expected 3, got {:?}", other),
    }
}
