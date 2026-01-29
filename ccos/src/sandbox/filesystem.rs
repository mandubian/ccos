use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualFilesystem {
    pub mounts: Vec<Mount>,
    pub quota_mb: u64,
    pub mode: FilesystemMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mount {
    pub host_path: PathBuf,
    pub guest_path: String,
    pub mode: MountMode,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MountMode {
    ReadOnly,
    ReadWrite,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum FilesystemMode {
    Ephemeral,
    Session,
    Persistent,
}

impl Default for VirtualFilesystem {
    fn default() -> Self {
        Self {
            mounts: Vec::new(),
            quota_mb: 0,
            mode: FilesystemMode::Ephemeral,
        }
    }
}
