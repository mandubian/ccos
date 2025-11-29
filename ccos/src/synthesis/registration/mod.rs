//! Capability registration and validation workflow.
//!
//! This module contains the registration pipeline for capabilities:
//! - Pre-flight validation before registration
//! - Governance policy enforcement
//! - Static code analysis
//! - Registration flow orchestration

pub mod governance_policies;
pub mod registration_flow;
pub mod static_analyzers;
pub mod validation_harness;

// Re-export commonly used types
pub use governance_policies::{GovernancePolicy, MaxParameterCountPolicy};
pub use registration_flow::{RegistrationFlow, RegistrationResult};
pub use static_analyzers::{PerformanceAnalyzer, SecurityAnalyzer, StaticAnalyzer};
pub use validation_harness::{ValidationHarness, ValidationResult, ValidationStatus};
