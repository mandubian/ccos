#[cfg(test)]
mod object_tests {
    use crate::{
        ast::TopLevel,
        parser,
        runtime::{module_runtime::ModuleRegistry, Evaluator, RuntimeResult, Value},
        validator::SchemaValidator,
    };
    use crate::ccos::delegation::StaticDelegationEngine;
    use crate::ccos::capabilities::registry::CapabilityRegistry;
    use crate::ccos::capability_marketplace::CapabilityMarketplace;
    use crate::ccos::host::RuntimeHost;
    use std::collections::HashMap;
    use std::sync::Arc;
    

    #[test]
    fn test_intent_definition() {
        let intent_code = r#"
        (intent :rtfs.core:v2.0:intent
            :type :rtfs.core:v2.0:intent
            :intent-id "intent-001"
            :goal "Process user data"
            :created-at "2024-01-01T00:00:00Z"
            :created-by "user-001"
            :status "active"
        )
        "#;
        let parsed = parser::parse(intent_code).expect("Failed to parse intent");
        assert_eq!(parsed.len(), 1);
        let validation_result = SchemaValidator::validate_object(&parsed[0]);
        assert!(
            validation_result.is_ok(),
            "Intent validation failed: {:?}",
            validation_result
        );
    }

    #[test]
    fn test_plan_definition() {
        let plan_code = r#"
        (plan :rtfs.core:v2.0:plan
            :type :rtfs.core:v2.0:plan
            :plan-id "plan-001"
            :created-at "2024-01-01T00:00:00Z"
            :created-by "user-001"
            :intent-ids ["intent-001"]
            :program (+ 1 2)
            :status "ready"
        )
        "#;
        let parsed = parser::parse(plan_code).expect("Failed to parse plan");
        assert_eq!(parsed.len(), 1);
        let validation_result = SchemaValidator::validate_object(&parsed[0]);
        assert!(
            validation_result.is_ok(),
            "Plan validation failed: {:?}",
            validation_result
        );
    }

    #[test]
    fn test_action_definition() {
        let action_code = r#"
        (action :rtfs.core:v2.0:action
            :type :rtfs.core:v2.0:action
            :action-id "action-001"
            :timestamp "2024-01-01T00:00:00Z"
            :plan-id "plan-001"
            :step-id "step-001"
            :intent-id "intent-001"
            :capability-used "data-processing"
            :executor "agent-001"
            :input {:data "test"}
            :output {:result "success"}
            :execution {:duration 100}
            :signature "abc123"
        )
        "#;
        let parsed = parser::parse(action_code).expect("Failed to parse action");
        assert_eq!(parsed.len(), 1);
        let validation_result = SchemaValidator::validate_object(&parsed[0]);
        assert!(
            validation_result.is_ok(),
            "Action validation failed: {:?}",
            validation_result
        );
    }

    #[test]
    fn test_rtfs2_integration() {
        let rtfs2_program = r#"
        ;; Define an intent
        (intent :rtfs.core:v2.0:intent
            :type :rtfs.core:v2.0:intent
            :intent-id "test-intent"
            :goal "Test RTFS 2.0 integration"
            :created-at "2024-01-01T00:00:00Z"
            :created-by "test-user"
            :status "active"
        )
        
        ;; Define a plan
        (plan :rtfs.core:v2.0:plan
            :type :rtfs.core:v2.0:plan
            :plan-id "test-plan"
            :created-at "2024-01-01T00:00:00Z"
            :created-by "test-user"
            :intent-ids ["test-intent"]
            :program (+ 1 2)
            :status "ready"
        )
        
        ;; Execute the plan
        (+ 1 2)
        "#;
        let parsed = parser::parse(rtfs2_program).expect("Failed to parse RTFS 2.0 program");
        assert_eq!(parsed.len(), 3);

        // Validate objects
        for item in &parsed[..2] {
            let validation_result = SchemaValidator::validate_object(item);
            assert!(
                validation_result.is_ok(),
                "RTFS 2.0 object validation failed"
            );
        }

        // Execute the expression
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
        if let TopLevel::Expression(expr) = &parsed[2] {
            let result = evaluator.evaluate(expr);
            assert!(result.is_ok(), "Expression evaluation failed");
            match result.unwrap() {
                crate::runtime::execution_outcome::ExecutionOutcome::Complete(value) => {
                    assert_eq!(value, Value::Integer(3));
                },
                crate::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
                    panic!("Host call required in pure test");
                }
            }
        } else {
            panic!("Expected expression");
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
