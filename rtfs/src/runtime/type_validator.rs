use crate::ast::{
    ArrayDimension, Keyword, Literal, MapKey, PrimitiveType, TypeExpr, TypePredicate,
};
use crate::runtime::values::Value;
use regex::Regex;
use std::collections::HashMap;

/// Validation error types
#[derive(Debug, Clone)]
pub enum ValidationError {
    TypeMismatch {
        expected: TypeExpr,
        actual: String,
        path: String,
    },
    PredicateViolation {
        predicate: String,
        value: String,
        path: String,
    },
    ShapeViolation {
        expected_shape: Vec<ArrayDimension>,
        actual_shape: Vec<usize>,
        path: String,
    },
    InvalidRegexPattern(String),
    MissingRequiredKey {
        key: Keyword,
        path: String,
    },
    UnknownPredicate(String),
}

/// Type checking configuration for optimization
#[derive(Debug, Clone)]
pub struct TypeCheckingConfig {
    /// Skip runtime validation for types verified at compile-time
    pub skip_compile_time_verified: bool,
    /// Always validate at capability boundaries regardless of compile-time verification
    pub enforce_capability_boundaries: bool,
    /// Always validate data from external sources
    pub validate_external_data: bool,
    /// Validation level: Basic, Standard, Strict
    pub validation_level: ValidationLevel,
}

impl Default for TypeCheckingConfig {
    fn default() -> Self {
        Self {
            skip_compile_time_verified: false, // Conservative default
            enforce_capability_boundaries: true,
            validate_external_data: true,
            validation_level: ValidationLevel::Standard,
        }
    }
}

/// Validation levels for performance/safety tradeoffs
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationLevel {
    /// Only validate types, skip all predicates
    Basic,
    /// Validate types and security-critical predicates
    Standard,
    /// Validate all types and predicates
    Strict,
}

/// Context for type verification decisions
#[derive(Debug, Clone)]
pub struct VerificationContext {
    pub compile_time_verified: bool,
    pub is_capability_boundary: bool,
    pub is_external_data: bool,
    pub source_location: Option<String>,
    pub trust_level: TrustLevel,
}

impl Default for VerificationContext {
    fn default() -> Self {
        Self {
            compile_time_verified: false,
            is_capability_boundary: false,
            is_external_data: false,
            source_location: None,
            trust_level: TrustLevel::Untrusted,
        }
    }
}

impl VerificationContext {
    /// Create context for capability boundary validation
    pub fn capability_boundary(capability_id: &str) -> Self {
        Self {
            compile_time_verified: false, // Force validation at boundaries
            is_capability_boundary: true,
            is_external_data: false,
            source_location: Some(format!("capability:{}", capability_id)),
            trust_level: TrustLevel::Untrusted,
        }
    }

    /// Create context for external data validation
    pub fn external_data(source: &str) -> Self {
        Self {
            compile_time_verified: false,
            is_capability_boundary: false,
            is_external_data: true,
            source_location: Some(source.to_string()),
            trust_level: TrustLevel::Untrusted,
        }
    }

    /// Create context for compile-time verified types
    pub fn compile_time_verified() -> Self {
        Self {
            compile_time_verified: true,
            is_capability_boundary: false,
            is_external_data: false,
            source_location: None,
            trust_level: TrustLevel::Trusted,
        }
    }

    /// Check if this context should skip runtime validation
    pub fn should_skip_validation(&self, config: &TypeCheckingConfig) -> bool {
        // Never skip validation at capability boundaries
        if self.is_capability_boundary && config.enforce_capability_boundaries {
            return false;
        }

        // Never skip validation for external data
        if self.is_external_data && config.validate_external_data {
            return false;
        }

        // Skip if compile-time verified and optimization enabled
        config.skip_compile_time_verified && self.compile_time_verified
    }
}

