#[cfg(test)]
mod collections_tests {
    use crate::{
        ast::{Keyword, MapKey, TopLevel},
        parser,
        runtime::{module_runtime::ModuleRegistry, Evaluator, RuntimeResult, Value},
    };
    use std::collections::HashMap;
    
    use crate::ccos::capabilities::registry::CapabilityRegistry;
    use crate::ccos::capability_marketplace::CapabilityMarketplace;
    use crate::ccos::host::RuntimeHost;
    

    #[test]
    fn test_vectors() {
        let test_cases = vec![
            ("[]", Value::Vector(vec![])),
            (
                "[1 2 3]",
                Value::Vector(vec![
                    Value::Integer(1),
                    Value::Integer(2),
                    Value::Integer(3),
                ]),
            ),
            (
                "[1 \"hello\" true]",
                Value::Vector(vec![
                    Value::Integer(1),
                    Value::String("hello".to_string()),
                    Value::Boolean(true),
                ]),
            ),
        ];
        for (input, expected) in test_cases {
            let result = parse_and_evaluate(input);
            assert!(result.is_ok(), "Failed to parse/evaluate: {}", input);
            assert_eq!(result.unwrap(), expected, "Mismatch for: {}", input);
        }
    }

    #[test]
    fn test_maps() {
        let test_cases = vec![
            ("{}", Value::Map(HashMap::new())),
            ("{:a 1 :b 2}", {
                let mut map = HashMap::new();
                map.insert(MapKey::Keyword(Keyword("a".to_string())), Value::Integer(1));
                map.insert(MapKey::Keyword(Keyword("b".to_string())), Value::Integer(2));
                Value::Map(map)
            }),
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
        let evaluator = Evaluator::new(module_registry, security_context, host);
        if let Some(last_item) = parsed.last() {
            match last_item {
                TopLevel::Expression(expr) => {
                    match evaluator.evaluate(expr)? {
                        crate::runtime::execution_outcome::ExecutionOutcome::Complete(value) => Ok(value),
                        crate::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
                            Err(crate::runtime::error::RuntimeError::Generic("Host call required in pure test".to_string()))
                        }
                    }
                },
                _ => Ok(Value::String("object_defined".to_string())),
            }
        } else {
            Err(crate::runtime::error::RuntimeError::Generic(
                "Empty program".to_string(),
            ))
        }
    }
}
