#[cfg(test)]
mod control_flow_tests {
    use crate::{
        parser,
        runtime::{module_runtime::ModuleRegistry, Evaluator, RuntimeResult, Value},
    };
    
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
        let registry = std::sync::Arc::new(tokio::sync::RwLock::new(crate::runtime::capabilities::registry::CapabilityRegistry::new()));
        let capability_marketplace = std::sync::Arc::new(crate::runtime::capability_marketplace::CapabilityMarketplace::new(registry));
        let causal_chain = std::sync::Arc::new(std::sync::Mutex::new(crate::ccos::causal_chain::CausalChain::new().unwrap()));
        let security_context = crate::runtime::security::RuntimeContext::pure();
        let host = std::sync::Arc::new(crate::runtime::host::RuntimeHost::new(
            causal_chain,
            capability_marketplace,
            security_context.clone(),
        ));
    let mut evaluator = Evaluator::new(std::sync::Arc::new(module_registry), Arc::new(StaticDelegationEngine::new(HashMap::new())), security_context, host);

        // Evaluate all top-level forms in sequence using the evaluator's environment
        let result = evaluator.eval_toplevel(&parsed);
        if let Err(ref e) = result {
            println!("Evaluation error: {:?}", e);
        }
        result
    }
}
