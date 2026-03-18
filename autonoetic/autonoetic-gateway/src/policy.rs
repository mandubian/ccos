//! Capability Policy Engine.
//!
//! Provides security validation for agent actions including:
//! - Command pattern matching against capability restrictions
//! - Security analysis for dangerous commands
//! - Path access validation for file operations

use autonoetic_types::agent::AgentManifest;
use autonoetic_types::capability::Capability;

/// Security threat categories for command analysis.
#[derive(Debug, Clone, PartialEq)]
pub enum SecurityThreat {
    /// Command can destroy data or filesystem (e.g., rm -rf /, dd)
    Destructive,
    /// Command attempts privilege escalation (e.g., sudo, su)
    PrivilegeEscalation,
    /// Command reads or prints environment/process secrets (e.g., env, printenv)
    EnvironmentDisclosure,
    /// Command may exfiltrate data or make unauthorized network calls
    NetworkExfiltration,
    /// Command attempts to escape sandbox (e.g., accessing /proc, /sys)
    SandboxEscape,
    /// Command may cause resource exhaustion (e.g., fork bomb)
    ResourceExhaustion,
    /// Command contains shell injection patterns (e.g., $(...), eval)
    ShellInjection,
    /// Command executes code from string/pipe (e.g., python -c, bash -c)
    CodeFromInput,
}

/// Result of security analysis.
#[derive(Debug, Clone)]
pub struct SecurityAnalysis {
    pub is_safe: bool,
    pub threats: Vec<SecurityThreat>,
    pub reason: Option<String>,
}

/// Analyzes shell commands for security threats.
pub struct SecurityAnalyzer;

impl SecurityAnalyzer {
    /// Analyze a command for security threats.
    /// Returns Analysis with threats found and whether it's safe to execute.
    pub fn analyze_command(command: &str) -> SecurityAnalysis {
        let mut threats = Vec::new();

        // Split command by shell separators to analyze each part
        let segments: Vec<&str> = command
            .split(|c| c == '|' || c == '&' || c == ';')
            .collect();

        for segment in &segments {
            let trimmed = segment.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Check for destructive commands
            if Self::is_destructive(trimmed) {
                threats.push(SecurityThreat::Destructive);
            }

            // Check for privilege escalation
            if Self::is_privilege_escalation(trimmed) {
                threats.push(SecurityThreat::PrivilegeEscalation);
            }

            // Check for environment disclosure patterns
            if Self::is_environment_disclosure(trimmed) {
                threats.push(SecurityThreat::EnvironmentDisclosure);
            }

            // Check for sandbox escape attempts
            if Self::is_sandbox_escape(trimmed) {
                threats.push(SecurityThreat::SandboxEscape);
            }

            // Check for shell injection
            if Self::is_shell_injection(trimmed) {
                threats.push(SecurityThreat::ShellInjection);
            }

            // Check for code execution from input
            if Self::is_code_from_input(trimmed) {
                threats.push(SecurityThreat::CodeFromInput);
            }

            // Check for resource exhaustion
            if Self::is_resource_exhaustion(trimmed) {
                threats.push(SecurityThreat::ResourceExhaustion);
            }
        }

        let is_safe = threats.is_empty();
        let reason = if !threats.is_empty() {
            Some(format!("Command contains security threats: {:?}", threats))
        } else {
            None
        };

        SecurityAnalysis {
            is_safe,
            threats,
            reason,
        }
    }

    /// Check for destructive commands that can destroy data.
    fn is_destructive(cmd: &str) -> bool {
        let cmd_lower = cmd.to_lowercase();

        // Block direct destructive shell/file operations, even outside extreme forms like rm -rf /
        if Self::contains_shell_word(&cmd_lower, "rm")
            || Self::contains_shell_word(&cmd_lower, "rmdir")
            || Self::contains_shell_word(&cmd_lower, "unlink")
            || cmd_lower.contains("find ") && cmd_lower.contains(" -delete")
        {
            return true;
        }

        let destructive_patterns = &[
            "dd if=",
            "dd of=/dev/",
            "mkfs",
            "format ",
            ":(){ :|:& };:",
            "> /dev/",
            "shred ",
            "wipefs",
        ];

        destructive_patterns.iter().any(|p| cmd_lower.contains(p))
    }

