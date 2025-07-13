#[cfg(test)]
mod function_tests {
    use crate::{
        ast::TopLevel,
        parser,
        runtime::{module_runtime::ModuleRegistry, Evaluator, RuntimeResult, Value},
    };
    use std::rc::Rc;
    use crate::ccos::delegation::{StaticDelegationEngine, ExecTarget};
    use std::sync::Arc;
    use std::collections::HashMap;

    #[test]
    fn test_function_definitions() {
        let test_cases = vec![
            ("(defn add [x y] (+ x y)) (add 1 2)", Value::Integer(3)),
            ("(defn double [x] (* x 2)) (double 5)", Value::Integer(10)),
        ];
        for (input, expected) in test_cases {
            let result = parse_and_evaluate(input);
            assert!(result.is_ok(), "Failed to parse/evaluate: {}", input);
            assert_eq!(result.unwrap(), expected, "Mismatch for: {}", input);
        }
    }

    #[test]
    fn test_recursion() {
        let factorial_code = r#"
        (defn factorial [n]
          (if (= n 0)
            1
            (* n (factorial (- n 1)))))
        (factorial 5)
        "#;
        let result = parse_and_evaluate(factorial_code);
        assert!(result.is_ok(), "Recursion test failed");
        assert_eq!(result.unwrap(), Value::Integer(120));
    }

    #[test]
    fn test_higher_order_functions() {
        let map_code = r#"
        (defn double [x] (* x 2))
        (map double [1 2 3 4 5])
        "#;
        let result = parse_and_evaluate(map_code);
        println!("Higher order function result: {:?}", result);
        assert!(result.is_ok(), "Higher order function test failed");
        // Expected: [2 4 6 8 10]
        if let Ok(Value::Vector(values)) = result {
            assert_eq!(values.len(), 5);
            assert_eq!(values[0], Value::Integer(2));
            assert_eq!(values[4], Value::Integer(10));
        } else {
            panic!("Expected vector result");
        }
    }

    #[test]
    fn test_map_performance_comparison() {
        // Test with builtin arithmetic (fast path)
        let builtin_code = r#"
        (map (fn [x] (+ x x)) [1 2 3 4 5 6 7 8 9 10])
        "#;

        // Test with user-defined function (slow path)
        let user_defined_code = r#"
        (defn double [x] (+ x x))
        (map double [1 2 3 4 5 6 7 8 9 10])
        "#;

        let start = std::time::Instant::now();
        let builtin_result = parse_and_evaluate(builtin_code);
        let builtin_time = start.elapsed();

        let start = std::time::Instant::now();
        let user_result = parse_and_evaluate(user_defined_code);
        let user_time = start.elapsed();

        println!("Builtin arithmetic map time: {:?}", builtin_time);
        println!("User-defined function map time: {:?}", user_time);
        println!(
            "Performance ratio: {:.2}x",
            user_time.as_nanos() as f64 / builtin_time.as_nanos() as f64
        );

        assert!(builtin_result.is_ok());
        assert!(user_result.is_ok());

        // Both should produce same result
        assert_eq!(builtin_result.unwrap(), user_result.unwrap());
    }

    #[test]
    fn test_user_defined_higher_order_function() {
        let code = r#"
        (defn my_map [f xs]
          (map f xs))
        (defn inc [x] (+ x 1))
        (my_map inc [1 2 3 4 5])
        "#;
        let result = parse_and_evaluate(code);
        println!("User-defined higher-order function result: {:?}", result);
        assert!(
            result.is_ok(),
            "User-defined higher-order function test failed"
        );
        if let Ok(Value::Vector(values)) = result {
            assert_eq!(
                values,
                vec![
                    Value::Integer(2),
                    Value::Integer(3),
                    Value::Integer(4),
                    Value::Integer(5),
                    Value::Integer(6)
                ]
            );
        } else {
            panic!("Expected vector result");
        }
    }

    #[test]
    fn test_delegation_engine_integration() {
        // Set up a StaticDelegationEngine that delegates "delegate-me" to a model that exists
        let mut static_map = HashMap::new();
        static_map.insert("delegate-me".to_string(), ExecTarget::LocalModel("echo-model".to_string()));
        let de = Arc::new(StaticDelegationEngine::new(static_map));

        // Define and call the delegated function
        let code = r#"
        (defn delegate-me [x] (+ x 1))
        (delegate-me 42)
        "#;
        let result = parse_and_evaluate_with_de(code, de.clone());
        // Now that model providers are implemented, this should work
        assert!(result.is_ok(), "Expected delegated call to work with echo model");
        let value = result.unwrap();
        // The echo model should return a string with the prompt
        assert!(matches!(value, Value::String(_)), "Expected string result from model");
        if let Value::String(s) = value {
            assert!(s.contains("[ECHO]"), "Expected echo model prefix");
            assert!(s.contains("arg0: 42"), "Expected argument in prompt");
        }

        // Now test a function that is not delegated (should work)
        let static_map = HashMap::new();
        let de = Arc::new(StaticDelegationEngine::new(static_map));
        let code = r#"
        (defn add1 [x] (+ x 1))
        (add1 41)
        "#;
        let result = parse_and_evaluate_with_de(code, de);
        assert!(result.is_ok(), "Expected local call to succeed");
        assert_eq!(result.unwrap(), Value::Integer(42));
    }

    fn parse_and_evaluate(input: &str) -> RuntimeResult<Value> {
        let parsed = parser::parse(input).expect("Failed to parse");
        let mut module_registry = ModuleRegistry::new();
        // Load stdlib to get map and other builtin functions
        crate::runtime::stdlib::load_stdlib(&mut module_registry).expect("Failed to load stdlib");
        let de = Arc::new(StaticDelegationEngine::new(HashMap::new()));
        let mut evaluator = Evaluator::new(Rc::new(module_registry), de, crate::runtime::security::RuntimeContext::pure());
        println!("Symbols in environment: {:?}", evaluator.env.symbol_names());
        println!(
            "Map lookupable: {:?}",
            evaluator.env.lookup(&crate::ast::Symbol("map".to_string()))
        );
        // Check if map is actually lookupable
        let map_symbol = crate::ast::Symbol("map".to_string());
        match evaluator.env.lookup(&map_symbol) {
            Some(value) => println!("Map function found: {:?}", value),
            None => println!("Map function NOT found in environment"),
        }

        // Evaluate all top-level forms in sequence
        evaluator.eval_toplevel(&parsed)
    }

    fn parse_and_evaluate_with_de(input: &str, de: Arc<dyn crate::ccos::delegation::DelegationEngine>) -> RuntimeResult<Value> {
        let parsed = parser::parse(input).expect("Failed to parse");
        let mut module_registry = ModuleRegistry::new();
        crate::runtime::stdlib::load_stdlib(&mut module_registry).expect("Failed to load stdlib");
        let mut evaluator = Evaluator::new(Rc::new(module_registry), de, crate::runtime::security::RuntimeContext::pure());
        evaluator.eval_toplevel(&parsed)
    }
}
