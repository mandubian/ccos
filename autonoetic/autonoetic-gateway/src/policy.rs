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

    /// Check if the agent is allowed to invoke a named tool (typically MCP tools).
    pub fn can_invoke_tool(&self, tool_name: &str) -> bool {
        for cap in &self.manifest.capabilities {
            if let Capability::ToolInvoke { allowed } = cap {
                for pattern in allowed {
                    let prefix = pattern.trim_end_matches('*');
                    if tool_name.starts_with(prefix) {
                        return true;
                    }
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

    /// Check if the agent is allowed to share memory with specific targets.
    pub fn can_share_memory(&self, target_agent: &str) -> bool {
        for cap in &self.manifest.capabilities {
            if let Capability::MemoryShare { allowed_targets } = cap {
                for target in allowed_targets {
                    let prefix = target.trim_end_matches('*');
                    if target_agent.starts_with(prefix) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Check if the agent is allowed to search memory in specific scopes.
    pub fn can_search_memory(&self, scope: &str) -> bool {
        for cap in &self.manifest.capabilities {
            if let Capability::MemorySearch { scopes } = cap {
                for allowed_scope in scopes {
                    let prefix = allowed_scope.trim_end_matches('*');
                    if scope.starts_with(prefix) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Check if the agent can write to a Tier 2 memory scope.
    pub fn can_write_memory_scope(&self, scope: &str) -> bool {
        for cap in &self.manifest.capabilities {
            if let Capability::MemoryWrite { scopes } = cap {
                // Wildcard allows all scopes
                if scopes
                    .iter()
                    .any(|s| s == "*" || s.trim_end_matches('*').is_empty())
                {
                    return true;
                }
                for allowed_scope in scopes {
                    let prefix = allowed_scope.trim_end_matches('*');
                    if scope.starts_with(prefix) || scope == allowed_scope {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Check if the agent can read from a Tier 2 memory scope.
    pub fn can_read_memory_scope(&self, scope: &str) -> bool {
        for cap in &self.manifest.capabilities {
            if let Capability::MemoryRead { scopes } = cap {
                // Wildcard allows all scopes
                if scopes
                    .iter()
                    .any(|s| s == "*" || s.trim_end_matches('*').is_empty())
                {
                    return true;
                }
                for allowed_scope in scopes {
                    let prefix = allowed_scope.trim_end_matches('*');
                    if scope.starts_with(prefix) || scope == allowed_scope {
                        return true;
                    }
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use autonoetic_types::agent::{AgentIdentity, AgentManifest, RuntimeDeclaration};

    fn manifest_with_caps(capabilities: Vec<Capability>) -> AgentManifest {
        AgentManifest {
            version: "1.0".to_string(),
            runtime: RuntimeDeclaration {
                engine: "autonoetic".to_string(),
                gateway_version: "0.1.0".to_string(),
                sdk_version: "0.1.0".to_string(),
                runtime_type: "stateful".to_string(),
                sandbox: "bubblewrap".to_string(),
                runtime_lock: "runtime.lock".to_string(),
            },
            agent: AgentIdentity {
                id: "policy-test".to_string(),
                name: "policy-test".to_string(),
                description: "test".to_string(),
            },
            capabilities,
            llm_config: None,
            limits: None,
            background: None,
            disclosure: None,
            adaptation_hooks: None,
            io: None,
            middleware: None,
        }
    }

    #[test]
    fn test_can_invoke_tool_exact_and_wildcard() {
        let manifest = manifest_with_caps(vec![Capability::ToolInvoke {
            allowed: vec!["mcp_web_search".to_string(), "mcp_docs_*".to_string()],
        }]);
        let policy = PolicyEngine::new(manifest);

        assert!(policy.can_invoke_tool("mcp_web_search"));
        assert!(policy.can_invoke_tool("mcp_docs_fetch"));
        assert!(!policy.can_invoke_tool("mcp_web_fetch"));
    }

    #[test]
    fn test_can_invoke_tool_denied_without_capability() {
        let manifest = manifest_with_caps(vec![Capability::MemoryRead {
            scopes: vec!["*".to_string()],
        }]);
        let policy = PolicyEngine::new(manifest);
        assert!(!policy.can_invoke_tool("mcp_web_search"));
    }
}
