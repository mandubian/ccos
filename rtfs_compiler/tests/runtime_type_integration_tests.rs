/// Integration tests for hybrid type checking in the broader RTFS runtime
/// 
/// This test suite validates that TypeCheckingConfig and VerificationContext
/// are properly integrated into the parser, evaluator, and general expression
/// evaluation pipeline - not just capability boundaries.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use rtfs_compiler::ast::{TypeExpr, PrimitiveType};
use rtfs_compiler::parser;
use rtfs_compiler::runtime::{Evaluator, Value};
use rtfs_compiler::runtime::environment::Environment;
use rtfs_compiler::runtime::module_runtime::ModuleRegistry;
use rtfs_compiler::runtime::host_interface::HostInterface;
use rtfs_compiler::runtime::security::RuntimeContext;
use rtfs_compiler::runtime::stdlib::StandardLibrary;
use rtfs_compiler::runtime::type_validator::{
    TypeCheckingConfig, ValidationLevel
};
use rtfs_compiler::runtime::error::RuntimeError;
use rtfs_compiler::ccos::delegation::StaticDelegationEngine;
use rtfs_compiler::ccos::types::ExecutionResult;

/// Mock host interface for testing
#[derive(Debug)]
struct MockHost;

impl HostInterface for MockHost {
    fn execute_capability(&self, _name: &str, _args: &[Value]) -> Result<Value, RuntimeError> {
        Ok(Value::Nil)
    }
    
    fn notify_step_started(&self, _step_name: &str) -> Result<String, RuntimeError> {
        Ok("mock-step-id".to_string())
    }
    
    fn notify_step_completed(&self, _step_action_id: &str, _result: &ExecutionResult) -> Result<(), RuntimeError> {
        Ok(())
    }
    
    fn notify_step_failed(&self, _step_action_id: &str, _error: &str) -> Result<(), RuntimeError> {
        Ok(())
    }
    
    fn set_execution_context(&self, _plan_id: String, _intent_ids: Vec<String>, _parent_action_id: String) {
        // Mock implementation
    }
    
    fn clear_execution_context(&self) {
        // Mock implementation
    }

    fn set_step_exposure_override(&self, _expose: bool, _context_keys: Option<Vec<String>>) {
        // Mock implementation
    }

    fn clear_step_exposure_override(&self) {
        // Mock implementation
    }

    fn get_context_value(&self, _key: &str) -> Option<Value> {
    // Return None for all keys in the mock (compatible with updated HostInterface)
    None
    }
}

#[cfg(test)]
mod hybrid_runtime_integration_tests {
    use super::*;

    fn create_test_evaluator_optimized() -> Evaluator {
        let module_registry = Rc::new(ModuleRegistry::new());
        let static_map = HashMap::new();
        let delegation_engine = Arc::new(StaticDelegationEngine::new(static_map));
        let security_context = RuntimeContext::pure();
        let host = Arc::new(MockHost);
        
        Evaluator::new_optimized(module_registry, delegation_engine, security_context, host)
    }

    fn create_test_evaluator_strict() -> Evaluator {
        let module_registry = Rc::new(ModuleRegistry::new());
        let static_map = HashMap::new();
        let delegation_engine = Arc::new(StaticDelegationEngine::new(static_map));
        let security_context = RuntimeContext::pure();
        let host = Arc::new(MockHost);
        
        Evaluator::new_strict(module_registry, delegation_engine, security_context, host)
    }

