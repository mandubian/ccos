#[cfg(test)]
mod control_flow_tests {
    use crate::{
        ast::TopLevel,
        parser,
        runtime::{module_runtime::ModuleRegistry, Evaluator, RuntimeResult, Value},
    };
    use std::rc::Rc;
    use crate::ccos::delegation::StaticDelegationEngine;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[test]
    fn test_if_expressions() {
        let test_cases = vec![
            ("(if true 1 2)", Value::Integer(1)),
            ("(if false 1 2)", Value::Integer(2)),
            (
                "(if (= 1 1) \"yes\" \"no\")",
                Value::String("yes".to_string()),
            ),
        ];
        for (input, expected) in test_cases {
            let result = parse_and_evaluate(input);
            assert!(result.is_ok(), "Failed to parse/evaluate: {}", input);
            assert_eq!(result.unwrap(), expected, "Mismatch for: {}", input);
        }
    }

    #[test]
    fn test_let_expressions() {
        let test_cases = vec![
            ("(let [x 1] x)", Value::Integer(1)),
            ("(let [x 1 y 2] (+ x y))", Value::Integer(3)),
            ("(let [x 1 y (+ x 1)] y)", Value::Integer(2)),
        ];
        for (input, expected) in test_cases {
            let result = parse_and_evaluate(input);
            assert!(result.is_ok(), "Failed to parse/evaluate: {}", input);
            assert_eq!(result.unwrap(), expected, "Mismatch for: {}", input);
        }
    }

    fn parse_and_evaluate(input: &str) -> RuntimeResult<Value> {
        let parsed = parser::parse(input).expect("Failed to parse");
        let mut module_registry = ModuleRegistry::new();
        // Load stdlib to get arithmetic functions
        crate::runtime::stdlib::load_stdlib(&mut module_registry).expect("Failed to load stdlib");
        let mut evaluator = Evaluator::new(Rc::new(module_registry), Arc::new(StaticDelegationEngine::new(HashMap::new())), crate::runtime::security::RuntimeContext::pure());

        // Evaluate all top-level forms in sequence using the evaluator's environment
        let result = evaluator.eval_toplevel(&parsed);
        if let Err(ref e) = result {
            println!("Evaluation error: {:?}", e);
        }
        result
    }
}
