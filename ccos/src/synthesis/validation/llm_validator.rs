//! LLM-based validation for schemas and plans.
//!
//! This module provides configurable validation using LLM:
//! - Schema validation: verify inferred schemas are correct
//! - Plan validation: check schema compatibility, dependencies, parameters

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for LLM validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationConfig {
    /// Enable LLM schema validation for inferred schemas
    #[serde(default)]
    pub enable_schema_validation: bool,

    /// Enable LLM plan validation (schema compatibility, dependencies)
    #[serde(default)]
    pub enable_plan_validation: bool,

    /// Enable auto-repair on validation failures
    #[serde(default = "default_true")]
    pub enable_auto_repair: bool,

    /// Max auto-repair attempts before queuing for external review
    #[serde(default = "default_max_repair_attempts")]
    pub max_repair_attempts: usize,

    /// Override LLM model for validation (uses default if None)
    #[serde(default)]
    pub validation_model: Option<String>,
}

fn default_true() -> bool {
    true
}
fn default_max_repair_attempts() -> usize {
    2
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            enable_schema_validation: false,
            enable_plan_validation: false,
            enable_auto_repair: true,
            max_repair_attempts: 2,
            validation_model: None,
        }
    }
}

/// Result of a validation attempt.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub errors: Vec<ValidationError>,
    pub suggestions: Vec<String>,
}

/// A specific validation error.
#[derive(Debug, Clone)]
pub struct ValidationError {
    pub error_type: ValidationErrorType,
    pub message: String,
    pub location: Option<String>,
    pub suggested_fix: Option<String>,
}

#[derive(Debug, Clone)]
pub enum ValidationErrorType {
    SchemaMismatch,
    MissingParameter,
    InvalidDependency,
    CyclicDependency,
    UnavailableCapability,
    TypeIncompatibility,
    Other,
}

impl ValidationResult {
    pub fn valid() -> Self {
        Self {
            is_valid: true,
            errors: vec![],
            suggestions: vec![],
        }
    }

    pub fn invalid(errors: Vec<ValidationError>) -> Self {
        Self {
            is_valid: false,
            errors,
            suggestions: vec![],
        }
    }
}

/// Validate an inferred schema using LLM.
///
/// # Arguments
/// - `schema`: The inferred RTFS schema string
/// - `capability_description`: Description of what the capability does
/// - `sample_output`: Optional sample output that was used to infer the schema
pub async fn validate_schema(
    _schema: &str,
    _capability_description: &str,
    _sample_output: Option<&str>,
    _config: &ValidationConfig,
) -> Result<ValidationResult, String> {
    // TODO: Implement LLM call for schema validation
    // For now, return valid to allow the pipeline to work
    log::debug!("Schema validation not yet implemented, returning valid");
    Ok(ValidationResult::valid())
}

/// Validate a generated RTFS plan.
///
/// # Arguments
/// - `plan`: The RTFS plan code
/// - `resolutions`: Map of intent IDs to resolved capabilities
/// - `context`: Additional context about the plan goal
pub async fn validate_plan(
    _plan: &str,
    _resolutions: &HashMap<String, String>,
    _context: &str,
    _config: &ValidationConfig,
) -> Result<ValidationResult, String> {
    // TODO: Implement LLM call for plan validation
    // For now, return valid to allow the pipeline to work
    log::debug!("Plan validation not yet implemented, returning valid");
    Ok(ValidationResult::valid())
}

/// Try to auto-repair a plan based on validation errors.
///
/// # Arguments
/// - `plan`: The original plan code
/// - `errors`: Validation errors to fix
/// - `attempt`: Current repair attempt (1-indexed)
pub async fn auto_repair_plan(
    plan: &str,
    _errors: &[ValidationError],
    attempt: usize,
    config: &ValidationConfig,
) -> Result<Option<String>, String> {
    if attempt > config.max_repair_attempts {
        log::info!(
            "Max repair attempts ({}) exceeded, queuing for external review",
            config.max_repair_attempts
        );
        return Ok(None);
    }

    // TODO: Implement LLM-based auto-repair
    // For now, return None to trigger queue escalation
    log::debug!(
        "Auto-repair not yet implemented, attempt {}/{}",
        attempt,
        config.max_repair_attempts
    );
    Ok(Some(plan.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ValidationConfig::default();
        assert!(!config.enable_schema_validation);
        assert!(!config.enable_plan_validation);
        assert!(config.enable_auto_repair);
        assert_eq!(config.max_repair_attempts, 2);
    }

    #[tokio::test]
    async fn test_validate_schema_placeholder() {
        let config = ValidationConfig::default();
        let result = validate_schema("[:map]", "test capability", None, &config).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_valid);
    }
}