    /// Check for privilege escalation attempts.
    fn is_privilege_escalation(cmd: &str) -> bool {
        let cmd_lower = cmd.to_lowercase();

        if Self::contains_shell_word(&cmd_lower, "sudo")
            || Self::contains_shell_word(&cmd_lower, "su")
            || Self::contains_shell_word(&cmd_lower, "doas")
        {
            return true;
        }

        let escalation_patterns = &[
            "setuid",
            "setgid",
            "chmod +s",
            "chmod u+s",
            "chown root",
            "visudo",
        ];

        escalation_patterns.iter().any(|p| cmd_lower.contains(p))
    }

    /// Check for environment disclosure patterns.
    fn is_environment_disclosure(cmd: &str) -> bool {
        let cmd_lower = cmd.to_lowercase();

        if Self::contains_shell_word(&cmd_lower, "env")
            || cmd_lower.contains("printenv")
            || cmd_lower.contains("declare -x")
            || cmd_lower.contains("/proc/self/environ")
            || cmd_lower.contains("/proc/1/environ")
            || cmd_lower.contains("/etc/environment")
        {
            return true;
        }

        false
    }

    /// Check for sandbox escape attempts.
    fn is_sandbox_escape(cmd: &str) -> bool {
        let escape_patterns = &[
            "cat /proc/",
            "ls /proc/",
            "cat /sys/",
            "ls /sys/",
            "mount",
            "umount",
            "chroot",
            "nsenter",
            "unshare",
            "docker ",
            "lxc-",
            "systemctl",
            "service ",
        ];

        let cmd_lower = cmd.to_lowercase();
        escape_patterns.iter().any(|p| cmd_lower.contains(p))
    }

    /// Check for shell injection patterns.
    fn is_shell_injection(cmd: &str) -> bool {
        // Check for $(...) but allow $VAR in quotes
        if cmd.contains("$(") || cmd.contains("`") {
            // Allow common safe patterns like $(pwd), $(dirname $0) in scripts
            // For now, flag as potential threat - can be refined
            let safe_patterns = ["$(pwd)", "$(dirname", "$(basename"];
            if !safe_patterns.iter().any(|p| cmd.contains(p)) {
                return true;
            }
        }

        // Check for eval with user input
        if cmd.contains("eval ") {
            return true;
        }

        false
    }

    /// Check for code execution from string input (high risk).
    /// Note: python3 -c, bash -c, sh -c are NOT flagged here because they're
    /// already controlled by CodeExecution capability patterns.
    fn is_code_from_input(cmd: &str) -> bool {
        let code_patterns = &[
            // Less common/higher risk patterns
            "node -e ",
            "node --eval ",
            "perl -e ",
            "ruby -e ",
            "php -r ",
            "lua -e ",
        ];

        code_patterns.iter().any(|p| cmd.contains(p))
    }

    /// Check for resource exhaustion attacks.
    fn is_resource_exhaustion(cmd: &str) -> bool {
        let exhaustion_patterns = &[
            ":(){ :|:& };:", // Fork bomb
            "while true",
            "while :",
            "for (( ;; ))",
            "ulimit -c unlimited",
        ];

        exhaustion_patterns.iter().any(|p| cmd.contains(p))
    }

    fn contains_shell_word(cmd: &str, word: &str) -> bool {
        let mut offset = 0usize;
        while let Some(found) = cmd[offset..].find(word) {
            let start = offset + found;
            let end = start + word.len();

            let prev = if start == 0 {
                None
            } else {
                cmd[..start].chars().next_back()
            };
            let next = if end >= cmd.len() {
                None
            } else {
                cmd[end..].chars().next()
            };

            let prev_is_boundary = prev.map(Self::is_word_boundary).unwrap_or(true);
            let next_is_boundary = next.map(Self::is_word_boundary).unwrap_or(true);

            if prev_is_boundary && next_is_boundary {
                return true;
            }
            offset = end;
        }
        false
    }

