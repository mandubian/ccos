use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

use crate::capability_marketplace::types::{
    FilesystemSpec, FilesystemSpecMode, MountSpec, MountSpecMode, ResourceSpec, RuntimeType,
    SandboxedCapability,
};
use crate::sandbox::filesystem::{FilesystemMode, Mount, MountMode, VirtualFilesystem};
use crate::sandbox::resources::ResourceLimits;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SandboxRuntimeType {
    Process,
    MicroVM,
    Wasm,
    Container,
    Bubblewrap,
    Native,
}

impl From<RuntimeType> for SandboxRuntimeType {
    fn from(rt: RuntimeType) -> Self {
        match rt {
            RuntimeType::Rtfs => SandboxRuntimeType::Native, // Pure RTFS runs natively
            RuntimeType::Wasm => SandboxRuntimeType::Wasm,
            RuntimeType::Container => SandboxRuntimeType::Container,
            RuntimeType::MicroVM => SandboxRuntimeType::MicroVM,
            RuntimeType::Native => SandboxRuntimeType::Native,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    pub runtime_type: SandboxRuntimeType,
    pub provider: Option<String>,
    pub capability_id: Option<String>,
    pub required_secrets: Vec<String>,
    pub allowed_hosts: HashSet<String>,
    pub allowed_ports: HashSet<u16>,
    pub secret_mount_dir: Option<String>,
    pub filesystem: Option<VirtualFilesystem>,
    pub resources: Option<ResourceLimits>,
    pub network_enabled: bool,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            runtime_type: SandboxRuntimeType::MicroVM,
            provider: None,
            capability_id: None,
            required_secrets: Vec::new(),
            allowed_hosts: HashSet::new(),
            allowed_ports: HashSet::new(),
            secret_mount_dir: None,
            filesystem: None,
            resources: None,
            network_enabled: true,
        }
    }
}

impl SandboxConfig {
    /// Build a SandboxConfig from a SandboxedCapability manifest.
    /// Falls back to metadata keys for backward compatibility.
    pub fn from_sandboxed_capability(
        cap: &SandboxedCapability,
        capability_id: Option<String>,
    ) -> Self {
        let mut config = SandboxConfig::default();
        config.capability_id = capability_id;
        config.provider = cap.provider.clone();
        config.required_secrets = cap.secrets.clone();

        // Determine runtime type from spec or provider name
        if let Some(spec) = &cap.runtime_spec {
            config.runtime_type = spec.runtime_type.clone().into();
        } else if let Some(provider) = &cap.provider {
            config.runtime_type = match provider.to_lowercase().as_str() {
                "firecracker" | "microvm" => SandboxRuntimeType::MicroVM,
                "bwrap" | "bubblewrap" => SandboxRuntimeType::Bubblewrap,
                "wasm" | "wasmtime" => SandboxRuntimeType::Wasm,
                "container" | "docker" | "nsjail" => SandboxRuntimeType::Container,
                "process" => SandboxRuntimeType::Process,
                _ => SandboxRuntimeType::MicroVM,
            };
        }

        // Apply network policy
        if let Some(network) = &cap.network_policy {
            config.allowed_hosts = network.allowed_hosts.iter().cloned().collect();
            config.allowed_ports = network.allowed_ports.iter().cloned().collect();
        }

        // Apply filesystem spec
        if let Some(fs) = &cap.filesystem {
            config.filesystem = Some(VirtualFilesystem::from(fs));
        }

        // Apply resource limits
        if let Some(res) = &cap.resources {
            config.resources = Some(ResourceLimits::from(res));
        }

        config
    }
}

// Conversion from manifest FilesystemSpec to sandbox VirtualFilesystem
impl From<&FilesystemSpec> for VirtualFilesystem {
    fn from(spec: &FilesystemSpec) -> Self {
        VirtualFilesystem {
            mounts: spec.mounts.iter().map(Mount::from).collect(),
            quota_mb: spec.quota_mb,
            mode: match spec.mode {
                FilesystemSpecMode::Ephemeral => FilesystemMode::Ephemeral,
                FilesystemSpecMode::Session => FilesystemMode::Session,
                FilesystemSpecMode::Persistent => FilesystemMode::Persistent,
            },
        }
    }
}

impl From<&MountSpec> for Mount {
    fn from(spec: &MountSpec) -> Self {
        Mount {
            host_path: PathBuf::from(&spec.host),
            guest_path: spec.guest.clone(),
            mode: match spec.mode {
                MountSpecMode::ReadOnly => MountMode::ReadOnly,
                MountSpecMode::ReadWrite => MountMode::ReadWrite,
            },
        }
    }
}

// Conversion from manifest ResourceSpec to sandbox ResourceLimits
impl From<&ResourceSpec> for ResourceLimits {
    fn from(spec: &ResourceSpec) -> Self {
        ResourceLimits {
            cpu_shares: spec.cpu_shares,
            memory_mb: spec.memory_mb,
            timeout_ms: spec.timeout_ms,
            network_egress_bytes: spec.network_bytes.unwrap_or(0),
        }
    }
}
