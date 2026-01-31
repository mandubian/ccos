//! Skills Layer for CCOS
//!
//! Provides natural-language skill definitions that map to governed capabilities.
//! Skills are higher-level abstractions that bundle capabilities with instructions,
//! approval requirements, and display metadata.

pub mod loader;
pub mod mapper;
pub mod parser;
pub mod primitives;
pub mod types;
pub mod capabilities;

pub use loader::{load_skill_from_url, LoadError, LoadedSkillInfo, SkillFormat};
pub use mapper::{Intent, SkillError, SkillMapper};
pub use parser::parse_skill_yaml;
pub use primitives::{MappedCapability, PrimitiveMapper};
pub use types::{ApprovalConfig, DataClassification, DisplayMetadata, Skill};
pub use capabilities::register_skill_capabilities;