    fn is_word_boundary(ch: char) -> bool {
        !ch.is_ascii_alphanumeric() && ch != '_'
    }

    /// Analyze Python script content for security threats.
    /// Returns threats found in the script code itself.
    pub fn analyze_script_content(script_content: &str) -> Vec<SecurityThreat> {
        let mut threats = Vec::new();

        // Network access patterns in Python
        let network_patterns = &[
            "urllib.request",
            "urllib.urlopen",
            "requests.get",
            "requests.post",
            "http.client",
            "httpx",
            "aiohttp",
            "socket.socket",
            "subprocess",
            "os.system",
            "os.popen",
        ];

        for pattern in network_patterns {
            if script_content.contains(pattern) {
                threats.push(SecurityThreat::NetworkExfiltration);
                break;
            }
        }

        // Code execution patterns
        let exec_patterns = &[
            "eval(",
            "exec(",
            "__import__(",
            "compile(",
            "getattr(__builtins__",
        ];

        for pattern in exec_patterns {
            if script_content.contains(pattern) {
                threats.push(SecurityThreat::ShellInjection);
                break;
            }
        }

        // File system destruction
        let fs_patterns = &[
            "shutil.rmtree",
            "os.remove(\"/\")",
            "os.unlink",
            "open('/dev/",
        ];

        for pattern in fs_patterns {
            if script_content.contains(pattern) {
                threats.push(SecurityThreat::Destructive);
                break;
            }
        }

        threats
    }

    /// Check if a Python script needs approval based on its content.
    /// Returns Some(reason) if approval is required, None if safe.
    pub fn script_requires_approval(
        script_content: &str,
        has_network_access: bool,
    ) -> Option<String> {
        let threats = Self::analyze_script_content(script_content);

        if threats.is_empty() {
            return None;
        }

        // Check if NetworkAccess capability would cover network calls
        if threats.contains(&SecurityThreat::NetworkExfiltration) && !has_network_access {
            return Some(
                "Script makes network calls but agent lacks NetworkAccess capability".to_string(),
            );
        }

        // Always require approval for these threats
        if threats.contains(&SecurityThreat::ShellInjection) {
            return Some("Script uses eval/exec which could be dangerous".to_string());
        }

        if threats.contains(&SecurityThreat::Destructive) {
            return Some("Script performs potentially destructive file operations".to_string());
        }

        None
    }
}

/// Validates requested actions against the Agent's configured capabilities.
pub struct PolicyEngine {
    manifest: AgentManifest,
}

impl PolicyEngine {
    pub fn new(manifest: AgentManifest) -> Self {
        Self { manifest }
    }

    /// Check if the agent is allowed to execute a given command string.
    /// First runs security analysis, then checks against capability patterns.
    /// Returns (allowed, Option<SecurityAnalysis>) - analysis is Some if rejected.
    pub fn can_exec_shell_detailed(&self, command: &str) -> (bool, Option<SecurityAnalysis>) {
        // First, run security analysis
        let security = SecurityAnalyzer::analyze_command(command);
        if !security.is_safe {
            return (false, Some(security));
        }

        // Then check against capability patterns
        for cap in &self.manifest.capabilities {
            if let Capability::CodeExecution { patterns } = cap {
                let command_segments: Vec<&str> = command
                    .split(|c| c == '|' || c == '&' || c == ';')
                    .collect();

                for pattern in patterns {
                    let prefix = pattern.trim_end_matches('*');

                    for segment in &command_segments {
                        let trimmed = segment.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        if trimmed.starts_with(prefix) {
                            return (true, None);
                        }
                    }
                }
            }
        }

        (false, None)
    }

