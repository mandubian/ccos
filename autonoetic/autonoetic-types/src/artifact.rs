//! Artifact Handle — content-addressed immutable data objects.

use serde::{Deserialize, Serialize};

/// The kind of artifact stored.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Binary,
    SkillBundle,
    Dataset,
    GatewayRuntime,
    Report,
}

/// Provenance of the artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRuntime {
    pub gateway_version: String,
    pub skill_name: Option<String>,
}

/// Visibility scope.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    #[default]
    Private,
    Shared,
    Capsule,
}

/// An immutable content-addressed artifact handle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactHandle {
    pub artifact_id: String,
    pub sha256: String,
    pub kind: ArtifactKind,
    pub owner_id: String,
    #[serde(default)]
    pub visibility: Visibility,
    pub size_bytes: u64,
    pub mime_type: Option<String>,
    pub created_at: String,
    pub summary: Option<String>,
    pub source_runtime: Option<SourceRuntime>,
}
