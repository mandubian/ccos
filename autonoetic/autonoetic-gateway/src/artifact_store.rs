//! Content-addressed Artifact Store.

use autonoetic_types::artifact::{ArtifactHandle, ArtifactKind, SourceRuntime, Visibility};
use std::path::{Path, PathBuf};

pub struct ArtifactStore {
    base_dir: PathBuf,
}

impl ArtifactStore {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        let base_dir = base_dir.into();
        // Ensure directory exists
        let _ = std::fs::create_dir_all(&base_dir);
        Self { base_dir }
    }

    /// Resolve an Artifact ID to its physical path on disk.
    pub fn resolve_path(&self, artifact_id: &str) -> PathBuf {
        self.base_dir.join(artifact_id)
    }

    /// Register a new local file into the artifact store.
    /// (Stub implementation)
    pub fn register(
        &self,
        _source_path: &Path,
        kind: ArtifactKind,
        owner_id: String,
    ) -> anyhow::Result<ArtifactHandle> {
        let fake_id = format!("artifact_{}", uuid::Uuid::new_v4().simple());
        let fake_sha =
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string(); // SHA-256 of empty str

        Ok(ArtifactHandle {
            artifact_id: fake_id,
            sha256: fake_sha,
            kind,
            owner_id,
            visibility: Visibility::Private,
            size_bytes: 0,
            mime_type: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            summary: None,
            source_runtime: Some(SourceRuntime {
                gateway_version: env!("CARGO_PKG_VERSION").to_string(),
                skill_name: None,
            }),
        })
    }
}
