//! Validation module for synthesized capabilities and plans.
//!
//! Provides LLM-based validation with configurable auto-repair
//! and queue escalation for external review.

pub mod llm_validator;

pub use crate::config::types::ValidationConfig;
pub use llm_validator::{
    auto_repair_plan, llm_repair_runtime_error, repair_runtime_error_with_retry, validate_plan,
    validate_schema, ValidationError, ValidationErrorType, ValidationResult,
};
