#[cfg(test)]
mod primitives_tests {
    use crate::{
        ast::{Keyword, TopLevel},
        parser,
        runtime::{module_runtime::ModuleRegistry, Evaluator, RuntimeResult, Value},
    };
    use crate::ccos::delegation::StaticDelegationEngine;
    use crate::ccos::capabilities::registry::CapabilityRegistry;
    use crate::ccos::capability_marketplace::CapabilityMarketplace;
    use crate::ccos::host::RuntimeHost;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[test]
    fn test_basic_literals() {
        let test_cases = vec![
            ("42", Value::Integer(42)),
            ("3.14", Value::Float(3.14)),
            ("\"hello\"", Value::String("hello".to_string())),
            ("true", Value::Boolean(true)),
            ("false", Value::Boolean(false)),
            ("nil", Value::Nil),
            (":keyword", Value::Keyword(Keyword("keyword".to_string()))),
        ];
        for (input, expected) in test_cases {
            let result = parse_and_evaluate(input);
            assert!(result.is_ok(), "Failed to parse/evaluate: {}", input);
            assert_eq!(result.unwrap(), expected, "Mismatch for: {}", input);
        }
    }

    #[test]
    fn test_basic_arithmetic() {
        let test_cases = vec![
            ("(+ 1 2)", Value::Integer(3)),
            ("(- 5 3)", Value::Integer(2)),
            ("(* 4 3)", Value::Integer(12)),
            ("(/ 10 2)", Value::Integer(5)),
            ("(+ 1.5 2.5)", Value::Float(4.0)),
            ("(- 5.5 2.5)", Value::Float(3.0)),
        ];
        for (input, expected) in test_cases {
            let result = parse_and_evaluate(input);
            assert!(result.is_ok(), "Failed to parse/evaluate: {}", input);
            assert_eq!(result.unwrap(), expected, "Mismatch for: {}", input);
        }
    }

    #[test]
    fn test_basic_comparisons() {
        let test_cases = vec![
            ("(= 1 1)", Value::Boolean(true)),
            ("(= 1 2)", Value::Boolean(false)),
            ("(< 1 2)", Value::Boolean(true)),
            ("(> 2 1)", Value::Boolean(true)),
            ("(<= 1 1)", Value::Boolean(true)),
            ("(>= 2 1)", Value::Boolean(true)),
        ];
        for (input, expected) in test_cases {
            let result = parse_and_evaluate(input);
            assert!(result.is_ok(), "Failed to parse/evaluate: {}", input);
            assert_eq!(result.unwrap(), expected, "Mismatch for: {}", input);
        }
    }

    fn parse_and_evaluate(input: &str) -> RuntimeResult<Value> {
        let parsed = parser::parse(input).expect("Failed to parse");
    let module_registry = std::sync::Arc::new(ModuleRegistry::new());
        let registry = std::sync::Arc::new(tokio::sync::RwLock::new(CapabilityRegistry::new()));
        let capability_marketplace = std::sync::Arc::new(CapabilityMarketplace::new(registry));
        let causal_chain = std::sync::Arc::new(std::sync::Mutex::new(crate::ccos::causal_chain::CausalChain::new().unwrap()));
        let security_context = crate::runtime::security::RuntimeContext::pure();
        let host = std::sync::Arc::new(RuntimeHost::new(
            causal_chain,
            capability_marketplace,
            security_context.clone(),
        ));
        let evaluator = Evaluator::new(module_registry, Arc::new(StaticDelegationEngine::new(HashMap::new())), security_context, host);
        if let Some(last_item) = parsed.last() {
            match last_item {
                TopLevel::Expression(expr) => evaluator.evaluate(expr),
                _ => Ok(Value::String("object_defined".to_string())),
            }
        } else {
            Err(crate::runtime::error::RuntimeError::Generic(
                "Empty program".to_string(),
            ))
        }
    }
}
