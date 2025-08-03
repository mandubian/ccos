//! MicroVM Configuration Types

use std::collections::HashMap;
use std::time::Duration;
use serde::{Deserialize, Serialize};

/// Configuration for MicroVM execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MicroVMConfig {
    /// Maximum execution time for operations
    pub timeout: Duration,
    /// Memory limit in MB
    pub memory_limit_mb: u64,
    /// CPU limit (relative to host)
    pub cpu_limit: f64,
    /// Network access policy
    pub network_policy: NetworkPolicy,
    /// File system access policy
    pub fs_policy: FileSystemPolicy,
    /// Environment variables to pass
    pub env_vars: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkPolicy {
    /// No network access
    Denied,
    /// Allow specific domains only
    AllowList(Vec<String>),
    /// Allow all except specific domains
    DenyList(Vec<String>),
    /// Full network access (dangerous)
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FileSystemPolicy {
    /// No file system access
    None,
    /// Read-only access to specific paths
    ReadOnly(Vec<String>),
    /// Read-write access to specific paths
    ReadWrite(Vec<String>),
    /// Full file system access (dangerous)
    Full,
}

impl Default for MicroVMConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            memory_limit_mb: 512,
            cpu_limit: 1.0,
            network_policy: NetworkPolicy::Denied,
            fs_policy: FileSystemPolicy::None,
            env_vars: HashMap::new(),
        }
    }
}
