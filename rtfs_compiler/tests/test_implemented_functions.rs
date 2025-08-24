use rtfs_compiler::runtime::stdlib::StandardLibrary;
use rtfs_compiler::runtime::evaluator::Evaluator;
use rtfs_compiler::runtime::module_runtime::ModuleRegistry;
use rtfs_compiler::parser::parse_expression;
use rtfs_compiler::runtime::values::Value;
use rtfs_compiler::ast::MapKey;
use std::sync::Arc;

#[test]
fn test_factorial() {
    let mut env = StandardLibrary::create_global_environment();
    let module_registry = Arc::new(ModuleRegistry::new());
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(rtfs_compiler::runtime::capability_registry::CapabilityRegistry::new()));
    let capability_marketplace = std::sync::Arc::new(rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace::new(registry));
    let causal_chain = std::sync::Arc::new(std::sync::Mutex::new(rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap()));
    let security_context = rtfs_compiler::runtime::security::RuntimeContext::pure();
    let host = std::sync::Arc::new(rtfs_compiler::runtime::host::RuntimeHost::new(
        causal_chain,
        capability_marketplace,
        security_context.clone(),
    ));
    let evaluator = Evaluator::new(module_registry, Arc::new(rtfs_compiler::ccos::delegation::StaticDelegationEngine::new(std::collections::HashMap::new())), rtfs_compiler::runtime::security::RuntimeContext::pure(), host);

    // Test factorial of 0
    let expr = parse_expression("(factorial 0)").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    assert_eq!(result, Value::Integer(1));

    // Test factorial of 1
    let expr = parse_expression("(factorial 1)").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    assert_eq!(result, Value::Integer(1));

    // Test factorial of 5
    let expr = parse_expression("(factorial 5)").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    assert_eq!(result, Value::Integer(120));

    // Test factorial of 7
    let expr = parse_expression("(factorial 7)").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
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
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(rtfs_compiler::runtime::capability_registry::CapabilityRegistry::new()));
    let capability_marketplace = std::sync::Arc::new(rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace::new(registry));
    let causal_chain = std::sync::Arc::new(std::sync::Mutex::new(rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap()));
    let security_context = rtfs_compiler::runtime::security::RuntimeContext::pure();
    let host = std::sync::Arc::new(rtfs_compiler::runtime::host::RuntimeHost::new(
        causal_chain,
        capability_marketplace,
        security_context.clone(),
    ));
    let evaluator = Evaluator::new(module_registry, Arc::new(rtfs_compiler::ccos::delegation::StaticDelegationEngine::new(std::collections::HashMap::new())), rtfs_compiler::runtime::security::RuntimeContext::pure(), host);

    // Test length of empty vector
    let expr = parse_expression("(length [])").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    assert_eq!(result, Value::Integer(0));

    // Test length of vector
    let expr = parse_expression("(length [1 2 3 4])").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    assert_eq!(result, Value::Integer(4));

    // Test length of string
    let expr = parse_expression("(length \"hello\")").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    assert_eq!(result, Value::Integer(5));

    // Test length of empty string
    let expr = parse_expression("(length \"\")").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    assert_eq!(result, Value::Integer(0));

    // Test length of map
    let expr = parse_expression("(length {:a 1 :b 2})").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
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
#[ignore]
fn test_current_time() {
    let mut env = StandardLibrary::create_global_environment();
    let module_registry = Arc::new(ModuleRegistry::new());
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(rtfs_compiler::runtime::capability_registry::CapabilityRegistry::new()));
    let capability_marketplace = std::sync::Arc::new(rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace::new(registry));
    let causal_chain = std::sync::Arc::new(std::sync::Mutex::new(rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap()));
    let security_context = rtfs_compiler::runtime::security::RuntimeContext::pure();
    let host = std::sync::Arc::new(rtfs_compiler::runtime::host::RuntimeHost::new(
        causal_chain,
        capability_marketplace,
        security_context.clone(),
    ));
    let evaluator = Evaluator::new(module_registry, Arc::new(rtfs_compiler::ccos::delegation::StaticDelegationEngine::new(std::collections::HashMap::new())), rtfs_compiler::runtime::security::RuntimeContext::pure(), host);

    // Test current-time returns a string
    let expr = parse_expression("(current-time)").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    if let Value::String(time_str) = result {
        // Should be a valid RFC3339 timestamp
        assert!(time_str.contains("T"));
        assert!(time_str.contains("Z") || time_str.contains("+") || time_str.contains("-"));
    } else {
        panic!("Expected string result from current-time");
    }

    // Test error case - wrong arity
    let expr = parse_expression("(current-time 123)").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env);
    assert!(result.is_err());
}

