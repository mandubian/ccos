//! Runtime Lock types — the pinned execution closure for reproducible resolution.

use serde::{Deserialize, Serialize};

/// A pinned artifact reference inside the runtime lock.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedArtifact {
    pub name: String,
    pub version: String,
    pub sha256: String,
    pub source: String,
}

/// Gateway binary reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedGateway {
    pub artifact: String,
    pub version: String,
    pub sha256: String,
    pub signature: Option<String>,
}

/// SDK version reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedSdk {
    pub version: String,
}

/// Sandbox backend reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedSandbox {
    pub backend: String,
}

/// The complete `runtime.lock` file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeLock {
    pub gateway: LockedGateway,
    pub sdk: LockedSdk,
    pub sandbox: LockedSandbox,
    #[serde(default)]
    pub artifacts: Vec<LockedArtifact>,
}
