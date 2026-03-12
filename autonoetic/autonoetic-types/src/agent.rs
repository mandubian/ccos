//! Agent Manifest types — the Rust representation of `SKILL.md` frontmatter.

use crate::background::BackgroundPolicy;
use crate::disclosure::DisclosurePolicy;
use serde::{Deserialize, Serialize};

use crate::capability::Capability;

/// Runtime declaration block from the SKILL.md frontmatter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeDeclaration {
    pub engine: String,
    pub gateway_version: String,
    pub sdk_version: String,
    #[serde(rename = "type")]
    pub runtime_type: String, // "stateful" | "stateless"
    pub sandbox: String, // "bubblewrap" | "docker" | "microvm" | "wasm"
    pub runtime_lock: String,
}

/// Core agent identity fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentIdentity {
    pub id: String,
    pub name: String,
    pub description: String,
}

/// LLM configuration for the agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub temperature: f64,
    pub fallback_provider: Option<String>,
    pub fallback_model: Option<String>,
}

/// Resource limits enforced by the Gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub max_memory_mb: u64,
    pub max_execution_time_sec: u64,
    pub token_budget_monthly: Option<u64>,
}

/// The full parsed Agent Manifest (SKILL.md frontmatter).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentManifest {
    pub version: String,
    pub runtime: RuntimeDeclaration,
    pub agent: AgentIdentity,
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    pub llm_config: Option<LlmConfig>,
    pub limits: Option<ResourceLimits>,
    #[serde(default)]
    pub background: Option<BackgroundPolicy>,
    #[serde(default)]
    pub disclosure: Option<DisclosurePolicy>,
    #[serde(default)]
    pub adaptation_hooks: Option<AdaptationHooks>,
}

/// Pipeline hooks extracted from adaptation overlays.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdaptationHooks {
    /// Script/command to run on user input before passing to the LLM.
    #[serde(default)]
    pub pre_process: Option<String>,
    /// Script/command to run on LLM output before returning to the user.
    #[serde(default)]
    pub post_process: Option<String>,
}

/// Lightweight metadata about a discovered agent on disk.
#[derive(Debug, Clone)]
pub struct AgentMeta {
    pub id: String,
    pub dir: std::path::PathBuf,
}

/// Action to perform on an asset during adaptation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AssetAction {
    Create,
    Update,
    Delete,
}

/// A specific file change to apply during adaptation overlay.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssetChange {
    pub path: String,
    pub content: String,
    pub action: AssetAction,
}
