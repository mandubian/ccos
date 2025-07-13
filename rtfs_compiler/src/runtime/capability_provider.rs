// CCOS Capability Provider Core Interfaces
// This file implements the architecture described in CAPABILITY_PROVIDER_IMPLEMENTATION.md
// 
// Key Features:
// - Uses RTFS TypeExpr::Function for capability type signatures
// - Leverages RTFS TypeExpr::Intersection for constraint validation ([:and Type Predicate])
// - Provides runtime validation utilities for constrained types
// - Includes helper functions for common constraint patterns
// - Supports automatic input/output validation via ValidatedCapabilityProvider trait

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use serde::{Deserialize, Serialize};

// Placeholder types for integration
use crate::runtime::{RuntimeResult, RuntimeError, Value};
use crate::ast::{TypeExpr, Expression, PrimitiveType, ParamType, Literal, Keyword};

// Helper functions for creating constrained types
impl CapabilityDescriptor {
    /// Helper to create a positive integer constraint: [:and int [:> 0]]
    pub fn positive_int_type() -> TypeExpr {
        TypeExpr::Intersection(vec![
            TypeExpr::Primitive(PrimitiveType::Int),
            TypeExpr::Literal(Literal::Keyword(Keyword::new("> 0")))
        ])
    }
    
    /// Helper to create an email string constraint: [:and string [:string-contains "@"]]
    pub fn email_string_type() -> TypeExpr {
        TypeExpr::Intersection(vec![
            TypeExpr::Primitive(PrimitiveType::String),
            TypeExpr::Literal(Literal::Keyword(Keyword::new("string-contains @")))
        ])
    }
    
    /// Helper to create a non-empty string constraint: [:and string [:string-min-length 1]]
    pub fn non_empty_string_type() -> TypeExpr {
        TypeExpr::Intersection(vec![
            TypeExpr::Primitive(PrimitiveType::String),
            TypeExpr::Literal(Literal::Keyword(Keyword::new("string-min-length 1")))
        ])
    }
    
    /// Helper to create a range-constrained integer: [:and int [:>= min] [:<= max]]
    pub fn range_int_type(min: i64, max: i64) -> TypeExpr {
        TypeExpr::Intersection(vec![
            TypeExpr::Primitive(PrimitiveType::Int),
            TypeExpr::Literal(Literal::Keyword(Keyword::new(&format!(">= {}", min)))),
            TypeExpr::Literal(Literal::Keyword(Keyword::new(&format!("<= {}", max))))
        ])
    }
    
