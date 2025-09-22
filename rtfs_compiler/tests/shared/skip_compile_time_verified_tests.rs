/// Test suite for skip_compile_time_verified optimization
/// 
/// This test validates the hybrid type checking system with configurable optimization levels.

use std::collections::HashMap;
use std::sync::Arc;
use rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace;
use rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry;
use rtfs_compiler::runtime::type_validator::{
    TypeValidator, TypeCheckingConfig, VerificationContext, ValidationLevel
};
use rtfs_compiler::runtime::values::Value;
use rtfs_compiler::ast::{TypeExpr, PrimitiveType, TypePredicate, MapKey};
use tokio::sync::RwLock;

#[cfg(test)]
mod optimization_tests {
    use super::*;

    fn create_test_marketplace() -> CapabilityMarketplace {
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        CapabilityMarketplace::new(registry)
    }

    /// Test that simple types can be skipped when compile-time verified
    #[tokio::test]
    async fn test_skip_simple_types() {
        let validator = TypeValidator::new();
        
        // High-performance config that skips compile-time verified types
        let fast_config = TypeCheckingConfig {
            skip_compile_time_verified: true,
            enforce_capability_boundaries: true,
            validate_external_data: true,
            validation_level: ValidationLevel::Standard,
        };
        
        // Compile-time verified context
        let trusted_context = VerificationContext::compile_time_verified();
        
        // Simple string type - should be skipped
        let string_type = TypeExpr::Primitive(PrimitiveType::String);
        let string_value = Value::String("hello".to_string());
        
        let result = validator.validate_with_config(
            &string_value,
            &string_type,
            &fast_config,
            &trusted_context,
        );
        
        assert!(result.is_ok());
    }

    /// Test that refined types are never skipped (even when compile-time verified)
    #[tokio::test]
    async fn test_never_skip_refined_types() {
        let validator = TypeValidator::new();
        
        // High-performance config
        let fast_config = TypeCheckingConfig {
            skip_compile_time_verified: true,
            enforce_capability_boundaries: true,
            validate_external_data: true,
            validation_level: ValidationLevel::Standard,
        };
        
        // Compile-time verified context
        let trusted_context = VerificationContext::compile_time_verified();
        
        // Refined type with regex - should never be skipped
        let email_type = TypeExpr::Refined {
            base_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
            predicates: vec![TypePredicate::MatchesRegex("\\w+@\\w+\\.\\w+".to_string())],
        };
        
        // Valid email
        let valid_email = Value::String("test@example.com".to_string());
        let result = validator.validate_with_config(
            &valid_email,
            &email_type,
            &fast_config,
            &trusted_context,
        );
        assert!(result.is_ok());
        
        // Invalid email - should fail validation
        let invalid_email = Value::String("not-an-email".to_string());
        let result = validator.validate_with_config(
            &invalid_email,
            &email_type,
            &fast_config,
            &trusted_context,
        );
        assert!(result.is_err());
    }

    /// Test that capability boundaries always validate regardless of config
    #[tokio::test]
    async fn test_capability_boundary_always_validates() {
        let validator = TypeValidator::new();
        
        // Aggressive optimization config
        let aggressive_config = TypeCheckingConfig {
            skip_compile_time_verified: true,
            enforce_capability_boundaries: true,
            validate_external_data: true,
            validation_level: ValidationLevel::Basic,
        };
        
        // Capability boundary context - should never skip
        let boundary_context = VerificationContext::capability_boundary("test.capability");
        
        // Simple type that would normally be skipped
        let int_type = TypeExpr::Primitive(PrimitiveType::Int);
        
        // Valid integer
        let valid_int = Value::Integer(42);
        let result = validator.validate_with_config(
            &valid_int,
            &int_type,
            &aggressive_config,
            &boundary_context,
        );
        assert!(result.is_ok());
        
        // Type mismatch should be caught even with aggressive optimization
        let invalid_string = Value::String("not-an-int".to_string());
        let result = validator.validate_with_config(
            &invalid_string,
            &int_type,
            &aggressive_config,
            &boundary_context,
        );
        assert!(result.is_err());
    }

    /// Test external data validation is never skipped
    #[tokio::test]
    async fn test_external_data_always_validates() {
        let validator = TypeValidator::new();
        
        // Aggressive optimization config
        let aggressive_config = TypeCheckingConfig {
            skip_compile_time_verified: true,
            enforce_capability_boundaries: true,
            validate_external_data: true,
            validation_level: ValidationLevel::Basic,
        };
        
        // External data context - should never skip
        let external_context = VerificationContext::external_data("network.api");
        
        // Simple type that would normally be skipped
        let bool_type = TypeExpr::Primitive(PrimitiveType::Bool);
        
        // Valid boolean
        let valid_bool = Value::Boolean(true);
        let result = validator.validate_with_config(
            &valid_bool,
            &bool_type,
            &aggressive_config,
            &external_context,
        );
        assert!(result.is_ok());
        
        // Type mismatch should be caught
        let invalid_int = Value::Integer(123);
        let result = validator.validate_with_config(
            &invalid_int,
            &bool_type,
            &aggressive_config,
            &external_context,
        );
        assert!(result.is_err());
    }