    /// Check if the agent is allowed to execute a given command string.
    pub fn can_exec_shell(&self, command: &str) -> bool {
        self.can_exec_shell_detailed(command).0
    }

    /// Check if the agent is allowed to connect to a specific host.
    pub fn can_connect_net(&self, host: &str) -> bool {
        for cap in &self.manifest.capabilities {
            if let Capability::NetworkAccess { hosts } = cap {
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
            if let Capability::SandboxFunctions { allowed } = cap {
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
            if let Capability::ReadAccess { scopes } = cap {
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
            if let Capability::WriteAccess { scopes } = cap {
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
    /// Sharing is included in WriteAccess capability.
    pub fn can_share_memory(&self, _target_agent: &str) -> bool {
        // Sharing requires WriteAccess capability - check write scopes
        for cap in &self.manifest.capabilities {
            if let Capability::WriteAccess { scopes } = cap {
                // If has broad write access, can share
                if scopes.iter().any(|s| s == "*") {
                    return true;
                }
            }
        }
        false
    }

    /// Check if the agent is allowed to search memory.
    /// Searching is included in ReadAccess capability.
    pub fn can_search_memory(&self, scope: &str) -> bool {
        // Search uses the same scopes as read
        self.can_read_memory_scope(scope)
    }

    /// Check if the agent can write to a Tier 2 memory scope.
    pub fn can_write_memory_scope(&self, scope: &str) -> bool {
        for cap in &self.manifest.capabilities {
            if let Capability::WriteAccess { scopes } = cap {
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
            if let Capability::ReadAccess { scopes } = cap {
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
            io: None,
            middleware: None,
            execution_mode: Default::default(),
            script_entry: None,
            gateway_url: None,
            gateway_token: None,
        }
    }

    #[test]
    fn test_can_invoke_tool_exact_and_wildcard() {
        let manifest = manifest_with_caps(vec![Capability::SandboxFunctions {
            allowed: vec!["mcp_web_search".to_string(), "mcp_docs_*".to_string()],
        }]);
        let policy = PolicyEngine::new(manifest);

        assert!(policy.can_invoke_tool("mcp_web_search"));
        assert!(policy.can_invoke_tool("mcp_docs_fetch"));
        assert!(!policy.can_invoke_tool("mcp_web_fetch"));
    }

    #[test]
    fn test_can_invoke_tool_denied_without_capability() {
        let manifest = manifest_with_caps(vec![Capability::ReadAccess {
            scopes: vec!["*".to_string()],
        }]);
        let policy = PolicyEngine::new(manifest);
        assert!(!policy.can_invoke_tool("mcp_web_search"));
    }

    // SecurityAnalyzer tests
    #[test]
    fn test_security_analyzer_clean_command() {
        let analysis = SecurityAnalyzer::analyze_command("python3 script.py");
        assert!(analysis.is_safe);
        assert!(analysis.threats.is_empty());
    }

    #[test]
    fn test_security_analyzer_pipe_command() {
        let analysis = SecurityAnalyzer::analyze_command("echo hello | python3 process.py");
        assert!(analysis.is_safe);
    }

    #[test]
    fn test_security_analyzer_destructive_rm() {
        let analysis = SecurityAnalyzer::analyze_command("rm -rf /");
        assert!(!analysis.is_safe);
        assert!(analysis.threats.contains(&SecurityThreat::Destructive));
    }

    #[test]
    fn test_security_analyzer_destructive_rm_file() {
        let analysis = SecurityAnalyzer::analyze_command("rm /tmp/test.txt");
        assert!(!analysis.is_safe);
        assert!(analysis.threats.contains(&SecurityThreat::Destructive));
    }

    #[test]
    fn test_security_analyzer_destructive_dd() {
        let analysis = SecurityAnalyzer::analyze_command("dd if=/dev/zero of=/dev/sda");
        assert!(!analysis.is_safe);
        assert!(analysis.threats.contains(&SecurityThreat::Destructive));
    }

    #[test]
    fn test_security_analyzer_privilege_escalation() {
        let analysis = SecurityAnalyzer::analyze_command("sudo rm /etc/passwd");
        assert!(!analysis.is_safe);
        assert!(analysis
            .threats
            .contains(&SecurityThreat::PrivilegeEscalation));
    }

    #[test]
    fn test_security_analyzer_environment_disclosure_env() {
        let analysis = SecurityAnalyzer::analyze_command("bash -c 'env'");
        assert!(!analysis.is_safe);
        assert!(analysis
            .threats
            .contains(&SecurityThreat::EnvironmentDisclosure));
    }

    #[test]
    fn test_security_analyzer_environment_disclosure_printenv() {
        let analysis = SecurityAnalyzer::analyze_command("printenv");
        assert!(!analysis.is_safe);
        assert!(analysis
            .threats
            .contains(&SecurityThreat::EnvironmentDisclosure));
    }

    #[test]
    fn test_security_analyzer_sandbox_escape() {
        let analysis = SecurityAnalyzer::analyze_command("cat /proc/self/status");
        assert!(!analysis.is_safe);
        assert!(analysis.threats.contains(&SecurityThreat::SandboxEscape));
    }

    #[test]
    fn test_security_analyzer_code_from_input() {
        // python3 -c is allowed (controlled by CodeExecution patterns)
        // but node -e is still blocked as high risk
        let analysis =
            SecurityAnalyzer::analyze_command("node -e 'require(\"child_process\").exec(\"ls\")'");
        assert!(!analysis.is_safe);
        assert!(analysis.threats.contains(&SecurityThreat::CodeFromInput));
    }

    #[test]
    fn test_security_analyzer_python_c_allowed() {
        // python3 -c should NOT be flagged - controlled by CodeExecution patterns
        let analysis = SecurityAnalyzer::analyze_command("python3 -c 'print(\"hello\")'");
        assert!(analysis.is_safe);
    }

    #[test]
    fn test_security_analyzer_pipe_with_safe_python() {
        // This is the case that was failing - piped python should be safe
        let analysis = SecurityAnalyzer::analyze_command(
            "echo '{\"place\": \"London\"}' | python3 weather.py",
        );
        assert!(analysis.is_safe);
    }

    #[test]
    fn test_policy_allows_safe_bash_when_pattern_matches() {
        let manifest = manifest_with_caps(vec![Capability::CodeExecution {
            patterns: vec!["bash -c ".to_string()],
        }]);
        let policy = PolicyEngine::new(manifest);

        let (allowed, analysis) = policy.can_exec_shell_detailed("bash -c 'printf hello'");
        assert!(allowed);
        assert!(analysis.is_none());
    }

    #[test]
    fn test_policy_denies_bash_rm_even_when_pattern_matches() {
        let manifest = manifest_with_caps(vec![Capability::CodeExecution {
            patterns: vec!["bash -c ".to_string()],
        }]);
        let policy = PolicyEngine::new(manifest);

        let (allowed, analysis) = policy.can_exec_shell_detailed("bash -c 'rm /tmp/a'");
        assert!(!allowed);
        let analysis = analysis.expect("security analysis should be present for denial");
        assert!(analysis.threats.contains(&SecurityThreat::Destructive));
    }

    #[test]
    fn test_policy_denies_bash_printenv_even_when_pattern_matches() {
        let manifest = manifest_with_caps(vec![Capability::CodeExecution {
            patterns: vec!["bash -c ".to_string()],
        }]);
        let policy = PolicyEngine::new(manifest);

        let (allowed, analysis) = policy.can_exec_shell_detailed("bash -c 'printenv'");
        assert!(!allowed);
        let analysis = analysis.expect("security analysis should be present for denial");
        assert!(analysis
            .threats
            .contains(&SecurityThreat::EnvironmentDisclosure));
    }
}