    /// Helper to create a simple function type with constraints
    pub fn constrained_function_type(
        param_types: Vec<TypeExpr>,
        return_type: TypeExpr,
        variadic_param_type: Option<TypeExpr>
    ) -> TypeExpr {
        TypeExpr::Function {
            param_types: param_types.into_iter()
                .map(|t| ParamType::Simple(Box::new(t)))
                .collect(),
            variadic_param_type: variadic_param_type.map(Box::new),
            return_type: Box::new(return_type),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderMetadata {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub license: Option<String>,
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub max_memory: Option<u64>,
    pub max_cpu_time: Option<Duration>,
    pub max_disk_space: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkAccess {
    None,
    Limited(Vec<String>),
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Permission {
    FileRead(PathBuf),
    FileWrite(PathBuf),
    NetworkAccess(String),
    EnvironmentRead(String),
    SystemCommand(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityRequirements {
    pub permissions: Vec<Permission>,
    pub requires_microvm: bool,
    pub resource_limits: ResourceLimits,
    pub network_access: NetworkAccess,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityDescriptor {
    pub id: String,
    pub description: String,
    /// The capability's function type signature: (param_types...) -> return_type
    /// This should be a TypeExpr::Function variant for structural validation
    /// Use TypeExpr::Intersection ([:and Type Predicate]) for complex constraints
    pub capability_type: TypeExpr,
    pub security_requirements: SecurityRequirements,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ExecutionContext {
    // pub security_context: SecurityContext, // Uncomment and fix import if/when available
    pub trace_id: String,
    // pub environment: EnvironmentInfo, // Uncomment and fix import if/when available
    pub timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthStatus {
    Healthy,
    Degraded(String),
    Unhealthy(String),
}

pub trait CapabilityProvider: Send + Sync + std::fmt::Debug {
    fn provider_id(&self) -> &str;
    fn list_capabilities(&self) -> Vec<CapabilityDescriptor>;
    fn execute_capability(
        &self,
        capability_id: &str,
        inputs: &Value,
        context: &ExecutionContext,
    ) -> RuntimeResult<Value>;
    fn initialize(&mut self, config: &ProviderConfig) -> Result<(), String>;
    fn health_check(&self) -> HealthStatus;
    fn metadata(&self) -> ProviderMetadata;
}

/// Extension trait for CapabilityProvider that adds automatic type validation
pub trait ValidatedCapabilityProvider: CapabilityProvider {
    /// Execute capability with automatic input/output validation
    fn execute_capability_validated(
        &self,
        capability_id: &str,
        inputs: &[Value],
        context: &ExecutionContext,
    ) -> RuntimeResult<Value> {
        // Find the capability descriptor
        let capability = self.list_capabilities()
            .into_iter()
            .find(|c| c.id == capability_id)
            .ok_or_else(|| RuntimeError::Generic(format!("Capability not found: {}", capability_id)))?;
        
        // Validate inputs
        capability.validate_inputs(inputs)
            .map_err(|e| RuntimeError::Generic(format!("Input validation failed: {}", e)))?;
        
        // Convert inputs to the expected format for the provider
        let inputs_value = if inputs.len() == 1 {
            inputs[0].clone()
        } else {
            Value::Vector(inputs.to_vec())
        };
        
        // Execute the capability
        let result = self.execute_capability(capability_id, &inputs_value, context)?;
        
        // Validate output
        capability.validate_output(&result)
            .map_err(|e| RuntimeError::Generic(format!("Output validation failed: {}", e)))?;
        
        Ok(result)
    }
}

// Auto-implement the validated trait for all providers
impl<T: CapabilityProvider> ValidatedCapabilityProvider for T {}


// Placeholder for provider config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub config: Expression,
}

// Example implementations showing constrained capability types

/// Example: Email validation capability
/// Type: (email_string) -> validation_result
pub fn email_validation_capability() -> CapabilityDescriptor {
    CapabilityDescriptor {
        id: "email.validate".to_string(),
        description: "Validates email address format".to_string(),
        capability_type: CapabilityDescriptor::constrained_function_type(
            vec![CapabilityDescriptor::email_string_type()],
            TypeExpr::Union(vec![
                TypeExpr::Literal(Literal::Keyword(Keyword::new("valid"))),
                TypeExpr::Literal(Literal::Keyword(Keyword::new("invalid")))
            ]),
            None
        ),
        security_requirements: SecurityRequirements {
            permissions: vec![],
            requires_microvm: false,
            resource_limits: ResourceLimits {
                max_memory: Some(1024 * 1024), // 1MB
                max_cpu_time: Some(Duration::from_millis(100)),
                max_disk_space: None,
            },
            network_access: NetworkAccess::None,
        },
        metadata: HashMap::new(),
    }
}

/// Example: Mathematical operation with positive integers
/// Type: (positive_int, positive_int) -> positive_int
pub fn positive_int_math_capability() -> CapabilityDescriptor {
    CapabilityDescriptor {
        id: "math.multiply-positive".to_string(),
        description: "Multiplies two positive integers".to_string(),
        capability_type: CapabilityDescriptor::constrained_function_type(
            vec![
                CapabilityDescriptor::positive_int_type(),
                CapabilityDescriptor::positive_int_type()
            ],
            CapabilityDescriptor::positive_int_type(),
            None
        ),
        security_requirements: SecurityRequirements {
            permissions: vec![],
            requires_microvm: false,
            resource_limits: ResourceLimits {
                max_memory: Some(512 * 1024), // 512KB
                max_cpu_time: Some(Duration::from_millis(50)),
                max_disk_space: None,
            },
            network_access: NetworkAccess::None,
        },
        metadata: HashMap::new(),
    }
}

/// Example: Age validation capability
/// Type: (age_int) -> validation_result
pub fn age_validation_capability() -> CapabilityDescriptor {
    CapabilityDescriptor {
        id: "person.validate-age".to_string(),
        description: "Validates person age (0-150)".to_string(),
        capability_type: CapabilityDescriptor::constrained_function_type(
            vec![CapabilityDescriptor::range_int_type(0, 150)],
            TypeExpr::Union(vec![
                TypeExpr::Literal(Literal::Keyword(Keyword::new("valid"))),
                TypeExpr::Literal(Literal::Keyword(Keyword::new("invalid")))
            ]),
            None
        ),
        security_requirements: SecurityRequirements {
            permissions: vec![],
            requires_microvm: false,
            resource_limits: ResourceLimits {
                max_memory: Some(256 * 1024), // 256KB
                max_cpu_time: Some(Duration::from_millis(25)),
                max_disk_space: None,
            },
            network_access: NetworkAccess::None,
        },
        metadata: HashMap::new(),
    }
}

/// Utility functions for type validation at runtime
impl CapabilityDescriptor {
    /// Validates a value against a TypeExpr constraint
    /// This is a basic implementation - a full implementation would integrate with RTFS's type checker
    pub fn validate_against_type(&self, value: &Value, type_expr: &TypeExpr) -> Result<(), String> {
        match type_expr {
            TypeExpr::Primitive(PrimitiveType::Int) => {
                match value {
                    Value::Integer(_) => Ok(()),
                    _ => Err(format!("Expected integer, got {:?}", value))
                }
            }
            TypeExpr::Primitive(PrimitiveType::String) => {
                match value {
                    Value::String(_) => Ok(()),
                    _ => Err(format!("Expected string, got {:?}", value))
                }
            }
            TypeExpr::Intersection(types) => {
                // All intersection types must be satisfied
                for constraint_type in types {
                    self.validate_against_type(value, constraint_type)?;
                }
                Ok(())
            }
            TypeExpr::Union(types) => {
                // At least one union type must be satisfied
                for union_type in types {
                    if self.validate_against_type(value, union_type).is_ok() {
                        return Ok(());
                    }
                }
                Err(format!("Value {:?} doesn't match any union type", value))
            }
            TypeExpr::Function { .. } => {
                // For function types, we'd need to validate the callable
                // This is a placeholder for actual function type validation
                Ok(())
            }
            _ => {
                // For other types, return Ok for now
                // A full implementation would handle all type variants
                Ok(())
            }
        }
    }
    
    /// Validates capability inputs against the function parameter types
    pub fn validate_inputs(&self, inputs: &[Value]) -> Result<(), String> {
        if let TypeExpr::Function { param_types, .. } = &self.capability_type {
            if inputs.len() != param_types.len() {
                return Err(format!(
                    "Expected {} parameters, got {}",
                    param_types.len(),
                    inputs.len()
                ));
            }
            
            for (i, (input, param_type)) in inputs.iter().zip(param_types.iter()).enumerate() {
                let ParamType::Simple(type_expr) = param_type;
                self.validate_against_type(input, type_expr)
                    .map_err(|e| format!("Parameter {}: {}", i, e))?;
            }
            
            Ok(())
        } else {
            Err("Capability type is not a function".to_string())
        }
    }
    
    /// Validates capability output against the return type
    pub fn validate_output(&self, output: &Value) -> Result<(), String> {
        if let TypeExpr::Function { return_type, .. } = &self.capability_type {
            self.validate_against_type(output, return_type)
                .map_err(|e| format!("Return value: {}", e))
        } else {
            Err("Capability type is not a function".to_string())
        }
    }
}
