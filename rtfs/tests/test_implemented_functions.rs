use rtfs::ast::MapKey;
use rtfs::parser::parse_expression;
use rtfs::runtime::pure_host::create_pure_host;
use rtfs::runtime::evaluator::Evaluator;
use rtfs::runtime::module_runtime::ModuleRegistry;
use rtfs::runtime::stdlib::StandardLibrary;
use rtfs::runtime::values::Value;
use std::sync::Arc;

#[test]
fn test_factorial() {
    let mut env = StandardLibrary::create_global_environment();
    let module_registry = Arc::new(ModuleRegistry::new());
    let security_context = rtfs::runtime::security::RuntimeContext::pure();
    let host = create_pure_host();
    let evaluator = Evaluator::new(
        module_registry,
        rtfs::runtime::security::RuntimeContext::pure(),
        host,
    );

    // Test factorial of 0
    let expr = parse_expression("(factorial 0)").expect("Parse failed");
    let outcome = evaluator.evaluate(&expr).expect("Evaluation failed");
    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    };
    assert_eq!(result, Value::Integer(1));

    // Test factorial of 1
    let expr = parse_expression("(factorial 1)").expect("Parse failed");
    let outcome = evaluator.evaluate(&expr).expect("Evaluation failed");
    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    };
    assert_eq!(result, Value::Integer(1));

    // Test factorial of 5
    let expr = parse_expression("(factorial 5)").expect("Parse failed");
    let outcome = evaluator.evaluate(&expr).expect("Evaluation failed");
    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    };
    assert_eq!(result, Value::Integer(120));

    // Test factorial of 7
    let expr = parse_expression("(factorial 7)").expect("Parse failed");
    let outcome = evaluator.evaluate(&expr).expect("Evaluation failed");
    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    };
    assert_eq!(result, Value::Integer(5040));

    // Test error case - negative number
    let expr = parse_expression("(factorial -1)").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env);
    assert!(result.is_err());

    // Test error case - wrong type
    let expr = parse_expression("(factorial \"hello\")").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env);
    assert!(result.is_err());
}

#[test]
fn test_length_value() {
    let mut env = StandardLibrary::create_global_environment();
    let module_registry = Arc::new(ModuleRegistry::new());
    let security_context = rtfs::runtime::security::RuntimeContext::pure();
    let host = create_pure_host();
    let evaluator = Evaluator::new(
        module_registry,
        rtfs::runtime::security::RuntimeContext::pure(),
        host,
    );

    // Test length of empty vector
    let expr = parse_expression("(length [])").expect("Parse failed");
    let outcome = evaluator.evaluate(&expr).expect("Evaluation failed");
    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    };
    assert_eq!(result, Value::Integer(0));

    // Test length of vector
    let expr = parse_expression("(length [1 2 3 4])").expect("Parse failed");
    let outcome = evaluator.evaluate(&expr).expect("Evaluation failed");
    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    };
    assert_eq!(result, Value::Integer(4));

    // Test length of string
    let expr = parse_expression("(length \"hello\")").expect("Parse failed");
    let outcome = evaluator.evaluate(&expr).expect("Evaluation failed");
    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    };
    assert_eq!(result, Value::Integer(5));

    // Test length of empty string
    let expr = parse_expression("(length \"\")").expect("Parse failed");
    let outcome = evaluator.evaluate(&expr).expect("Evaluation failed");
    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    };
    assert_eq!(result, Value::Integer(0));

    // Test length of map
    let expr = parse_expression("(length {:a 1 :b 2})").expect("Parse failed");
    let outcome = evaluator.evaluate(&expr).expect("Evaluation failed");
    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    };
    assert_eq!(result, Value::Integer(2));

    // Test length of nil
    let expr = parse_expression("(length nil)").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env);
    assert!(result.is_err());

    // Test error case - unsupported type
    let expr = parse_expression("(length 42)").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env);
    assert!(result.is_err());
}


