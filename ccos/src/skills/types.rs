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
    /// Operations defined in this skill
    #[serde(default, deserialize_with = "deserialize_operations")]
    pub operations: Vec<SkillOperation>,
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
    /// Optional list of data classifications (spec-compatible)
    #[serde(default, alias = "data_classifications")]
    pub data_classifications: Vec<DataClassification>,
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
    /// Onboarding configuration for multi-step setup
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub onboarding: Option<OnboardingConfig>,
    /// Additional metadata
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

/// An operation defined within a skill
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillOperation {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub runtime: Option<String>,
    #[serde(default)]
    pub input_schema: Option<rtfs::ast::TypeExpr>,
    #[serde(default)]
    pub output_schema: Option<rtfs::ast::TypeExpr>,
}

/// Custom deserializer for `operations` that handles both:
/// - A YAML map: `operations: run: {command: python:run, description: ...}`
/// - A YAML array: `operations: [{name: run, command: python:run, ...}]`
fn deserialize_operations<'de, D>(deserializer: D) -> Result<Vec<SkillOperation>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    /// Intermediate struct for map-style operation values.
    #[derive(Debug, Deserialize)]
    struct OpMapValue {
        #[serde(default)]
        description: String,
        #[serde(default)]
        endpoint: Option<String>,
        #[serde(default)]
        method: Option<String>,
        #[serde(default)]
        command: Option<String>,
        #[serde(default)]
        runtime: Option<String>,
        #[serde(default)]
        input_schema: Option<rtfs::ast::TypeExpr>,
        #[serde(default)]
        output_schema: Option<rtfs::ast::TypeExpr>,
    }

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OperationsRepr {
        /// Array of SkillOperation (has `name` field)
        Array(Vec<SkillOperation>),
        /// Map of operation_name → {command, description, ...}
        Map(HashMap<String, OpMapValue>),
    }

    match OperationsRepr::deserialize(deserializer) {
        Ok(OperationsRepr::Array(ops)) => Ok(ops),
        Ok(OperationsRepr::Map(map)) => {
            let mut ops: Vec<SkillOperation> = map
                .into_iter()
                .map(|(name, v)| SkillOperation {
                    name,
                    description: v.description,
                    endpoint: v.endpoint,
                    method: v.method,
                    command: v.command,
                    runtime: v.runtime,
                    input_schema: v.input_schema,
                    output_schema: v.output_schema,
                })
                .collect();
            // Sort by name for deterministic ordering
            ops.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(ops)
        }
        Err(_) => {
            // If neither format matches, default to empty.
            // This mirrors the previous `#[serde(default)]` behavior.
            Ok(Vec::new())
        }
    }
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
            operations: Vec::new(),
            capabilities,
            effects: Vec::new(),
            secrets: Vec::new(),
            data_class: DataClassification::default(),
            data_classifications: Vec::new(),
            approval: ApprovalConfig::default(),
            display: DisplayMetadata::default(),
            instructions: instructions.into(),
            examples: Vec::new(),
            onboarding: None,
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

    /// Set onboarding configuration
    pub fn with_onboarding(mut self, onboarding: OnboardingConfig) -> Self {
        self.onboarding = Some(onboarding);
        self
    }
}

// =============================================================================
// Onboarding Types
// =============================================================================

/// Onboarding configuration for skills requiring multi-step setup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnboardingConfig {
    /// Whether onboarding is required for this skill
    #[serde(default = "default_true")]
    pub required: bool,
    /// Raw onboarding/setup section content from skill markdown.
    /// This is the primary source for LLM reasoning - the agent reads and interprets
    /// this prose to understand what setup steps are needed.
    #[serde(default)]
    pub raw_content: String,
    /// Ordered list of onboarding steps (for structured skills only).
    /// Only populated when skills provide explicit YAML/JSON step definitions.
    /// For markdown skills, prefer raw_content and let the LLM reason.
    #[serde(default)]
    pub steps: Vec<OnboardingStep>,
}

impl OnboardingConfig {
    /// Create an OnboardingConfig from raw prose content.
    /// The LLM will read and interpret this content to determine setup steps.
    pub fn from_raw(content: String) -> Self {
        Self {
            required: true,
            raw_content: content,
            steps: Vec::new(),
        }
    }
}

