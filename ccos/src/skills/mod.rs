//! Skills Layer for CCOS
//!
//! Provides natural-language skill definitions that map to governed capabilities.
//! Skills are higher-level abstractions that bundle capabilities with instructions,
//! approval requirements, and display metadata.

pub mod mapper;
pub mod parser;
pub mod types;

pub use mapper::{SkillError, SkillMapper};
pub use parser::parse_skill_yaml;
pub use types::{ApprovalConfig, DataClassification, DisplayMetadata, Skill};
