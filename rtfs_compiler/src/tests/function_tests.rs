#[cfg(test)]
mod function_tests {
    use crate::{
        ast::TopLevel,
        parser,
        runtime::{module_runtime::ModuleRegistry, Evaluator, RuntimeResult, Value},
    };
    use std::rc::Rc;

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

    fn parse_and_evaluate(input: &str) -> RuntimeResult<Value> {
        let parsed = parser::parse(input).expect("Failed to parse");
        println!("Parsed top-level forms: {:#?}", parsed);
        let mut module_registry = ModuleRegistry::new();
        // Load stdlib to get map and other builtin functions
        crate::runtime::stdlib::load_stdlib(&mut module_registry).expect("Failed to load stdlib");
        let mut evaluator = Evaluator::new(Rc::new(module_registry));
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
}