#[test]
fn test_json_functions() {
    let mut env = StandardLibrary::create_global_environment();
    let module_registry = Arc::new(ModuleRegistry::new());
    let security_context = rtfs::runtime::security::RuntimeContext::pure();
    let host = create_pure_host();
    let evaluator = Evaluator::new(
        module_registry,
        rtfs::runtime::security::RuntimeContext::pure(),
        host,
    );

    // Test parse-json with simple object
    let expr = parse_expression("(parse-json \"{\\\"name\\\": \\\"John\\\", \\\"age\\\": 30}\")")
        .expect("Parse failed");
    let outcome = evaluator.evaluate(&expr).expect("Evaluation failed");
    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    };
    if let Value::Map(map) = result {
        assert_eq!(map.len(), 2);
        assert_eq!(
            map.get(&MapKey::String("name".to_string())),
            Some(&Value::String("John".to_string()))
        );
        assert_eq!(
            map.get(&MapKey::String("age".to_string())),
            Some(&Value::Integer(30))
        );
    } else {
        panic!("Expected map result from parse-json");
    }

    // Test parse-json with array
    let expr = parse_expression("(parse-json \"[1, 2, 3]\")").expect("Parse failed");
    let outcome = evaluator.evaluate(&expr).expect("Evaluation failed");
    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    };
    if let Value::Vector(vec) = result {
        assert_eq!(vec.len(), 3);
        assert_eq!(vec[0], Value::Integer(1));
        assert_eq!(vec[1], Value::Integer(2));
        assert_eq!(vec[2], Value::Integer(3));
    } else {
        panic!("Expected vector result from parse-json");
    }

    // Test parse-json with primitives
    let expr = parse_expression("(parse-json \"true\")").expect("Parse failed");
    let outcome = evaluator.evaluate(&expr).expect("Evaluation failed");
    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    };
    assert_eq!(result, Value::Boolean(true));

    let expr = parse_expression("(parse-json \"null\")").expect("Parse failed");
    let outcome = evaluator.evaluate(&expr).expect("Evaluation failed");
    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    };
    match result {
        Value::Nil => {}
        _ => panic!("Unexpected result: {:?}", result),
    };

    // Test serialize-json with map
    let expr =
        parse_expression("(serialize-json {:name \"Alice\" :age 25})").expect("Parse failed");
    let outcome = evaluator.evaluate(&expr).expect("Evaluation failed");
    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    };
    if let Value::String(json_str) = result {
        // Should be valid JSON containing the key-value pairs
        assert!(json_str.contains("name"));
        assert!(json_str.contains("Alice"));
        assert!(json_str.contains("age"));
        assert!(json_str.contains("25"));
    } else {
        panic!("Expected string result from serialize-json");
    }

    // Test serialize-json with vector
    let expr = parse_expression("(serialize-json [1 2 3])").expect("Parse failed");
    let outcome = evaluator.evaluate(&expr).expect("Evaluation failed");
    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    };
    if let Value::String(json_str) = result {
        assert_eq!(json_str, "[1,2,3]");
    } else {
        panic!("Expected string result from serialize-json");
    }

    // Test error case - invalid JSON
    let expr = parse_expression("(parse-json \"invalid json\")").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env);
    assert!(result.is_err());

    // Test error case - wrong type for parse-json
    let expr = parse_expression("(parse-json 123)").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env);
    assert!(result.is_err());
}



#[test]
fn test_map_filter_functions() {
    let env = StandardLibrary::create_global_environment();
    let module_registry = Arc::new(ModuleRegistry::new());
    let security_context = rtfs::runtime::security::RuntimeContext::pure();
    let host = create_pure_host();
    let evaluator = Evaluator::new(
        module_registry,
        rtfs::runtime::security::RuntimeContext::pure(),
        host,
    );

    // Test map function
    let expr = parse_expression("(map (fn [x] (* x 2)) [1 2 3])").expect("Parse failed");
    let outcome = evaluator.evaluate(&expr).expect("Evaluation failed");
    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    };
    if let Value::Vector(vec) = result {
        assert_eq!(vec.len(), 3);
        assert_eq!(vec[0], Value::Integer(2));
        assert_eq!(vec[1], Value::Integer(4));
        assert_eq!(vec[2], Value::Integer(6));
    } else {
        panic!("Expected vector result from map");
    }

    // Test filter function
    let expr = parse_expression("(filter (fn [x] (> x 2)) [1 2 3 4 5])").expect("Parse failed");
    let outcome = evaluator.evaluate(&expr).expect("Evaluation failed");
    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    };
    if let Value::Vector(vec) = result {
        assert_eq!(vec.len(), 3);
        assert_eq!(vec[0], Value::Integer(3));
        assert_eq!(vec[1], Value::Integer(4));
        assert_eq!(vec[2], Value::Integer(5));
    } else {
        panic!("Expected vector result from filter");
    }

    // Test filter with boolean filter
    let expr = parse_expression("(filter (fn [x] (= x 3)) [1 2 3 4 3])").expect("Parse failed");
    let outcome = evaluator.evaluate(&expr).expect("Evaluation failed");
    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    };
    if let Value::Vector(vec) = result {
        assert_eq!(vec.len(), 2);
        assert_eq!(vec[0], Value::Integer(3));
        assert_eq!(vec[1], Value::Integer(3));
    } else {
        panic!("Expected vector result from filter");
    }
}

#[test]
fn test_reduce_function() {
    let mut env = StandardLibrary::create_global_environment();
    let module_registry = Arc::new(ModuleRegistry::new());
    let security_context = rtfs::runtime::security::RuntimeContext::pure();
    let host = create_pure_host();
    let evaluator = Evaluator::new(
        module_registry,
        rtfs::runtime::security::RuntimeContext::pure(),
        host,
    );

    // Test reduce with initial value
    let expr = parse_expression("(reduce + 0 [1 2 3])").expect("Parse failed");
    let outcome = evaluator.evaluate(&expr).expect("Evaluation failed");
    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    };
    assert_eq!(result, Value::Integer(6));

    // Test reduce without initial value
    let expr = parse_expression("(reduce + [1 2 3])").expect("Parse failed");
    let outcome = evaluator.evaluate(&expr).expect("Evaluation failed");
    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    };
    assert_eq!(result, Value::Integer(6));

    // Test reduce with empty collection and initial value
    let expr = parse_expression("(reduce + 42 [])").expect("Parse failed");
    let outcome = evaluator.evaluate(&expr).expect("Evaluation failed");
    let result = match outcome {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            panic!("Unexpected host call in pure test");
        }
    };
    assert_eq!(result, Value::Integer(42));

    // Test error case - empty collection without initial value
    let expr = parse_expression("(reduce + [])").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env);
    assert!(result.is_err());

    // Test error case - wrong arity
    let expr = parse_expression("(reduce)").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env);
    assert!(result.is_err());
}