/// Trust levels for validation decisions
#[derive(Debug, Clone, PartialEq)]
pub enum TrustLevel {
    /// Local expressions with static types
    Trusted,
    /// Function calls with known signatures
    Verified,
    /// Capability calls or external data
    Untrusted,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::TypeMismatch {
                expected,
                actual,
                path,
            } => {
                write!(
                    f,
                    "Type mismatch at {}: expected {}, got {}",
                    path, expected, actual
                )
            }
            ValidationError::PredicateViolation {
                predicate,
                value,
                path,
            } => {
                write!(
                    f,
                    "Predicate violation at {}: {} failed for value {}",
                    path, predicate, value
                )
            }
            ValidationError::ShapeViolation {
                expected_shape,
                actual_shape,
                path,
            } => {
                write!(
                    f,
                    "Shape violation at {}: expected {:?}, got {:?}",
                    path, expected_shape, actual_shape
                )
            }
            ValidationError::InvalidRegexPattern(pattern) => {
                write!(f, "Invalid regex pattern: {}", pattern)
            }
            ValidationError::MissingRequiredKey { key, path } => {
                write!(f, "Missing required key :{} at {}", key.0, path)
            }
            ValidationError::UnknownPredicate(pred) => {
                write!(f, "Unknown predicate: {}", pred)
            }
        }
    }
}

impl std::error::Error for ValidationError {}

pub type ValidationResult<T> = Result<T, ValidationError>;

/// Type validator with standard library predicates
pub struct TypeValidator {
    stdlib_predicates: HashMap<String, Box<dyn Fn(&Value) -> bool + Send + Sync>>,
}

impl std::fmt::Debug for TypeValidator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypeValidator")
            .field(
                "stdlib_predicates",
                &format!("{} predicates", self.stdlib_predicates.len()),
            )
            .finish()
    }
}

impl TypeValidator {
    pub fn new() -> Self {
        let mut validator = Self {
            stdlib_predicates: HashMap::new(),
        };
        validator.register_stdlib_predicates();
        validator
    }

