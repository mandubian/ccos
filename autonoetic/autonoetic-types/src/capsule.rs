//! Cognitive Capsule Manifest — portable agent export.

use serde::{Deserialize, Serialize};

/// Capsule mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapsuleMode {
    Thin,
    Hermetic,
}

/// A reference to an included artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncludedArtifact {
    pub artifact_id: String,
    pub sha256: String,
}

/// Gateway runtime embedded in a hermetic capsule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapsuleGatewayRuntime {
    pub artifact: String,
    pub version: String,
    pub sha256: String,
}

/// The `capsule.json` manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapsuleManifest {
    pub capsule_id: String,
    pub agent_id: String,
    pub mode: CapsuleMode,
    pub created_at: String,
    pub entrypoint: String,
    pub runtime_lock: String,
    #[serde(default)]
    pub included_artifacts: Vec<IncludedArtifact>,
    pub gateway_runtime: Option<CapsuleGatewayRuntime>,
    #[serde(default)]
    pub redactions: Vec<String>,
}
