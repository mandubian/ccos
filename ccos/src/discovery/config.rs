//! Configuration for discovery engine behavior

use serde::{Deserialize, Serialize};

/// Configuration for capability discovery matching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryConfig {
    /// Minimum semantic match score threshold (0.0 to 1.0)
    /// Default: 0.65 (higher than before to reduce false positives)
    #[serde(default = "default_match_threshold")]
    pub match_threshold: f64,

    /// Enable embedding-based matching (more accurate but requires API)
    /// Can be set via CCOS_DISCOVERY_USE_EMBEDDINGS env var
    #[serde(default = "default_use_embeddings")]
    pub use_embeddings: bool,

    /// Preferred remote embedding model (e.g., OpenRouter)
    #[serde(default)]
    pub embedding_model: Option<String>,

    /// Preferred local embedding model (e.g., Ollama)
    #[serde(default)]
    pub local_embedding_model: Option<String>,

    /// Minimum score required for action verb match (0.0 to 1.0)
    /// Action verbs must match for capabilities to be considered compatible
    #[serde(default = "default_action_verb_threshold")]
    pub action_verb_threshold: f64,

    /// Weight for action verbs in matching (higher = more important)
    #[serde(default = "default_action_verb_weight")]
    pub action_verb_weight: f64,

    /// Weight for capability class matching (0.0 to 1.0)
    #[serde(default = "default_capability_class_weight")]
    pub capability_class_weight: f64,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            match_threshold: default_match_threshold(),
            use_embeddings: default_use_embeddings(),
            embedding_model: None,
            local_embedding_model: None,
            action_verb_threshold: default_action_verb_threshold(),
            action_verb_weight: default_action_verb_weight(),
            capability_class_weight: default_capability_class_weight(),
        }
    }
}

impl DiscoveryConfig {
    /// Create config from RTFS AgentConfig discovery section
    pub fn from_agent_config(agent_config: &rtfs::config::types::DiscoveryConfig) -> Self {
        Self {
            match_threshold: agent_config.match_threshold,
            use_embeddings: agent_config.use_embeddings,
            embedding_model: agent_config.embedding_model.clone(),
            local_embedding_model: agent_config.local_embedding_model.clone(),
            action_verb_threshold: agent_config.action_verb_threshold,
            action_verb_weight: agent_config.action_verb_weight,
            capability_class_weight: agent_config.capability_class_weight,
        }
    }

    /// Create config from environment variables (fallback if no AgentConfig)
    pub fn from_env() -> Self {
        let match_threshold = std::env::var("CCOS_DISCOVERY_MATCH_THRESHOLD")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| default_match_threshold());

        let use_embeddings = std::env::var("CCOS_DISCOVERY_USE_EMBEDDINGS")
            .ok()
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or_else(|| default_use_embeddings());

        let embedding_model = std::env::var("CCOS_DISCOVERY_EMBEDDING_MODEL")
            .ok()
            .or_else(|| std::env::var("EMBEDDING_MODEL").ok());

        let local_embedding_model = std::env::var("CCOS_DISCOVERY_LOCAL_EMBEDDING_MODEL")
            .ok()
            .or_else(|| std::env::var("LOCAL_EMBEDDING_MODEL").ok());

        let action_verb_threshold = std::env::var("CCOS_DISCOVERY_ACTION_VERB_THRESHOLD")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| default_action_verb_threshold());

        let action_verb_weight = std::env::var("CCOS_DISCOVERY_ACTION_VERB_WEIGHT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| default_action_verb_weight());

        let capability_class_weight = std::env::var("CCOS_DISCOVERY_CAPABILITY_CLASS_WEIGHT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| default_capability_class_weight());

        Self {
            match_threshold,
            use_embeddings,
            embedding_model,
            local_embedding_model,
            action_verb_threshold,
            action_verb_weight,
            capability_class_weight,
        }
    }
}

fn default_match_threshold() -> f64 {
    0.65 // Higher than before to reduce false positives
}

fn default_use_embeddings() -> bool {
    false // Default to false (embedding-based matching disabled by default)
}

fn default_action_verb_threshold() -> f64 {
    0.7 // Action verbs must match well
}

fn default_action_verb_weight() -> f64 {
    0.4 // Action verbs are important but not everything
}

fn default_capability_class_weight() -> f64 {
    0.3 // Capability class matching is a bonus factor
}
