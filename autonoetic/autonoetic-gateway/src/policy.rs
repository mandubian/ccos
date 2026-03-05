//! Capability Policy Engine.

use autonoetic_types::agent::AgentManifest;
use autonoetic_types::capability::Capability;

/// Validates requested actions against the Agent's configured capabilities.
pub struct PolicyEngine {
    manifest: AgentManifest,
}

impl PolicyEngine {
    pub fn new(manifest: AgentManifest) -> Self {
        Self { manifest }
    }

    /// Check if the agent is allowed to execute a given command string.
    pub fn can_exec_shell(&self, command: &str) -> bool {
        for cap in &self.manifest.capabilities {
            if let Capability::ShellExec { patterns } = cap {
                for pattern in patterns {
                    // Naive glob stub: if pattern is "python3 scripts/*",
                    // we just check if command starts with "python3 scripts/".
                    // A real implementation would use the `glob` or `regex` crate.
                    let prefix = pattern.trim_end_matches('*');
                    if command.starts_with(prefix) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Check if the agent is allowed to connect to a specific host.
    pub fn can_connect_net(&self, host: &str) -> bool {
        for cap in &self.manifest.capabilities {
            if let Capability::NetConnect { hosts } = cap {
                if hosts.iter().any(|h| h == host || h == "*") {
                    return true;
                }
            }
        }
        false
    }
}
