//! Skill types for WS9 Phase 4
//!
//! Defines the Skill struct and related types for declarative skill definitions.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A skill is a high-level capability bundle with instructions and governance metadata.
/// Skills map natural-language intents to governed capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    /// Unique skill identifier (e.g., "search-places-nearby")
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Short description of what the skill does
    #[serde(default)]
    pub description: String,
    /// Semantic version
    #[serde(default = "default_version")]
    pub version: String,
    /// Required capability IDs that this skill uses
    pub capabilities: Vec<String>,
    /// Effects declared by this skill (union of capability effects)
    #[serde(default)]
    pub effects: Vec<String>,
    /// Secrets required for this skill's capabilities
    #[serde(default)]
    pub secrets: Vec<String>,
    /// Data classification for governance
    #[serde(default)]
    pub data_class: DataClassification,
    /// Approval configuration
    #[serde(default)]
    pub approval: ApprovalConfig,
    /// Display metadata for UI presentation
    #[serde(default)]
    pub display: DisplayMetadata,
    /// Natural language instructions for LLM interpretation
    /// This teaches the LLM how to use the skill's capabilities
    pub instructions: String,
    /// Example usages (for few-shot prompting)
    #[serde(default)]
    pub examples: Vec<SkillExample>,
    /// Additional metadata
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

fn default_version() -> String {
    "1.0.0".to_string()
}

/// Data classification levels for governance
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DataClassification {
    /// Public data - no restrictions
    #[default]
    Public,
    /// Internal data - company-internal only
    Internal,
    /// Confidential data - need-to-know basis
    Confidential,
    /// Restricted data - highly sensitive (PII, financial, etc.)
    Restricted,
    /// Personal Identifiable Information
    PII,
}

/// Approval configuration for skill execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalConfig {
    /// Whether approval is required before first use
    #[serde(default)]
    pub required: bool,
    /// Approval mode: once, per-session, per-call
    #[serde(default)]
    pub mode: ApprovalMode,
    /// Domains requiring extra approval (e.g., "finance", "pii")
    #[serde(default)]
    pub sensitive_domains: Vec<String>,
    /// Maximum autonomy level allowed (0-4)
    #[serde(default = "default_max_autonomy")]
    pub max_autonomy: u8,
}

fn default_max_autonomy() -> u8 {
    2
}

impl Default for ApprovalConfig {
    fn default() -> Self {
        Self {
            required: false,
            mode: ApprovalMode::Once,
            sensitive_domains: Vec::new(),
            max_autonomy: 2,
        }
    }
}

/// When approval is required
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ApprovalMode {
    /// Approve once, then auto-approve
    #[default]
    Once,
    /// Approve once per session
    PerSession,
    /// Approve every call
    PerCall,
    /// Never require approval (trusted skills only)
    Never,
}

/// Display metadata for UI presentation
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DisplayMetadata {
    /// Icon name or emoji
    #[serde(default)]
    pub icon: String,
    /// Category for grouping in UI
    #[serde(default)]
    pub category: String,
    /// Tags for search/filtering
    #[serde(default)]
    pub tags: Vec<String>,
    /// Short one-line summary
    #[serde(default)]
    pub summary: String,
    /// Whether to show in skill picker
    #[serde(default = "default_true")]
    pub visible: bool,
}

fn default_true() -> bool {
    true
}

/// Example usage for few-shot prompting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillExample {
    /// User intent/query
    pub input: String,
    /// Expected capability call
    pub capability: String,
    /// Expected parameters (JSON-like string or structured)
    pub params: String,
    /// Optional expected output description
    pub output: Option<String>,
}

impl Skill {
    /// Create a new skill with minimal required fields
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        capabilities: Vec<String>,
        instructions: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: description.into(),
            version: "1.0.0".to_string(),
            capabilities,
            effects: Vec::new(),
            secrets: Vec::new(),
            data_class: DataClassification::default(),
            approval: ApprovalConfig::default(),
            display: DisplayMetadata::default(),
            instructions: instructions.into(),
            examples: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    /// Add effects to the skill
    pub fn with_effects(mut self, effects: Vec<String>) -> Self {
        self.effects = effects;
        self
    }

    /// Add secrets requirement
    pub fn with_secrets(mut self, secrets: Vec<String>) -> Self {
        self.secrets = secrets;
        self
    }

    /// Set data classification
    pub fn with_data_class(mut self, data_class: DataClassification) -> Self {
        self.data_class = data_class;
        self
    }

    /// Set approval config
    pub fn with_approval(mut self, approval: ApprovalConfig) -> Self {
        self.approval = approval;
        self
    }

    /// Set display metadata
    pub fn with_display(mut self, display: DisplayMetadata) -> Self {
        self.display = display;
        self
    }

    /// Add examples
    pub fn with_examples(mut self, examples: Vec<SkillExample>) -> Self {
        self.examples = examples;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_creation() {
        let skill = Skill::new(
            "search-places",
            "Search Places",
            "Search for nearby places using Google Maps",
            vec!["google-maps.places.search".to_string()],
            "Use this skill to find restaurants, shops, and other places nearby.",
        );

        assert_eq!(skill.id, "search-places");
        assert_eq!(skill.capabilities.len(), 1);
        assert!(matches!(skill.data_class, DataClassification::Public));
    }

    #[test]
    fn test_skill_yaml_roundtrip() {
        let skill = Skill::new(
            "test-skill",
            "Test Skill",
            "A test skill",
            vec!["cap1".to_string()],
            "Test instructions",
        )
        .with_data_class(DataClassification::Confidential);

        let yaml = serde_yaml::to_string(&skill).unwrap();
        let parsed: Skill = serde_yaml::from_str(&yaml).unwrap();

        assert_eq!(parsed.id, skill.id);
        assert_eq!(parsed.data_class, DataClassification::Confidential);
    }
}
