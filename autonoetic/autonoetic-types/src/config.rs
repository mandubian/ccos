//! Gateway configuration types.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Top-level Gateway daemon configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    /// Directory containing agent subdirectories, each with a SKILL.md.
    #[serde(default = "default_agents_dir")]
    pub agents_dir: PathBuf,

    /// Port for the local JSON-RPC IPC listener.
    #[serde(default = "default_port")]
    pub port: u16,

    /// OFP federation port.
    #[serde(default = "default_ofp_port")]
    pub ofp_port: u16,

    /// Enable TLS on the OFP port.
    #[serde(default)]
    pub tls: bool,
}

fn default_agents_dir() -> PathBuf {
    PathBuf::from("./agents")
}

fn default_port() -> u16 {
    4000
}

fn default_ofp_port() -> u16 {
    4200
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            agents_dir: default_agents_dir(),
            port: default_port(),
            ofp_port: default_ofp_port(),
            tls: false,
        }
    }
}
