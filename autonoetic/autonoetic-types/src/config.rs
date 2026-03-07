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

    /// Maximum number of agent runtime executions allowed concurrently.
    #[serde(default = "default_max_concurrent_spawns")]
    pub max_concurrent_spawns: usize,

    /// Maximum number of pending executions admitted per target agent.
    /// This count includes the currently running execution for that agent.
    #[serde(default = "default_max_pending_spawns_per_agent")]
    pub max_pending_spawns_per_agent: usize,

    /// Enable the gateway-owned background scheduler.
    #[serde(default)]
    pub background_scheduler_enabled: bool,

    /// Tick interval for background due checks.
    #[serde(default = "default_background_tick_secs")]
    pub background_tick_secs: u64,

    /// Global minimum allowed reevaluation interval across agents.
    #[serde(default = "default_background_min_interval_secs")]
    pub background_min_interval_secs: u64,

    /// Max number of due background agents admitted per scheduler tick.
    #[serde(default = "default_max_background_due_per_tick")]
    pub max_background_due_per_tick: usize,
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

fn default_max_concurrent_spawns() -> usize {
    8
}

fn default_max_pending_spawns_per_agent() -> usize {
    4
}

fn default_background_tick_secs() -> u64 {
    5
}

fn default_background_min_interval_secs() -> u64 {
    60
}

fn default_max_background_due_per_tick() -> usize {
    32
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            agents_dir: default_agents_dir(),
            port: default_port(),
            ofp_port: default_ofp_port(),
            tls: false,
            max_concurrent_spawns: default_max_concurrent_spawns(),
            max_pending_spawns_per_agent: default_max_pending_spawns_per_agent(),
            background_scheduler_enabled: false,
            background_tick_secs: default_background_tick_secs(),
            background_min_interval_secs: default_background_min_interval_secs(),
            max_background_due_per_tick: default_max_background_due_per_tick(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_background_scheduler_defaults() {
        let config = GatewayConfig::default();
        assert!(!config.background_scheduler_enabled);
        assert_eq!(config.background_tick_secs, 5);
        assert_eq!(config.background_min_interval_secs, 60);
        assert_eq!(config.max_background_due_per_tick, 32);
    }
}