#[test]
#[ignore]
fn test_json_functions() {
    let mut env = StandardLibrary::create_global_environment();
    let module_registry = Arc::new(ModuleRegistry::new());
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(rtfs_compiler::runtime::capability_registry::CapabilityRegistry::new()));
    let capability_marketplace = std::sync::Arc::new(rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace::new(registry));
    let causal_chain = std::sync::Arc::new(std::sync::Mutex::new(rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap()));
    let security_context = rtfs_compiler::runtime::security::RuntimeContext::pure();
    let host = std::sync::Arc::new(rtfs_compiler::runtime::host::RuntimeHost::new(
        causal_chain,
        capability_marketplace,
        security_context.clone(),
    ));
    let evaluator = Evaluator::new(module_registry, Arc::new(rtfs_compiler::ccos::delegation::StaticDelegationEngine::new(std::collections::HashMap::new())), rtfs_compiler::runtime::security::RuntimeContext::pure(), host);

    // Test parse-json with simple object
    let expr = parse_expression("(parse-json \"{\\\"name\\\": \\\"John\\\", \\\"age\\\": 30}\")").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    if let Value::Map(map) = result {
        assert_eq!(map.len(), 2);
        assert_eq!(map.get(&MapKey::String("name".to_string())), Some(&Value::String("John".to_string())));
        assert_eq!(map.get(&MapKey::String("age".to_string())), Some(&Value::Integer(30)));
    } else {
        panic!("Expected map result from parse-json");
    }

    // Test parse-json with array
    let expr = parse_expression("(parse-json \"[1, 2, 3]\")").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
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
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    assert_eq!(result, Value::Boolean(true));

    let expr = parse_expression("(parse-json \"null\")").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    assert_eq!(result, Value::Nil);

    // Test serialize-json with map
    let expr = parse_expression("(serialize-json {:name \"Alice\" :age 25})").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
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
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
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
fn test_file_exists() {
    let mut env = StandardLibrary::create_global_environment();
    let module_registry = Arc::new(ModuleRegistry::new());
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(rtfs_compiler::runtime::capability_registry::CapabilityRegistry::new()));
    let capability_marketplace = std::sync::Arc::new(rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace::new(registry));
    let causal_chain = std::sync::Arc::new(std::sync::Mutex::new(rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap()));
    let security_context = rtfs_compiler::runtime::security::RuntimeContext::pure();
    let host = std::sync::Arc::new(rtfs_compiler::runtime::host::RuntimeHost::new(
        causal_chain,
        capability_marketplace,
        security_context.clone(),
    ));
    let evaluator = Evaluator::new(module_registry, Arc::new(rtfs_compiler::ccos::delegation::StaticDelegationEngine::new(std::collections::HashMap::new())), rtfs_compiler::runtime::security::RuntimeContext::pure(), host);

    // Test file-exists? with existing file (Cargo.toml should exist)
    let expr = parse_expression("(file-exists? \"Cargo.toml\")").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    assert_eq!(result, Value::Boolean(true));

    // Test file-exists? with non-existing file
    let expr = parse_expression("(file-exists? \"nonexistent_file_12345.txt\")").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    assert_eq!(result, Value::Boolean(false));

    // Test error case - wrong type
    let expr = parse_expression("(file-exists? 123)").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env);
    assert!(result.is_err());
}