    /// Register built-in standard library predicates
    fn register_stdlib_predicates(&mut self) {
        // URL validation predicate
        self.stdlib_predicates.insert(
            "is-url".to_string(),
            Box::new(|value| {
                if let Value::String(s) = value {
                    // CCOS dependency: url::Url::parse
                    // Simple URL validation for standalone RTFS (check for http/https prefix)
                    s.starts_with("http://") || s.starts_with("https://")
                } else {
                    false
                }
            }),
        );

        // Email validation predicate
        self.stdlib_predicates.insert(
            "is-email".to_string(),
            Box::new(|value| {
                if let Value::String(s) = value {
                    // Simple email regex - in production, use a proper email validation library
                    let email_regex =
                        regex::Regex::new(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$")
                            .unwrap();
                    email_regex.is_match(s)
                } else {
                    false
                }
            }),
        );
    }

    /// Main validation entry point (legacy - always strict)
    pub fn validate_value(&self, value: &Value, rtfs_type: &TypeExpr) -> ValidationResult<()> {
        let config = TypeCheckingConfig::default();
        let context = VerificationContext::default();
        self.validate_with_config(value, rtfs_type, &config, &context)
    }

    /// Optimized validation with configuration and context
    pub fn validate_with_config(
        &self,
        value: &Value,
        rtfs_type: &TypeExpr,
        config: &TypeCheckingConfig,
        context: &VerificationContext,
    ) -> ValidationResult<()> {
        // Check if we can skip validation based on compile-time verification
        if context.should_skip_validation(config) && self.is_simple_type(rtfs_type) {
            return Ok(()); // Skip runtime validation for compile-time verified simple types
        }

        // Apply validation level
        match config.validation_level {
            ValidationLevel::Basic => self.validate_basic_type(value, rtfs_type, ""),
            ValidationLevel::Standard => {
                self.validate_with_security_predicates(value, rtfs_type, context, "")
            }
            ValidationLevel::Strict => self.validate_value_at_path(value, rtfs_type, ""),
        }
    }

    /// Check if a type is simple enough for compile-time verification
    fn is_simple_type(&self, rtfs_type: &TypeExpr) -> bool {
        match rtfs_type {
            // Primitive types are simple
            TypeExpr::Primitive(_) => true,
            // Collections of simple types are simple
            TypeExpr::Vector(element_type) => self.is_simple_type(element_type),
            TypeExpr::Map { entries, .. } => entries
                .iter()
                .all(|entry| self.is_simple_type(&entry.value_type)),
            TypeExpr::Tuple(types) => types.iter().all(|t| self.is_simple_type(t)),
            // Refined types are NOT simple (need predicate validation)
            TypeExpr::Refined { .. } => false,
            // Arrays with shape constraints need validation
            TypeExpr::Array { .. } => false,
            // Union and optional types depend on their components
            TypeExpr::Union(types) => types.iter().all(|t| self.is_simple_type(t)),
            TypeExpr::Optional(inner) => self.is_simple_type(inner),
            // Enums are simple
            TypeExpr::Enum(_) => true,
            // Functions and other complex types are not simple
            _ => false,
        }
    }

    /// Basic type validation (types only, no predicates)
    fn validate_basic_type(
        &self,
        value: &Value,
        rtfs_type: &TypeExpr,
        path: &str,
    ) -> ValidationResult<()> {
        match (value, rtfs_type) {
            // Primitive types
            (Value::Integer(_), TypeExpr::Primitive(PrimitiveType::Int)) => Ok(()),
            (Value::Float(_), TypeExpr::Primitive(PrimitiveType::Float)) => Ok(()),
            (Value::String(_), TypeExpr::Primitive(PrimitiveType::String)) => Ok(()),
            (Value::Boolean(_), TypeExpr::Primitive(PrimitiveType::Bool)) => Ok(()),
            (Value::Nil, TypeExpr::Primitive(PrimitiveType::Nil)) => Ok(()),
            (Value::Keyword(_), TypeExpr::Primitive(PrimitiveType::Keyword)) => Ok(()),
            (Value::Symbol(_), TypeExpr::Primitive(PrimitiveType::Symbol)) => Ok(()),

            // Any type accepts everything
            (_, TypeExpr::Any) => Ok(()),

            // Collections - basic structure only
            (Value::Vector(items), TypeExpr::Vector(element_type)) => {
                for (i, item) in items.iter().enumerate() {
                    self.validate_basic_type(item, element_type, &format!("{}[{}]", path, i))?;
                }
                Ok(())
            }

            // For refined types in basic mode, just validate the base type
            (value, TypeExpr::Refined { base_type, .. }) => {
                self.validate_basic_type(value, base_type, path)
            }

            // Other types get full validation even in basic mode
            _ => self.validate_value_at_path(value, rtfs_type, path),
        }
    }

    /// Security-focused validation (types + security-critical predicates only)
    fn validate_with_security_predicates(
        &self,
        value: &Value,
        rtfs_type: &TypeExpr,
        _context: &VerificationContext,
        path: &str,
    ) -> ValidationResult<()> {
        // First validate basic type
        self.validate_basic_type(value, rtfs_type, path)?;

        // Then validate predicates
        match rtfs_type {
            TypeExpr::Refined {
                base_type: _,
                predicates,
            } => {
                // For refined types, ALL predicates define the type and must be validated
                for predicate in predicates {
                    self.validate_predicate(value, predicate, path)?;
                }
                Ok(())
            }
            _ => Ok(()), // No additional validation needed for non-refined types
        }
    }

    /// Validate value with path tracking for better error messages
    pub fn validate_value_at_path(
        &self,
        value: &Value,
        rtfs_type: &TypeExpr,
        path: &str,
    ) -> ValidationResult<()> {
        match (value, rtfs_type) {
            // Primitive types
            (Value::Integer(_), TypeExpr::Primitive(PrimitiveType::Int)) => Ok(()),
            (Value::Float(_), TypeExpr::Primitive(PrimitiveType::Float)) => Ok(()),
            (Value::String(_), TypeExpr::Primitive(PrimitiveType::String)) => Ok(()),
            (Value::Boolean(_), TypeExpr::Primitive(PrimitiveType::Bool)) => Ok(()),
            (Value::Nil, TypeExpr::Primitive(PrimitiveType::Nil)) => Ok(()),
            (Value::Keyword(_), TypeExpr::Primitive(PrimitiveType::Keyword)) => Ok(()),
            (Value::Symbol(_), TypeExpr::Primitive(PrimitiveType::Symbol)) => Ok(()),

            // Any type accepts everything
            (_, TypeExpr::Any) => Ok(()),

            // Never type accepts nothing
            (_, TypeExpr::Never) => Err(ValidationError::TypeMismatch {
                expected: rtfs_type.clone(),
                actual: value.type_name().to_string(),
                path: path.to_string(),
            }),

            // Optional types
            (Value::Nil, TypeExpr::Optional(_)) => Ok(()), // nil is always valid for optional
            (val, TypeExpr::Optional(inner)) => self.validate_value_at_path(val, inner, path),

            // Vector types
            (Value::Vector(items), TypeExpr::Vector(element_type)) => {
                for (i, item) in items.iter().enumerate() {
                    let item_path = format!("{}[{}]", path, i);
                    self.validate_value_at_path(item, element_type, &item_path)?;
                }
                Ok(())
            }

            // Array types with shape validation
            (
                Value::Vector(items),
                TypeExpr::Array {
                    element_type,
                    shape,
                },
            ) => {
                // Validate shape
                self.validate_array_shape(items, shape, path)?;

                // Validate element types
                for (i, item) in items.iter().enumerate() {
                    let item_path = format!("{}[{}]", path, i);
                    self.validate_value_at_path(item, element_type, &item_path)?;
                }
                Ok(())
            }

            // Tuple types
            (Value::Vector(items), TypeExpr::Tuple(types)) => {
                if items.len() != types.len() {
                    return Err(ValidationError::TypeMismatch {
                        expected: rtfs_type.clone(),
                        actual: format!("tuple of length {}", items.len()),
                        path: path.to_string(),
                    });
                }

                for (i, (item, expected_type)) in items.iter().zip(types.iter()).enumerate() {
                    let item_path = format!("{}[{}]", path, i);
                    self.validate_value_at_path(item, expected_type, &item_path)?;
                }
                Ok(())
            }

            // Map types
            (
                Value::Map(map),
                TypeExpr::Map {
                    entries,
                    wildcard: _,
                },
            ) => {
                for entry in entries {
                    let key = MapKey::Keyword(entry.key.clone());
                    if let Some(value) = map.get(&key) {
                        let field_path = format!("{}.{}", path, entry.key.0);
                        self.validate_value_at_path(value, &entry.value_type, &field_path)?;
                    } else if !entry.optional {
                        return Err(ValidationError::MissingRequiredKey {
                            key: entry.key.clone(),
                            path: path.to_string(),
                        });
                    }
                }
                Ok(())
            }

            // Union types - value must match at least one type
            (val, TypeExpr::Union(types)) => {
                for union_type in types {
                    if self.validate_value_at_path(val, union_type, path).is_ok() {
                        return Ok(());
                    }
                }
                Err(ValidationError::TypeMismatch {
                    expected: rtfs_type.clone(),
                    actual: value.type_name().to_string(),
                    path: path.to_string(),
                })
            }

            // Enum types
            (val, TypeExpr::Enum(allowed_values)) => {
                let val_as_literal = value_to_literal(val)?;
                if allowed_values.contains(&val_as_literal) {
                    Ok(())
                } else {
                    Err(ValidationError::TypeMismatch {
                        expected: rtfs_type.clone(),
                        actual: format!("{:?}", val_as_literal),
                        path: path.to_string(),
                    })
                }
            }

            // Refined types (base type + predicates)
            (
                val,
                TypeExpr::Refined {
                    base_type,
                    predicates,
                },
            ) => {
                // First validate against base type
                self.validate_value_at_path(val, base_type, path)?;

                // Then validate all predicates
                for predicate in predicates {
                    self.validate_predicate(val, predicate, path)?;
                }
                Ok(())
            }

            // Intersection types - value must match ALL types
            (val, TypeExpr::Intersection(types)) => {
                for intersect_type in types {
                    self.validate_value_at_path(val, intersect_type, path)?;
                }
                Ok(())
            }

            // Literal types
            (val, TypeExpr::Literal(expected_literal)) => {
                let val_as_literal = value_to_literal(val)?;
                if val_as_literal == *expected_literal {
                    Ok(())
                } else {
                    Err(ValidationError::TypeMismatch {
                        expected: rtfs_type.clone(),
                        actual: format!("{:?}", val_as_literal),
                        path: path.to_string(),
                    })
                }
            }

            // Type mismatch
            _ => Err(ValidationError::TypeMismatch {
                expected: rtfs_type.clone(),
                actual: value.type_name().to_string(),
                path: path.to_string(),
            }),
        }
    }

    /// Validate array shape constraints
    fn validate_array_shape(
        &self,
        items: &[Value],
        shape: &[ArrayDimension],
        path: &str,
    ) -> ValidationResult<()> {
        if shape.is_empty() {
            return Ok(()); // No shape constraints
        }

        // For now, only validate 1D arrays (can be extended for N-D)
        if shape.len() == 1 {
            match &shape[0] {
                ArrayDimension::Fixed(expected_size) => {
                    if items.len() != *expected_size {
                        return Err(ValidationError::ShapeViolation {
                            expected_shape: shape.to_vec(),
                            actual_shape: vec![items.len()],
                            path: path.to_string(),
                        });
                    }
                }
                ArrayDimension::Variable => {
                    // Variable size - any length is acceptable
                }
            }
        }

        Ok(())
    }

    /// Validate a predicate against a value
    fn validate_predicate(
        &self,
        value: &Value,
        predicate: &TypePredicate,
        path: &str,
    ) -> ValidationResult<()> {
        match predicate {
            TypePredicate::GreaterThan(threshold) => match (value, threshold) {
                (Value::Integer(v), Literal::Integer(t)) => {
                    if v > t {
                        Ok(())
                    } else {
                        Err(ValidationError::PredicateViolation {
                            predicate: format!("> {}", t),
                            value: v.to_string(),
                            path: path.to_string(),
                        })
                    }
                }
                (Value::Float(v), Literal::Float(t)) => {
                    if v > t {
                        Ok(())
                    } else {
                        Err(ValidationError::PredicateViolation {
                            predicate: format!("> {}", t),
                            value: v.to_string(),
                            path: path.to_string(),
                        })
                    }
                }
                _ => Err(ValidationError::PredicateViolation {
                    predicate: "type mismatch for comparison".to_string(),
                    value: value.type_name().to_string(),
                    path: path.to_string(),
                }),
            },

            TypePredicate::GreaterEqual(threshold) => match (value, threshold) {
                (Value::Integer(v), Literal::Integer(t)) => {
                    if v >= t {
                        Ok(())
                    } else {
                        Err(ValidationError::PredicateViolation {
                            predicate: format!(">= {}", t),
                            value: v.to_string(),
                            path: path.to_string(),
                        })
                    }
                }
                (Value::Float(v), Literal::Float(t)) => {
                    if v >= t {
                        Ok(())
                    } else {
                        Err(ValidationError::PredicateViolation {
                            predicate: format!(">= {}", t),
                            value: v.to_string(),
                            path: path.to_string(),
                        })
                    }
                }
                _ => Err(ValidationError::PredicateViolation {
                    predicate: "type mismatch for comparison".to_string(),
                    value: value.type_name().to_string(),
                    path: path.to_string(),
                }),
            },

            TypePredicate::LessThan(threshold) => match (value, threshold) {
                (Value::Integer(v), Literal::Integer(t)) => {
                    if v < t {
                        Ok(())
                    } else {
                        Err(ValidationError::PredicateViolation {
                            predicate: format!("< {}", t),
                            value: v.to_string(),
                            path: path.to_string(),
                        })
                    }
                }
                (Value::Float(v), Literal::Float(t)) => {
                    if v < t {
                        Ok(())
                    } else {
                        Err(ValidationError::PredicateViolation {
                            predicate: format!("< {}", t),
                            value: v.to_string(),
                            path: path.to_string(),
                        })
                    }
                }
                _ => Err(ValidationError::PredicateViolation {
                    predicate: "type mismatch for comparison".to_string(),
                    value: value.type_name().to_string(),
                    path: path.to_string(),
                }),
            },

            TypePredicate::LessEqual(threshold) => match (value, threshold) {
                (Value::Integer(v), Literal::Integer(t)) => {
                    if v <= t {
                        Ok(())
                    } else {
                        Err(ValidationError::PredicateViolation {
                            predicate: format!("<= {}", t),
                            value: v.to_string(),
                            path: path.to_string(),
                        })
                    }
                }
                (Value::Float(v), Literal::Float(t)) => {
                    if v <= t {
                        Ok(())
                    } else {
                        Err(ValidationError::PredicateViolation {
                            predicate: format!("<= {}", t),
                            value: v.to_string(),
                            path: path.to_string(),
                        })
                    }
                }
                _ => Err(ValidationError::PredicateViolation {
                    predicate: "type mismatch for comparison".to_string(),
                    value: value.type_name().to_string(),
                    path: path.to_string(),
                }),
            },

            TypePredicate::Equal(expected) => {
                let val_as_literal = value_to_literal(value)?;
                if val_as_literal == *expected {
                    Ok(())
                } else {
                    Err(ValidationError::PredicateViolation {
                        predicate: format!("= {:?}", expected),
                        value: format!("{:?}", val_as_literal),
                        path: path.to_string(),
                    })
                }
            }

            TypePredicate::NotEqual(expected) => {
                let val_as_literal = value_to_literal(value)?;
                if val_as_literal != *expected {
                    Ok(())
                } else {
                    Err(ValidationError::PredicateViolation {
                        predicate: format!("!= {:?}", expected),
                        value: format!("{:?}", val_as_literal),
                        path: path.to_string(),
                    })
                }
            }

            TypePredicate::InRange(min, max) => match (value, min, max) {
                (Value::Integer(v), Literal::Integer(min_val), Literal::Integer(max_val)) => {
                    if v >= min_val && v <= max_val {
                        Ok(())
                    } else {
                        Err(ValidationError::PredicateViolation {
                            predicate: format!("in-range {} {}", min_val, max_val),
                            value: v.to_string(),
                            path: path.to_string(),
                        })
                    }
                }
                (Value::Float(v), Literal::Float(min_val), Literal::Float(max_val)) => {
                    if v >= min_val && v <= max_val {
                        Ok(())
                    } else {
                        Err(ValidationError::PredicateViolation {
                            predicate: format!("in-range {} {}", min_val, max_val),
                            value: v.to_string(),
                            path: path.to_string(),
                        })
                    }
                }
                _ => Err(ValidationError::PredicateViolation {
                    predicate: "type mismatch for range".to_string(),
                    value: value.type_name().to_string(),
                    path: path.to_string(),
                }),
            },

            TypePredicate::MinLength(min_len) => {
                if let Value::String(s) = value {
                    if s.len() >= *min_len {
                        Ok(())
                    } else {
                        Err(ValidationError::PredicateViolation {
                            predicate: format!("min-length {}", min_len),
                            value: format!("length {}", s.len()),
                            path: path.to_string(),
                        })
                    }
                } else {
                    Err(ValidationError::PredicateViolation {
                        predicate: "min-length requires string".to_string(),
                        value: value.type_name().to_string(),
                        path: path.to_string(),
                    })
                }
            }

            TypePredicate::MaxLength(max_len) => {
                if let Value::String(s) = value {
                    if s.len() <= *max_len {
                        Ok(())
                    } else {
                        Err(ValidationError::PredicateViolation {
                            predicate: format!("max-length {}", max_len),
                            value: format!("length {}", s.len()),
                            path: path.to_string(),
                        })
                    }
                } else {
                    Err(ValidationError::PredicateViolation {
                        predicate: "max-length requires string".to_string(),
                        value: value.type_name().to_string(),
                        path: path.to_string(),
                    })
                }
            }

            TypePredicate::Length(expected_len) => {
                if let Value::String(s) = value {
                    if s.len() == *expected_len {
                        Ok(())
                    } else {
                        Err(ValidationError::PredicateViolation {
                            predicate: format!("length {}", expected_len),
                            value: format!("length {}", s.len()),
                            path: path.to_string(),
                        })
                    }
                } else {
                    Err(ValidationError::PredicateViolation {
                        predicate: "length requires string".to_string(),
                        value: value.type_name().to_string(),
                        path: path.to_string(),
                    })
                }
            }

            TypePredicate::MatchesRegex(pattern) => {
                if let Value::String(s) = value {
                    let regex = Regex::new(pattern)
                        .map_err(|_| ValidationError::InvalidRegexPattern(pattern.clone()))?;
                    if regex.is_match(s) {
                        Ok(())
                    } else {
                        Err(ValidationError::PredicateViolation {
                            predicate: format!("matches-regex \"{}\"", pattern),
                            value: s.clone(),
                            path: path.to_string(),
                        })
                    }
                } else {
                    Err(ValidationError::PredicateViolation {
                        predicate: "matches-regex requires string".to_string(),
                        value: value.type_name().to_string(),
                        path: path.to_string(),
                    })
                }
            }

            TypePredicate::IsUrl => {
                if let Some(predicate_fn) = self.stdlib_predicates.get("is-url") {
                    if predicate_fn(value) {
                        Ok(())
                    } else {
                        Err(ValidationError::PredicateViolation {
                            predicate: "is-url".to_string(),
                            value: format!("{:?}", value),
                            path: path.to_string(),
                        })
                    }
                } else {
                    Err(ValidationError::UnknownPredicate("is-url".to_string()))
                }
            }

            TypePredicate::IsEmail => {
                if let Some(predicate_fn) = self.stdlib_predicates.get("is-email") {
                    if predicate_fn(value) {
                        Ok(())
                    } else {
                        Err(ValidationError::PredicateViolation {
                            predicate: "is-email".to_string(),
                            value: format!("{:?}", value),
                            path: path.to_string(),
                        })
                    }
                } else {
                    Err(ValidationError::UnknownPredicate("is-email".to_string()))
                }
            }

            TypePredicate::MinCount(min_count) => {
                let actual_count = match value {
                    Value::Vector(v) => v.len(),
                    Value::Map(m) => m.len(),
                    _ => {
                        return Err(ValidationError::PredicateViolation {
                            predicate: "min-count requires collection".to_string(),
                            value: value.type_name().to_string(),
                            path: path.to_string(),
                        })
                    }
                };

                if actual_count >= *min_count {
                    Ok(())
                } else {
                    Err(ValidationError::PredicateViolation {
                        predicate: format!("min-count {}", min_count),
                        value: format!("count {}", actual_count),
                        path: path.to_string(),
                    })
                }
            }

            TypePredicate::MaxCount(max_count) => {
                let actual_count = match value {
                    Value::Vector(v) => v.len(),
                    Value::Map(m) => m.len(),
                    _ => {
                        return Err(ValidationError::PredicateViolation {
                            predicate: "max-count requires collection".to_string(),
                            value: value.type_name().to_string(),
                            path: path.to_string(),
                        })
                    }
                };

                if actual_count <= *max_count {
                    Ok(())
                } else {
                    Err(ValidationError::PredicateViolation {
                        predicate: format!("max-count {}", max_count),
                        value: format!("count {}", actual_count),
                        path: path.to_string(),
                    })
                }
            }

            TypePredicate::Count(expected_count) => {
                let actual_count = match value {
                    Value::Vector(v) => v.len(),
                    Value::Map(m) => m.len(),
                    _ => {
                        return Err(ValidationError::PredicateViolation {
                            predicate: "count requires collection".to_string(),
                            value: value.type_name().to_string(),
                            path: path.to_string(),
                        })
                    }
                };

                if actual_count == *expected_count {
                    Ok(())
                } else {
                    Err(ValidationError::PredicateViolation {
                        predicate: format!("count {}", expected_count),
                        value: format!("count {}", actual_count),
                        path: path.to_string(),
                    })
                }
            }

            TypePredicate::NonEmpty => {
                let is_empty = match value {
                    Value::Vector(v) => v.is_empty(),
                    Value::Map(m) => m.is_empty(),
                    Value::String(s) => s.is_empty(),
                    _ => {
                        return Err(ValidationError::PredicateViolation {
                            predicate: "non-empty requires collection or string".to_string(),
                            value: value.type_name().to_string(),
                            path: path.to_string(),
                        })
                    }
                };

                if !is_empty {
                    Ok(())
                } else {
                    Err(ValidationError::PredicateViolation {
                        predicate: "non-empty".to_string(),
                        value: "empty".to_string(),
                        path: path.to_string(),
                    })
                }
            }

            TypePredicate::HasKey(required_key) => {
                if let Value::Map(map) = value {
                    let key = MapKey::Keyword(required_key.clone());
                    if map.contains_key(&key) {
                        Ok(())
                    } else {
                        Err(ValidationError::MissingRequiredKey {
                            key: required_key.clone(),
                            path: path.to_string(),
                        })
                    }
                } else {
                    Err(ValidationError::PredicateViolation {
                        predicate: "has-key requires map".to_string(),
                        value: value.type_name().to_string(),
                        path: path.to_string(),
                    })
                }
            }

            TypePredicate::RequiredKeys(required_keys) => {
                if let Value::Map(map) = value {
                    for required_key in required_keys {
                        let key = MapKey::Keyword(required_key.clone());
                        if !map.contains_key(&key) {
                            return Err(ValidationError::MissingRequiredKey {
                                key: required_key.clone(),
                                path: path.to_string(),
                            });
                        }
                    }
                    Ok(())
                } else {
                    Err(ValidationError::PredicateViolation {
                        predicate: "required-keys requires map".to_string(),
                        value: value.type_name().to_string(),
                        path: path.to_string(),
                    })
                }
            }

            TypePredicate::Custom(name, _args) => {
                Err(ValidationError::UnknownPredicate(name.0.clone()))
            }
        }
    }
}

impl Default for TypeValidator {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert a runtime Value to a Literal for type checking
fn value_to_literal(value: &Value) -> ValidationResult<Literal> {
    match value {
        Value::Integer(i) => Ok(Literal::Integer(*i)),
        Value::Float(f) => Ok(Literal::Float(*f)),
        Value::String(s) => Ok(Literal::String(s.clone())),
        Value::Boolean(b) => Ok(Literal::Boolean(*b)),
        Value::Keyword(k) => Ok(Literal::Keyword(k.clone())),
        Value::Nil => Ok(Literal::Nil),
        Value::Timestamp(t) => Ok(Literal::Timestamp(t.clone())),
        Value::Uuid(u) => Ok(Literal::Uuid(u.clone())),
        Value::ResourceHandle(r) => Ok(Literal::ResourceHandle(r.clone())),
        _ => Err(ValidationError::TypeMismatch {
            expected: TypeExpr::Any, // Placeholder
            actual: format!("Cannot convert {} to literal", value.type_name()),
            path: "".to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Keyword;

    #[test]
    fn test_primitive_type_validation() {
        let validator = TypeValidator::new();

        // Integer validation
        assert!(validator
            .validate_value(
                &Value::Integer(42),
                &TypeExpr::Primitive(PrimitiveType::Int)
            )
            .is_ok());

        // String validation
        assert!(validator
            .validate_value(
                &Value::String("hello".to_string()),
                &TypeExpr::Primitive(PrimitiveType::String)
            )
            .is_ok());

        // Type mismatch
        assert!(validator
            .validate_value(
                &Value::String("hello".to_string()),
                &TypeExpr::Primitive(PrimitiveType::Int)
            )
            .is_err());
    }

    #[test]
    fn test_refined_type_validation() {
        let validator = TypeValidator::new();

        // Positive integer constraint
        let positive_int = TypeExpr::Refined {
            base_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
            predicates: vec![TypePredicate::GreaterThan(Literal::Integer(0))],
        };

        assert!(validator
            .validate_value(&Value::Integer(5), &positive_int)
            .is_ok());
        assert!(validator
            .validate_value(&Value::Integer(0), &positive_int)
            .is_err());
        assert!(validator
            .validate_value(&Value::Integer(-1), &positive_int)
            .is_err());
    }

    #[test]
    fn test_string_length_validation() {
        let validator = TypeValidator::new();

        // String with minimum length constraint
        let min_length_string = TypeExpr::Refined {
            base_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
            predicates: vec![TypePredicate::MinLength(3)],
        };

        assert!(validator
            .validate_value(&Value::String("hello".to_string()), &min_length_string)
            .is_ok());
        assert!(validator
            .validate_value(&Value::String("hi".to_string()), &min_length_string)
            .is_err());
    }

    #[test]
    fn test_enum_validation() {
        let validator = TypeValidator::new();

        // Color enum
        let color_enum = TypeExpr::Enum(vec![
            Literal::Keyword(Keyword::new("red")),
            Literal::Keyword(Keyword::new("green")),
            Literal::Keyword(Keyword::new("blue")),
        ]);

        assert!(validator
            .validate_value(&Value::Keyword(Keyword::new("red")), &color_enum)
            .is_ok());
        assert!(validator
            .validate_value(&Value::Keyword(Keyword::new("yellow")), &color_enum)
            .is_err());
    }

    #[test]
    fn test_array_shape_validation() {
        let validator = TypeValidator::new();

        // Fixed size array [3]
        let fixed_array = TypeExpr::Array {
            element_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
            shape: vec![ArrayDimension::Fixed(3)],
        };

        let valid_array = Value::Vector(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
        ]);

        let invalid_array = Value::Vector(vec![Value::Integer(1), Value::Integer(2)]);

        assert!(validator.validate_value(&valid_array, &fixed_array).is_ok());
        assert!(validator
            .validate_value(&invalid_array, &fixed_array)
            .is_err());
    }

    #[test]
    fn test_optional_type_validation() {
        let validator = TypeValidator::new();

        let optional_string =
            TypeExpr::Optional(Box::new(TypeExpr::Primitive(PrimitiveType::String)));

        assert!(validator
            .validate_value(&Value::String("hello".to_string()), &optional_string)
            .is_ok());
        assert!(validator
            .validate_value(&Value::Nil, &optional_string)
            .is_ok());
        assert!(validator
            .validate_value(&Value::Integer(42), &optional_string)
            .is_err());
    }
}
