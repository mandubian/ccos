//! Skills Layer for CCOS
//!
//! Provides natural-language skill definitions that map to governed capabilities.
//! Skills are higher-level abstractions that bundle capabilities with instructions,
//! approval requirements, and display metadata.

pub mod capabilities;
pub mod loader;
pub mod mapper;
pub mod onboarding_capabilities;
pub mod onboarding_state_machine;
pub mod parser;
pub mod primitives;
pub mod types;

pub use loader::{load_skill_from_url, LoadError, LoadedSkillInfo, SkillFormat};
pub use mapper::{Intent, SkillError, SkillMapper};
pub use onboarding_state_machine::{OnboardingStateMachine, OnboardingStatusSummary, StepExecutionResult};
pub use parser::parse_skill_yaml;
pub use primitives::{MappedCapability, PrimitiveMapper};
pub use types::{
    ApprovalConfig, DataClassification, DisplayMetadata, HumanActionConfig, OnboardingConfig,
    OnboardingState, OnboardingStep, OnboardingStepType, Skill, SkillOnboardingState,
};
pub use capabilities::register_skill_capabilities;