#[test]
fn test_get_env() {
    let mut env = StandardLibrary::create_global_environment();
    let module_registry = Arc::new(ModuleRegistry::new());
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(rtfs_compiler::runtime::capability_registry::CapabilityRegistry::new()));
    let capability_marketplace = std::sync::Arc::new(rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace::new(registry));
    let causal_chain = std::sync::Arc::new(std::sync::Mutex::new(rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap()));
    let security_context = rtfs_compiler::runtime::security::RuntimeContext::pure();
    let host = std::sync::Arc::new(rtfs_compiler::runtime::host::RuntimeHost::new(
        causal_chain,
        capability_marketplace,
        security_context.clone(),
    ));
    let evaluator = Evaluator::new(module_registry, Arc::new(rtfs_compiler::ccos::delegation::StaticDelegationEngine::new(std::collections::HashMap::new())), rtfs_compiler::runtime::security::RuntimeContext::pure(), host);

    // Set an environment variable for testing
    std::env::set_var("RTFS_TEST_VAR", "test_value");

    // Test get-env with existing variable
    let expr = parse_expression("(get-env \"RTFS_TEST_VAR\")").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    assert_eq!(result, Value::String("test_value".to_string()));

    // Test get-env with non-existing variable
    let expr = parse_expression("(get-env \"NON_EXISTENT_VAR_12345\")").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    assert_eq!(result, Value::Nil);

    // Test error case - wrong type
    let expr = parse_expression("(get-env 123)").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env);
    assert!(result.is_err());

    // Clean up
    std::env::remove_var("RTFS_TEST_VAR");
}

#[test]
fn test_log_function() {
    let mut env = StandardLibrary::create_global_environment();
    let module_registry = Arc::new(ModuleRegistry::new());
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(rtfs_compiler::runtime::capability_registry::CapabilityRegistry::new()));
    let capability_marketplace = std::sync::Arc::new(rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace::new(registry));
    let causal_chain = std::sync::Arc::new(std::sync::Mutex::new(rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap()));
    let security_context = rtfs_compiler::runtime::security::RuntimeContext::pure();
    let host = std::sync::Arc::new(rtfs_compiler::runtime::host::RuntimeHost::new(
        causal_chain,
        capability_marketplace,
        security_context.clone(),
    ));
    let evaluator = Evaluator::new(module_registry, Arc::new(rtfs_compiler::ccos::delegation::StaticDelegationEngine::new(std::collections::HashMap::new())), rtfs_compiler::runtime::security::RuntimeContext::pure(), host);

    // Test log function (just ensure it doesn't crash)
    let expr = parse_expression("(log \"test message\")").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    assert_eq!(result, Value::Nil);

    // Test log with multiple arguments
    let expr = parse_expression("(log \"Hello\" \"world\" 123)").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    assert_eq!(result, Value::Nil);

    // Test log with no arguments
    let expr = parse_expression("(log)").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    assert_eq!(result, Value::Nil);
}