/// A single step in the onboarding process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnboardingStep {
    /// Unique step identifier
    pub id: String,
    /// Step type: api_call, human_action, etc.
    #[serde(rename = "type")]
    pub step_type: OnboardingStepType,
    /// Operation to execute (for api_call steps)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    /// Step dependencies - must complete before this step
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
    /// Data storage configuration (what to store from responses)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub store: Vec<StoreConfig>,
    /// Human action configuration (for human_action steps)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<HumanActionConfig>,
    /// Parameters to pass to the operation
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub params: HashMap<String, String>,
    /// Optional completion predicate to verify success (e.g. ActionSucceeded)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verify_on_success: Option<crate::chat::Predicate>,
}

/// Types of onboarding steps
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnboardingStepType {
    /// API call step (agent-autonomous)
    ApiCall,
    /// Human-in-the-loop step
    HumanAction,
    /// Condition/check step
    Condition,
}

/// Configuration for what to store from step responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreConfig {
    /// JSON path in response (e.g., "response.agent_id")
    pub from: String,
    /// Where to store (e.g., "memory:moltbook.agent_id" or "secret:MOLTBOOK_SECRET")
    pub to: String,
    /// Whether this requires approval before storing
    #[serde(default)]
    pub requires_approval: bool,
}

/// Configuration for human action steps
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanActionConfig {
    /// Action type identifier
    pub action_type: String,
    /// Title for approval UI
    pub title: String,
    /// Detailed instructions (markdown supported)
    pub instructions: String,
    /// Expected response schema
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_response: Option<serde_json::Value>,
}

/// Current state of skill onboarding
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OnboardingState {
    /// Skill not yet loaded
    NotLoaded,
    /// Skill loaded, checking requirements
    Loaded,
    /// Ready to use (no onboarding required)
    Ready,
    /// Needs setup/onboarding
    NeedsSetup,
    /// Needs secrets configuration
    NeedsSecrets,
    /// Waiting for human action
    PendingHumanAction,
    /// Waiting for secret approval
    PendingSecretApproval,
    /// Fully operational
    Operational,
}

impl Default for OnboardingState {
    fn default() -> Self {
        OnboardingState::NotLoaded
    }
}

/// Runtime state of skill onboarding (stored in memory)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillOnboardingState {
    /// Current onboarding status
    pub status: OnboardingState,
    /// Current step index (0-based)
    pub current_step: usize,
    /// Total number of steps
    pub total_steps: usize,
    /// IDs of completed steps
    pub completed_steps: Vec<String>,
    /// Approval ID if waiting for human action
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_approval_id: Option<String>,
    /// Data collected during onboarding
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub data: HashMap<String, serde_json::Value>,
    /// When onboarding started
    pub started_at: String,
    /// Last update timestamp
    pub last_updated: String,
}

impl SkillOnboardingState {
    /// Create initial state for a skill with onboarding
    pub fn new(total_steps: usize) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            status: OnboardingState::Loaded,
            current_step: 0,
            total_steps,
            completed_steps: Vec::new(),
            pending_approval_id: None,
            data: HashMap::new(),
            started_at: now.clone(),
            last_updated: now,
        }
    }

    /// Mark current step as complete and advance
    pub fn complete_step(&mut self, step_id: String) {
        self.completed_steps.push(step_id);
        self.current_step += 1;
        self.last_updated = chrono::Utc::now().to_rfc3339();

        if self.current_step >= self.total_steps {
            self.status = OnboardingState::Operational;
        }
    }

    /// Set status to waiting for human action
    pub fn set_pending_human_action(&mut self, approval_id: String) {
        self.status = OnboardingState::PendingHumanAction;
        self.pending_approval_id = Some(approval_id);
        self.last_updated = chrono::Utc::now().to_rfc3339();
    }

    /// Resume after human action completion
    pub fn resume_from_human_action(&mut self) {
        self.status = OnboardingState::NeedsSetup;
        self.pending_approval_id = None;
        self.last_updated = chrono::Utc::now().to_rfc3339();
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
