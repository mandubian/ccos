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

    /// Check if the agent is allowed to read from a relative file path.
    pub fn can_read_path(&self, path: &str) -> bool {
        for cap in &self.manifest.capabilities {
            if let Capability::MemoryRead { scopes } = cap {
                for scope in scopes {
                    let prefix = scope.trim_end_matches('*');
                    if path.starts_with(prefix) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Check if the agent is allowed to write to a relative file path.
    pub fn can_write_path(&self, path: &str) -> bool {
        for cap in &self.manifest.capabilities {
            if let Capability::MemoryWrite { scopes } = cap {
                for scope in scopes {
                    let prefix = scope.trim_end_matches('*');
                    if path.starts_with(prefix) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Check if the agent is allowed to spawn child agents.
    pub fn can_spawn_agent(&self) -> bool {
        for cap in &self.manifest.capabilities {
            if matches!(cap, Capability::AgentSpawn { .. }) {
                return true;
            }
        }
        false
    }

    /// Return the configured child-agent delegation limit, if any.
    pub fn spawn_agent_limit(&self) -> Option<u32> {
        self.manifest.capabilities.iter().find_map(|cap| {
            if let Capability::AgentSpawn { max_children } = cap {
                Some(*max_children)
            } else {
                None
            }
        })
    }

    /// Check if the agent is allowed to message a target agent.
    pub fn can_message_agent(&self, target_agent: &str) -> bool {
        for cap in &self.manifest.capabilities {
            if let Capability::AgentMessage { patterns } = cap {
                for pattern in patterns {
                    let prefix = pattern.trim_end_matches('*');
                    if target_agent.starts_with(prefix) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Return background reevaluation limits, if configured.
    pub fn background_reevaluation_limits(&self) -> Option<(u64, bool)> {
        self.manifest.capabilities.iter().find_map(|cap| {
            if let Capability::BackgroundReevaluation {
                min_interval_secs,
                allow_reasoning,
            } = cap
            {
                Some((*min_interval_secs, *allow_reasoning))
            } else {
                None
            }
        })
    }
}