#[test]
#[ignore]
fn test_agent_functions() {
    let mut env = StandardLibrary::create_global_environment();
    let module_registry = Arc::new(ModuleRegistry::new());
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(rtfs_compiler::runtime::capability_registry::CapabilityRegistry::new()));
    let capability_marketplace = std::sync::Arc::new(rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace::new(registry));
    let causal_chain = std::sync::Arc::new(std::sync::Mutex::new(rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap()));
    let security_context = rtfs_compiler::runtime::security::RuntimeContext::pure();
    let host = std::sync::Arc::new(rtfs_compiler::runtime::host::RuntimeHost::new(
        causal_chain,
        capability_marketplace,
        security_context.clone(),
    ));
    let evaluator = Evaluator::new(module_registry, Arc::new(rtfs_compiler::ccos::delegation::StaticDelegationEngine::new(std::collections::HashMap::new())), rtfs_compiler::runtime::security::RuntimeContext::pure(), host);

    // Test discover-agents
    let expr = parse_expression("(discover-agents)").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    assert_eq!(result, Value::Vector(vec![]));

    // Test discover-agents with wrong arity
    let expr = parse_expression("(discover-agents 123)").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env);
    assert!(result.is_err());

    // Test task-coordination
    let expr = parse_expression("(task-coordination \"task1\")").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    if let Value::Map(map) = result {
        assert!(map.contains_key(&MapKey::Keyword(rtfs_compiler::ast::Keyword("status".to_string()))));
        assert!(map.contains_key(&MapKey::Keyword(rtfs_compiler::ast::Keyword("task-count".to_string()))));
    } else {
        panic!("Expected map result from task-coordination");
    }

    // Test discover-and-assess-agents
    let expr = parse_expression("(discover-and-assess-agents)").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    if let Value::Map(map) = result {
        assert!(map.contains_key(&MapKey::Keyword(rtfs_compiler::ast::Keyword("discovered".to_string()))));
        assert!(map.contains_key(&MapKey::Keyword(rtfs_compiler::ast::Keyword("assessed".to_string()))));
        assert!(map.contains_key(&MapKey::Keyword(rtfs_compiler::ast::Keyword("timestamp".to_string()))));
    } else {
        panic!("Expected map result from discover-and-assess-agents");
    }

    // Test establish-system-baseline
    let expr = parse_expression("(establish-system-baseline)").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    if let Value::Map(map) = result {
        assert!(map.contains_key(&MapKey::Keyword(rtfs_compiler::ast::Keyword("baseline-established".to_string()))));
        assert!(map.contains_key(&MapKey::Keyword(rtfs_compiler::ast::Keyword("timestamp".to_string()))));
        assert!(map.contains_key(&MapKey::Keyword(rtfs_compiler::ast::Keyword("system-info".to_string()))));
    } else {
        panic!("Expected map result from establish-system-baseline");
    }
}

#[test]
fn test_map_filter_functions() {
    let mut env = StandardLibrary::create_global_environment();
    let module_registry = Arc::new(ModuleRegistry::new());
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(rtfs_compiler::runtime::capability_registry::CapabilityRegistry::new()));
    let capability_marketplace = std::sync::Arc::new(rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace::new(registry));
    let causal_chain = std::sync::Arc::new(std::sync::Mutex::new(rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap()));
    let security_context = rtfs_compiler::runtime::security::RuntimeContext::pure();
    let host = std::sync::Arc::new(rtfs_compiler::runtime::host::RuntimeHost::new(
        causal_chain,
        capability_marketplace,
        security_context.clone(),
    ));
    let evaluator = Evaluator::new(module_registry, Arc::new(rtfs_compiler::ccos::delegation::StaticDelegationEngine::new(std::collections::HashMap::new())), rtfs_compiler::runtime::security::RuntimeContext::pure(), host);

    // Test map function
    let expr = parse_expression("(map (fn [x] (* x 2)) [1 2 3])").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
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
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
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
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
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
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(rtfs_compiler::runtime::capability_registry::CapabilityRegistry::new()));
    let capability_marketplace = std::sync::Arc::new(rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace::new(registry));
    let causal_chain = std::sync::Arc::new(std::sync::Mutex::new(rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap()));
    let security_context = rtfs_compiler::runtime::security::RuntimeContext::pure();
    let host = std::sync::Arc::new(rtfs_compiler::runtime::host::RuntimeHost::new(
        causal_chain,
        capability_marketplace,
        security_context.clone(),
    ));
    let evaluator = Evaluator::new(module_registry, Arc::new(rtfs_compiler::ccos::delegation::StaticDelegationEngine::new(std::collections::HashMap::new())), rtfs_compiler::runtime::security::RuntimeContext::pure(), host);

    // Test reduce with initial value
    let expr = parse_expression("(reduce + 0 [1 2 3])").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    assert_eq!(result, Value::Integer(6));

    // Test reduce without initial value
    let expr = parse_expression("(reduce + [1 2 3])").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
    assert_eq!(result, Value::Integer(6));

    // Test reduce with empty collection and initial value
    let expr = parse_expression("(reduce + 42 [])").expect("Parse failed");
    let result = evaluator.evaluate_with_env(&expr, &mut env).expect("Evaluation failed");
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
