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

// ---------------------------------------------------------------------------
// Artifact Bundle — closed file closure for review/install/execution
// ---------------------------------------------------------------------------

/// A single file entry in an artifact bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactFileEntry {
    /// Filename within the artifact (e.g., "src/main.py")
    pub name: String,
    /// Content handle in the content store (sha256:...)
    pub handle: String,
    /// Short alias for LLM-friendly reference
    pub alias: String,
}

/// An immutable artifact bundle — a closed set of files for review/install/execution.
///
/// Artifacts are the only units that may cross trust boundaries.
/// They are built from session content and are immutable once created.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactBundle {
    /// Short unique ID (e.g., "art_a1b2c3d4")
    pub artifact_id: String,
    /// Files included in the artifact
    pub files: Vec<ArtifactFileEntry>,
    /// Optional entrypoints (e.g., ["src/main.py"])
    #[serde(default)]
    pub entrypoints: Vec<String>,
    /// SHA-256 digest of the full manifest (content-addressable identity)
    pub digest: String,
    /// ISO 8601 creation timestamp
    pub created_at: String,
    /// Session that built this artifact
    pub builder_session_id: String,
}