    /// Test different validation levels
    #[tokio::test]
    async fn test_validation_levels() {
        let validator = TypeValidator::new();
        let context = VerificationContext::default();
        
        // Refined type with length constraint
        let constrained_string = TypeExpr::Refined {
            base_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
            predicates: vec![TypePredicate::MinLength(5)],
        };
        
        let short_string = Value::String("hi".to_string()); // Too short
        
        // Basic level - only validates types, should pass
        let basic_config = TypeCheckingConfig {
            skip_compile_time_verified: false,
            enforce_capability_boundaries: true,
            validate_external_data: true,
            validation_level: ValidationLevel::Basic,
        };
        
        let result = validator.validate_with_config(
            &short_string,
            &constrained_string,
            &basic_config,
            &context,
        );
        assert!(result.is_ok()); // Basic level ignores length constraint
        
        // Strict level - validates all predicates, should fail
        let strict_config = TypeCheckingConfig {
            skip_compile_time_verified: false,
            enforce_capability_boundaries: true,
            validate_external_data: true,
            validation_level: ValidationLevel::Strict,
        };
        
        let result = validator.validate_with_config(
            &short_string,
            &constrained_string,
            &strict_config,
            &context,
        );
        assert!(result.is_err()); // Strict level catches length violation
    }

    /// Test capability marketplace integration with optimization
    #[tokio::test]
    async fn test_marketplace_optimized_execution() {
        let marketplace = create_test_marketplace();
        
        // Register a simple capability
        marketplace.register_local_capability_with_schema(
            "math.add".to_string(),
            "Add Numbers".to_string(),
            "Simple addition capability".to_string(),
            Arc::new(|input| {
                if let Value::Map(map) = input {
                    if let (Some(Value::Integer(a)), Some(Value::Integer(b))) = (
                        map.get(&MapKey::String("a".to_string())),
                        map.get(&MapKey::String("b".to_string()))
                    ) {
                        Ok(Value::Integer(a + b))
                    } else {
                        Err(rtfs_compiler::runtime::error::RuntimeError::Generic("Invalid input".to_string()))
                    }
                } else {
                    Err(rtfs_compiler::runtime::error::RuntimeError::Generic("Expected map input".to_string()))
                }
            }),
            Some(TypeExpr::Map { entries: vec![], wildcard: None }), // Simple map type
            Some(TypeExpr::Primitive(PrimitiveType::Int)),
        ).await.expect("Failed to register capability");
        
        // Test optimized execution
        let fast_config = TypeCheckingConfig {
            skip_compile_time_verified: true,
            enforce_capability_boundaries: true,
            validate_external_data: true,
            validation_level: ValidationLevel::Standard,
        };
        
        let mut params = HashMap::new();
        params.insert("a".to_string(), Value::Integer(5));
        params.insert("b".to_string(), Value::Integer(3));
        
        let result = marketplace.execute_with_validation_config(
            "math.add",
            &params,
            &fast_config,
        ).await;
        
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::Integer(8));
    }

    /// Test performance characteristics of optimization
    #[tokio::test]
    async fn test_optimization_performance_characteristics() {
        let validator = TypeValidator::new();
        
        // Create a complex nested type structure
        let complex_type = TypeExpr::Vector(Box::new(
            TypeExpr::Map { entries: vec![], wildcard: None }
        ));
        
        let test_value = Value::Vector(vec![
            Value::Map(HashMap::new()),
            Value::Map(HashMap::new()),
            Value::Map(HashMap::new()),
        ]);
        
        // Optimized config should be faster for simple types
        let optimized_config = TypeCheckingConfig {
            skip_compile_time_verified: true,
            enforce_capability_boundaries: true,
            validate_external_data: true,
            validation_level: ValidationLevel::Basic,
        };
        
        let optimized_context = VerificationContext::compile_time_verified();
        
        // This should be very fast (skipped)
        let start = std::time::Instant::now();
        let result = validator.validate_with_config(
            &test_value,
            &complex_type,
            &optimized_config,
            &optimized_context,
        );
        let optimized_duration = start.elapsed();
        
        assert!(result.is_ok());
        
        // Strict validation should be slower
        let strict_config = TypeCheckingConfig {
            skip_compile_time_verified: false,
            enforce_capability_boundaries: true,
            validate_external_data: true,
            validation_level: ValidationLevel::Strict,
        };
        
        let strict_context = VerificationContext::default();
        
        let start = std::time::Instant::now();
        let result = validator.validate_with_config(
            &test_value,
            &complex_type,
            &strict_config,
            &strict_context,
        );
        let strict_duration = start.elapsed();
        
        assert!(result.is_ok());
        
        println!("Optimized: {:?}, Strict: {:?}", optimized_duration, strict_duration);
        // In a real scenario, optimized should be significantly faster
        // For this simple test, the difference may be minimal
    }

    /// Test security boundary enforcement
    #[tokio::test]
    async fn test_security_boundary_enforcement() {
        let validator = TypeValidator::new();
        
        // Security-critical regex type
        let security_type = TypeExpr::Refined {
            base_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
            predicates: vec![TypePredicate::MatchesRegex("^[a-zA-Z0-9_]+$".to_string())], // Only alphanumeric
        };
        
        let safe_input = Value::String("safe_input_123".to_string());
        let dangerous_input = Value::String("malicious; rm -rf /".to_string());
        
        // Standard validation at capability boundary
        let boundary_context = VerificationContext::capability_boundary("security.check");
        let standard_config = TypeCheckingConfig {
            skip_compile_time_verified: true,
            enforce_capability_boundaries: true,
            validate_external_data: true,
            validation_level: ValidationLevel::Standard,
        };
        
        // Safe input should pass
        let result = validator.validate_with_config(
            &safe_input,
            &security_type,
            &standard_config,
            &boundary_context,
        );
        assert!(result.is_ok());
        
        // Dangerous input should be rejected
        let result = validator.validate_with_config(
            &dangerous_input,
            &security_type,
            &standard_config,
            &boundary_context,
        );
        assert!(result.is_err());
    }
}
