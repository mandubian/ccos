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

#[test]
fn test_match_literal_integer() {
    let code = r#"
        (match 5
          5 100
          _ 0)
    "#;

    let parsed = parse(code).expect("Should parse");
    let expr = if let rtfs::ast::TopLevel::Expression(e) = &parsed[0] {
        e.clone()
    } else {
        panic!("Expected expression")
    };

    let mut eval = create_test_evaluator();
    let outcome = eval.evaluate(&expr).expect("Should evaluate");

    match outcome {
        ExecutionOutcome::Complete(Value::Integer(100)) => (),
        other => panic!("Expected 100, got {:?}", other),
    }
}

#[test]
fn test_match_literal_string() {
    let code = r#"
        (match "hello"
          "hello" true
          _ false)
    "#;

    let parsed = parse(code).expect("Should parse");
    let expr = if let rtfs::ast::TopLevel::Expression(e) = &parsed[0] {
        e.clone()
    } else {
        panic!("Expected expression")
    };
    let mut eval = create_test_evaluator();
    let outcome = eval.evaluate(&expr).expect("Should evaluate");

    match outcome {
        ExecutionOutcome::Complete(Value::Boolean(true)) => (),
        other => panic!("Expected true, got {:?}", other),
    }
}

#[test]
fn test_match_wildcard() {
    let code = r#"
        (match 42
          _ "matched")
    "#;

    let parsed = parse(code).expect("Should parse");
    let expr = if let rtfs::ast::TopLevel::Expression(e) = &parsed[0] {
        e.clone()
    } else {
        panic!("Expected expression")
    };
    let mut eval = create_test_evaluator();
    let outcome = eval.evaluate(&expr).expect("Should evaluate");

    match outcome {
        ExecutionOutcome::Complete(Value::String(s)) if s == "matched" => (),
        other => panic!("Expected 'matched', got {:?}", other),
    }
}

#[test]
fn test_match_variable_binding() {
    let code = r#"
        (match 42
          x (* x 2))
    "#;

    let parsed = parse(code).expect("Should parse");
    let expr = if let rtfs::ast::TopLevel::Expression(e) = &parsed[0] {
        e.clone()
    } else {
        panic!("Expected expression")
    };
    let mut eval = create_test_evaluator();
    let outcome = eval.evaluate(&expr).expect("Should evaluate");

    match outcome {
        ExecutionOutcome::Complete(Value::Integer(84)) => (),
        other => panic!("Expected 84, got {:?}", other),
    }
}

#[test]
fn test_match_vector_pattern() {
    let code = r#"
        (match [1 2 3]
          [a b c] (+ (+ a b) c))
    "#;

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
fn test_match_map_pattern() {
    let code = r#"
        (match {:a 1 :b 2}
          {:a x :b y} (+ x y))
    "#;

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

#[test]
fn test_match_guard_condition() {
    let code = r#"
        (match 10
          x when (> x 5) "large"
          x when (<= x 5) "small")
    "#;

    let parsed = parse(code).expect("Should parse");
    let expr = if let rtfs::ast::TopLevel::Expression(e) = &parsed[0] {
        e.clone()
    } else {
        panic!("Expected expression")
    };
    let mut eval = create_test_evaluator();
    let outcome = eval.evaluate(&expr).expect("Should evaluate");

    match outcome {
        ExecutionOutcome::Complete(Value::String(s)) if s == "large" => (),
        other => panic!("Expected 'large', got {:?}", other),
    }
}

#[test]
fn test_match_no_pattern_found() {
    let code = r#"
        (match 5
          10 "wrong")
    "#;

    let parsed = parse(code).expect("Should parse");
    let expr = if let rtfs::ast::TopLevel::Expression(e) = &parsed[0] {
        e.clone()
    } else {
        panic!("Expected expression")
    };
    let mut eval = create_test_evaluator();
    let outcome = eval.evaluate(&expr);

    match outcome {
        Err(_) => (), // Expected error
        Ok(other) => panic!("Expected error, got {:?}", other),
    }
}

#[test]
fn test_match_nested_pattern() {
    let code = r#"
        (match [[1 2] [3 4]]
          [[a b] [c d]] (+ a b c d))
    "#;

    let parsed = parse(code).expect("Should parse");
    let expr = if let rtfs::ast::TopLevel::Expression(e) = &parsed[0] {
        e.clone()
    } else {
        panic!("Expected expression")
    };
    let mut eval = create_test_evaluator();
    let outcome = eval.evaluate(&expr).expect("Should evaluate");

    match outcome {
        ExecutionOutcome::Complete(Value::Integer(10)) => (),
        other => panic!("Expected 10, got {:?}", other),
    }
}