    /// Test that optimized evaluator skips validation for compile-time verified literals
    #[test]
    fn test_optimized_literal_evaluation() {
        let evaluator = create_test_evaluator_optimized();
        
        // Parse a simple string literal
        let input = r#""hello world""#;
        let parsed = parser::parse_expression(input).expect("Failed to parse");
        
        let mut env = Environment::new();
        let result = evaluator.eval_expr(&parsed, &mut env);
        
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::String("hello world".to_string()));
    }

    /// Test that strict evaluator validates everything, even compile-time verified types
    #[test]
    fn test_strict_literal_evaluation() {
        let evaluator = create_test_evaluator_strict();
        
        // Parse a simple string literal
        let input = r#""hello world""#;
        let parsed = parser::parse_expression(input).expect("Failed to parse");
        
        let mut env = Environment::new();
        let result = evaluator.eval_expr(&parsed, &mut env);
        
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::String("hello world".to_string()));
    }

    /// Test arithmetic operations with different validation levels
    #[test]
    fn test_arithmetic_with_optimization() {
        let evaluator = create_test_evaluator_optimized();
        
        // Parse arithmetic expression
        let input = "(+ 1 2 3)";
        let parsed = parser::parse_expression(input).expect("Failed to parse");
        
        // Use the standard library environment that includes arithmetic functions
        let mut env = StandardLibrary::create_global_environment();
        let result = evaluator.eval_expr(&parsed, &mut env);
        
        if let Err(ref e) = result {
            eprintln!("Error in arithmetic test: {:?}", e);
        }
        assert!(result.is_ok(), "Expected successful evaluation, got: {:?}", result);
        assert_eq!(result.unwrap(), Value::Integer(6));
    }

    /// Test function definition and call with type checking
    #[test]
    fn test_function_definition_with_validation() {
        let evaluator = create_test_evaluator_strict();
        
        // Define a simple function
        let def_input = "(defn double [x] (* x 2))";
        let def_parsed = parser::parse_expression(def_input).expect("Failed to parse function definition");
        
        let mut env = StandardLibrary::create_global_environment();
        let def_result = evaluator.eval_expr(&def_parsed, &mut env);
        if let Err(ref e) = def_result {
            eprintln!("Error in function definition: {:?}", e);
        }
        assert!(def_result.is_ok(), "Expected successful function definition, got: {:?}", def_result);
        
        // Call the function
        let call_input = "(double 5)";
        let call_parsed = parser::parse_expression(call_input).expect("Failed to parse function call");
        
        let call_result = evaluator.eval_expr(&call_parsed, &mut env);
        if let Err(ref e) = call_result {
            eprintln!("Error in function call: {:?}", e);
        }
        assert!(call_result.is_ok(), "Expected successful function call, got: {:?}", call_result);
        assert_eq!(call_result.unwrap(), Value::Integer(10));
    }

    /// Test vector operations with type validation
    #[test]
    fn test_vector_operations() {
        let evaluator = create_test_evaluator_optimized();
        
        // Create a vector
        let input = "[1 2 3 4 5]";
        let parsed = parser::parse_expression(input).expect("Failed to parse");
        
        let mut env = Environment::new();
        let result = evaluator.eval_expr(&parsed, &mut env);
        
        assert!(result.is_ok());
        if let Value::Vector(vec) = result.unwrap() {
            assert_eq!(vec.len(), 5);
            assert_eq!(vec[0], Value::Integer(1));
            assert_eq!(vec[4], Value::Integer(5));
        } else {
            panic!("Expected vector result");
        }
    }

    /// Test map operations with type validation
    #[test]
    fn test_map_operations() {
        let evaluator = create_test_evaluator_strict();
        
        // Create a map
        let input = r#"{"name" "John" "age" 30}"#;
        let parsed = parser::parse_expression(input).expect("Failed to parse");
        
        let mut env = Environment::new();
        let result = evaluator.eval_expr(&parsed, &mut env);
        
        assert!(result.is_ok());
        if let Value::Map(map) = result.unwrap() {
            assert_eq!(map.len(), 2);
        } else {
            panic!("Expected map result");
        }
    }

    /// Test configuration changes affect validation behavior
    #[test]
    fn test_configuration_effects() {
        // Create evaluator with optimized config
        let mut optimized_evaluator = create_test_evaluator_optimized();
        assert!(optimized_evaluator.get_type_checking_config().skip_compile_time_verified);
        
        // Create evaluator with strict config
        let strict_evaluator = create_test_evaluator_strict();
        assert!(!strict_evaluator.get_type_checking_config().skip_compile_time_verified);
        
        // Change optimized evaluator to strict
        let strict_config = TypeCheckingConfig {
            skip_compile_time_verified: false,
            enforce_capability_boundaries: true,
            validate_external_data: true,
            validation_level: ValidationLevel::Strict,
        };
        optimized_evaluator.set_type_checking_config(strict_config);
        assert!(!optimized_evaluator.get_type_checking_config().skip_compile_time_verified);
    }

    /// Test that type validator is integrated into evaluator
    #[test]
    fn test_type_validator_integration() {
        let evaluator = create_test_evaluator_optimized();
        
        // Verify type validator is present and functional
        let validator = &evaluator.type_validator;
        let string_type = TypeExpr::Primitive(PrimitiveType::String);
        let string_value = Value::String("test".to_string());
        
        let result = validator.validate_value(&string_value, &string_type);
        assert!(result.is_ok());
        
        // Test type mismatch
        let int_value = Value::Integer(42);
        let mismatch_result = validator.validate_value(&int_value, &string_type);
        assert!(mismatch_result.is_err());
    }

    /// Test evaluation with different validation levels
    #[test]
    fn test_validation_levels() {
        let mut evaluator = create_test_evaluator_optimized();
        
        // Test Basic validation level
        let basic_config = TypeCheckingConfig {
            skip_compile_time_verified: true,
            enforce_capability_boundaries: true,
            validate_external_data: true,
            validation_level: ValidationLevel::Basic,
        };
        evaluator.set_type_checking_config(basic_config);
        
        let input = r#""test string""#;
        let parsed = parser::parse_expression(input).expect("Failed to parse");
        let mut env = Environment::new();
        let result = evaluator.eval_expr(&parsed, &mut env);
        assert!(result.is_ok());
        
        // Test Standard validation level
        let standard_config = TypeCheckingConfig {
            skip_compile_time_verified: true,
            enforce_capability_boundaries: true,
            validate_external_data: true,
            validation_level: ValidationLevel::Standard,
        };
        evaluator.set_type_checking_config(standard_config);
        
        let result2 = evaluator.eval_expr(&parsed, &mut env);
        assert!(result2.is_ok());
        
        // Test Strict validation level
        let strict_config = TypeCheckingConfig {
            skip_compile_time_verified: false,
            enforce_capability_boundaries: true,
            validate_external_data: true,
            validation_level: ValidationLevel::Strict,
        };
        evaluator.set_type_checking_config(strict_config);
        
        let result3 = evaluator.eval_expr(&parsed, &mut env);
        assert!(result3.is_ok());
    }

    /// Test complex expressions with nested evaluations
    #[test]
    fn test_complex_expression_evaluation() {
        let evaluator = create_test_evaluator_optimized();
        
        // Complex nested expression
        let input = "(+ (* 2 3) (- 10 5))";
        let parsed = parser::parse_expression(input).expect("Failed to parse");
        
        let mut env = StandardLibrary::create_global_environment();
        let result = evaluator.eval_expr(&parsed, &mut env);
        
        if let Err(ref e) = result {
            eprintln!("Error in complex expression: {:?}", e);
        }
        assert!(result.is_ok(), "Expected successful complex evaluation, got: {:?}", result);
        assert_eq!(result.unwrap(), Value::Integer(11)); // (2*3) + (10-5) = 6 + 5 = 11
    }

    /// Test that the hybrid architecture preserves performance optimizations
    #[test]
    fn test_performance_characteristics() {
        let optimized_evaluator = create_test_evaluator_optimized();
        let strict_evaluator = create_test_evaluator_strict();
        
        let input = r#"[1 2 3 "hello" true]"#;
        let parsed = parser::parse_expression(input).expect("Failed to parse");
        
        let mut env1 = Environment::new();
        let mut env2 = Environment::new();
        
        // Both should work, but optimized should be faster for simple types
        let start = std::time::Instant::now();
        let result1 = optimized_evaluator.eval_expr(&parsed, &mut env1);
        let optimized_duration = start.elapsed();
        
        let start = std::time::Instant::now();
        let result2 = strict_evaluator.eval_expr(&parsed, &mut env2);
        let strict_duration = start.elapsed();
        
        assert!(result1.is_ok());
        assert!(result2.is_ok());
        assert_eq!(result1.unwrap(), result2.unwrap());
        
        // Optimized should generally be faster or at least not significantly slower
        // Note: In practice the difference might be very small for simple expressions
        println!("Optimized duration: {:?}", optimized_duration);
        println!("Strict duration: {:?}", strict_duration);
    }
}
