use crate::llm::ToolDefinition;
use crate::policy::PolicyEngine;
use crate::runtime::reevaluation_state::{execute_scheduled_action, persist_reevaluation_state};
use crate::sandbox::{
    DependencyPlan, DependencyRuntime, SandboxDriverKind, SandboxMount, SandboxRunner,
};
use autonoetic_types::agent::{AgentIdentity, AgentManifest, ExecutionMode, LlmConfig};
use autonoetic_types::background::{
    ApprovalRequest, BackgroundMode, BackgroundPolicy, BackgroundState, ScheduledAction,
};
use autonoetic_types::capability::Capability;
use autonoetic_types::config::{
    AgentInstallApprovalPolicy, GatewayConfig, SchemaEnforcementConfig, SchemaEnforcementMode,
};
use autonoetic_types::runtime_lock::{
    LockedDependencySet, LockedGateway, LockedSandbox, LockedSdk, RuntimeLock,
};
use autonoetic_types::schema_enforcement::{default_enforcer, EnforcementResult, SchemaEnforcer};
use autonoetic_types::tool_error::tagged;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration as StdDuration, Instant};

/// Metadata extracted from a tool call for disclosure, audit, and logging.
#[derive(Debug, Default)]
pub struct ToolMetadata {
    pub path: Option<String>,
}

fn validate_relative_agent_path(path: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!path.trim().is_empty(), "path must not be empty");
    anyhow::ensure!(
        !path.starts_with('/')
            && !path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == ".."),
        "path must stay within the agent directory"
    );
    Ok(())
}

fn validate_agent_id(agent_id: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!agent_id.trim().is_empty(), "agent_id must not be empty");
    anyhow::ensure!(
        !agent_id.starts_with('.') && !agent_id.ends_with('.') && !agent_id.contains(".."),
        "agent_id must not start or end with '.', or contain '..'"
    );
    anyhow::ensure!(
        agent_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.'),
        "agent_id may only contain ASCII letters, digits, '.', '-' and '_'"
    );
    Ok(())
}

/// Loads session content from the content store and creates sandbox mounts.
/// Each content file is mounted at its original path inside the sandbox.
fn load_session_content_mounts(
    gateway_dir: Option<&Path>,
    session_id: &str,
) -> anyhow::Result<Vec<SandboxMount>> {
    let Some(gw_dir) = gateway_dir else {
        return Ok(Vec::new());
    };

    let store = match crate::runtime::content_store::ContentStore::new(gw_dir) {
        Ok(s) => s,
        Err(_) => return Ok(Vec::new()),
    };

    let mut mounts = Vec::new();

    // Get all content names in this session
    let names_with_handles = match store.list_names_with_handles(session_id) {
        Ok(n) => n,
        Err(_) => return Ok(Vec::new()),
    };

    for (name, handle) in names_with_handles {
        // Read the content from the store
        let content = match store.read(&handle) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Create a temporary file with the content
        // The temp file path: /tmp/autonoetic_content/{session_id}/{name}
        let temp_base = std::env::temp_dir()
            .join("autonoetic_content")
            .join(session_id.replace('/', "_"));

        if let Err(_) = std::fs::create_dir_all(&temp_base) {
            continue;
        }

        let temp_file = temp_base.join(&name);
        if let Some(parent) = temp_file.parent() {
            if let Err(_) = std::fs::create_dir_all(parent) {
                continue;
            }
        }

        if let Err(_) = std::fs::write(&temp_file, &content) {
            continue;
        }

        // Mount at the original path inside sandbox (/tmp/{name})
        let dest_path = format!("/tmp/{}", name);

        mounts.push(SandboxMount {
            source: temp_file,
            dest: dest_path,
        });

        tracing::debug!(
            target: "sandbox",
            name = %name,
            handle = %handle,
            "Mounted session content file into sandbox"
        );
    }

    Ok(mounts)
}

fn background_state_file_for_child(agent_dir: &Path, child_id: &str) -> anyhow::Result<PathBuf> {
    let agents_dir = agent_dir
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Agent directory is missing its agents root parent"))?;
    Ok(agents_dir
        .join(".gateway")
        .join("scheduler")
        .join("agents")
        .join(format!("{child_id}.json")))
}

fn default_true() -> bool {
    true
}

fn requires_promotion_gate(agent_id: &str) -> bool {
    matches!(
        agent_id,
        "specialized_builder.default" | "evolution-steward.default"
    )
}

/// Returns true if the agent is an evolution role that can install other agents.
/// Only evolution roles should have access to agent.install to prevent unauthorized agent creation.
fn is_evolution_role(agent_id: &str) -> bool {
    matches!(
        agent_id,
        "specialized_builder.default" | "evolution-steward.default"
    )
}

/// Classifies an install as high-risk for approval policy. When true, risk_based policy requires human approval.
fn is_install_high_risk(
    args: &InstallAgentArgs,
    scheduled_action: &Option<ScheduledAction>,
    background: &Option<BackgroundPolicy>,
) -> bool {
    // Broad or powerful capabilities
    for cap in &args.capabilities {
        match cap {
            Capability::CodeExecution { .. } => return true,
            Capability::WriteAccess { scopes }
                if scopes.len() > 2 || scopes.iter().any(|s| s == "*" || s.ends_with("/*")) =>
            {
                return true
            }
            Capability::NetworkAccess { hosts } if !hosts.is_empty() => return true,
            _ => {}
        }
    }
    // Background-enabled installs are higher risk
    if background.as_ref().map(|b| b.enabled).unwrap_or(false) {
        return true;
    }
    // Scheduled action that runs shell or writes files
    if matches!(
        scheduled_action,
        Some(ScheduledAction::SandboxExec { .. }) | Some(ScheduledAction::WriteFile { .. })
    ) {
        return true;
    }
    false
}

fn install_request_fingerprint(
    args: &InstallAgentArgs,
    scheduled_action: &Option<ScheduledAction>,
    background: &Option<BackgroundPolicy>,
) -> anyhow::Result<String> {
    let payload = serde_json::json!({
        "agent_id": args.agent_id,
        "name": args.name,
        "description": args.description,
        "instructions": args.instructions,
        "llm_config": args.llm_config,
        "capabilities": args.capabilities,
        "background": background,
        "scheduled_action": scheduled_action,
        "files": args.files,
        "runtime_lock_dependencies": args.runtime_lock_dependencies,
        "arm_immediately": args.arm_immediately,
        "validate_on_install": args.validate_on_install
    });
    let canonical = serde_json::to_string(&payload)?;
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    Ok(format!("{:x}", hasher.finalize()))
}

fn extract_host(url: &str) -> anyhow::Result<String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| {
        anyhow::Error::from(tagged::Tagged::validation(anyhow::anyhow!(
            "Invalid URL '{}': {}",
            url,
            e
        )))
    })?;
    let host = parsed.host_str().ok_or_else(|| {
        anyhow::Error::from(tagged::Tagged::validation(anyhow::anyhow!(
            "URL '{}' does not contain a host",
            url
        )))
    })?;
    Ok(host.to_string())
}

/// Stores the install payload alongside the approval request for deterministic retry.
/// This ensures that when the caller retries with install_approval_ref, the gateway
/// can use the exact same payload that was approved (avoiding fingerprint mismatch).
fn store_install_payload(
    config: &GatewayConfig,
    request_id: &str,
    args: &InstallAgentArgs,
) -> anyhow::Result<()> {
    let payload_path = crate::scheduler::store::pending_approvals_dir(config)
        .join(format!("{request_id}_payload.json"));
    if let Some(parent) = payload_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let payload_json = serde_json::to_string_pretty(args)?;
    std::fs::write(&payload_path, payload_json)?;
    tracing::info!(
        target: "agent.install",
        request_id = %request_id,
        path = %payload_path.display(),
        "Stored install payload for retry"
    );
    Ok(())
}

/// Loads a stored install payload by request_id.
/// Returns None if the payload file doesn't exist (backward compatibility).
fn load_install_payload(
    config: &GatewayConfig,
    request_id: &str,
) -> anyhow::Result<Option<InstallAgentArgs>> {
    let payload_path = crate::scheduler::store::pending_approvals_dir(config)
        .join(format!("{request_id}_payload.json"));
    if !payload_path.exists() {
        tracing::debug!(
            target: "agent.install",
            request_id = %request_id,
            "No stored payload found, using incoming args"
        );
        return Ok(None);
    }
    let payload_json = std::fs::read_to_string(&payload_path)?;
    let args: InstallAgentArgs = serde_json::from_str(&payload_json)?;
    tracing::info!(
        target: "agent.install",
        request_id = %request_id,
        "Loaded stored install payload for retry"
    );
    Ok(Some(args))
}

/// Cleans up a stored install payload after successful install.
fn cleanup_install_payload(config: &GatewayConfig, request_id: &str) {
    let payload_path = crate::scheduler::store::pending_approvals_dir(config)
        .join(format!("{request_id}_payload.json"));
    if payload_path.exists() {
        if let Err(e) = std::fs::remove_file(&payload_path) {
            tracing::debug!(
                target: "agent.install",
                path = %payload_path.display(),
                error = %e,
                "Failed to cleanup stored payload"
            );
        } else {
            tracing::info!(
                target: "agent.install",
                request_id = %request_id,
                "Cleaned up stored install payload"
            );
        }
    }
}

fn block_on_http<F, T>(future: F) -> anyhow::Result<T>
where
    F: std::future::Future<Output = anyhow::Result<T>>,
{
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        tokio::task::block_in_place(|| handle.block_on(future))
    } else {
        tokio::runtime::Runtime::new()?.block_on(future)
    }
}

fn capabilities_are_empty(capabilities: &&[Capability]) -> bool {
    capabilities.is_empty()
}

fn is_default_execution_mode(mode: &ExecutionMode) -> bool {
    matches!(mode, ExecutionMode::Reasoning)
}

#[derive(Debug, Serialize)]
struct StandardSkillFrontmatter<'a> {
    name: &'a str,
    description: &'a str,
    metadata: StandardMetadataRoot<'a>,
}

#[derive(Debug, Serialize)]
struct StandardMetadataRoot<'a> {
    autonoetic: StandardAutonoeticMetadata<'a>,
}

#[derive(Debug, Serialize)]
struct StandardAutonoeticMetadata<'a> {
    version: &'a str,
    runtime: &'a autonoetic_types::agent::RuntimeDeclaration,
    agent: &'a AgentIdentity,
    #[serde(skip_serializing_if = "capabilities_are_empty")]
    capabilities: &'a [Capability],
    #[serde(skip_serializing_if = "Option::is_none")]
    llm_config: &'a Option<LlmConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    limits: &'a Option<autonoetic_types::agent::ResourceLimits>,
    #[serde(skip_serializing_if = "Option::is_none")]
    background: &'a Option<BackgroundPolicy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    disclosure: &'a Option<autonoetic_types::disclosure::DisclosurePolicy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    io: &'a Option<autonoetic_types::agent::AgentIO>,
    #[serde(skip_serializing_if = "Option::is_none")]
    middleware: &'a Option<autonoetic_types::agent::Middleware>,
    #[serde(skip_serializing_if = "is_default_execution_mode")]
    execution_mode: ExecutionMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    script_entry: &'a Option<String>,
}

fn render_skill_frontmatter(manifest: &AgentManifest) -> anyhow::Result<String> {
    let frontmatter = StandardSkillFrontmatter {
        name: &manifest.agent.id,
        description: &manifest.agent.description,
        metadata: StandardMetadataRoot {
            autonoetic: StandardAutonoeticMetadata {
                version: &manifest.version,
                runtime: &manifest.runtime,
                agent: &manifest.agent,
                capabilities: &manifest.capabilities,
                llm_config: &manifest.llm_config,
                limits: &manifest.limits,
                background: &manifest.background,
                disclosure: &manifest.disclosure,
                io: &manifest.io,
                middleware: &manifest.middleware,
                execution_mode: manifest.execution_mode,
                script_entry: &manifest.script_entry,
            },
        },
    };
    serde_yaml::to_string(&frontmatter).map_err(Into::into)
}

/// Defines a native, in-process tool handler.
pub trait NativeTool: Send + Sync {
    /// The exact name of the tool as it appears in LLM requests.
    fn name(&self) -> &'static str;

    /// The schema definition exposed to the LLM.
    fn definition(&self) -> ToolDefinition;

    /// Checks if the manifest/policy allows this tool to be exposed or called.
    fn is_available(&self, manifest: &AgentManifest) -> bool;

    /// Executes the tool call. `config` is provided when the gateway runs with config (e.g. for agent.install approval policy); tests may pass `None`.
    fn execute(
        &self,
        manifest: &AgentManifest,
        policy: &PolicyEngine,
        agent_dir: &Path,
        gateway_dir: Option<&Path>,
        arguments_json: &str,
        session_id: Option<&str>,
        turn_id: Option<&str>,
        config: Option<&autonoetic_types::config::GatewayConfig>,
    ) -> anyhow::Result<String>;

    /// Optionally extracts metadata from the tool's JSON arguments for disclosure policy tracking and audit.
    fn extract_metadata(&self, _arguments_json: &str) -> ToolMetadata {
        ToolMetadata::default()
    }
}

/// A thin static registry for native tool handlers.
pub struct NativeToolRegistry {
    tools: Vec<Box<dyn NativeTool>>,
}

impl NativeToolRegistry {
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    pub fn register(&mut self, tool: Box<dyn NativeTool>) {
        self.tools.push(tool);
    }

    /// Returns the definitions for all native tools available to the given agent.
    pub fn available_definitions(&self, manifest: &AgentManifest) -> Vec<ToolDefinition> {
        self.tools
            .iter()
            .filter(|t| t.is_available(manifest))
            .map(|t| t.definition())
            .collect()
    }

    /// Returns true if a native tool with the given name is registered.
    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.iter().any(|t| t.name() == name)
    }

    /// Executes a registered tool call. Enforces availability checks defensively.
    pub fn execute(
        &self,
        name: &str,
        manifest: &AgentManifest,
        policy: &PolicyEngine,
        agent_dir: &Path,
        gateway_dir: Option<&Path>,
        arguments_json: &str,
        session_id: Option<&str>,
        turn_id: Option<&str>,
        config: Option<&autonoetic_types::config::GatewayConfig>,
    ) -> anyhow::Result<String> {
        let tool = self
            .tools
            .iter()
            .find(|t| t.name() == name)
            .ok_or_else(|| anyhow::anyhow!("Unknown native tool '{}'", name))?;

        if !tool.is_available(manifest) {
            anyhow::bail!("Native tool '{}' is not available or permitted", name);
        }

        tool.execute(
            manifest,
            policy,
            agent_dir,
            gateway_dir,
            arguments_json,
            session_id,
            turn_id,
            config,
        )
    }

    /// Optionally extracts metadata from the tool's JSON arguments for disclosure policy tracking and audit.
    pub fn extract_metadata(&self, name: &str, arguments_json: &str) -> ToolMetadata {
        self.tools
            .iter()
            .find(|t| t.name() == name)
            .map(|t| t.extract_metadata(arguments_json))
            .unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// Sandbox Exec Tool
// ---------------------------------------------------------------------------

pub struct SandboxExecTool;

/// Extract code content for security analysis.
/// If running a script file (e.g., "python3 script.py"), reads the script content.
/// First checks the content store (session content), then falls back to filesystem.
/// Otherwise, returns the command itself for analysis.
fn extract_code_for_analysis(
    command: &str,
    agent_dir: &Path,
    gateway_dir: Option<&Path>,
    session_id: Option<&str>,
) -> String {
    let trimmed = command.trim();

    // Pattern: python3 /path/to/script.py or python /path/to/script.py
    for python_cmd in &["python3", "python", "python3.11", "python3.12"] {
        if trimmed.starts_with(python_cmd) || trimmed.starts_with(&format!("{} ", python_cmd)) {
            let after_python = trimmed[python_cmd.len()..].trim();

            // Skip flags like -c, -m, -u
            if after_python.starts_with('-') {
                // For "python -c 'code'", analyze the code string
                if after_python.starts_with("-c ") || after_python.starts_with("-c'") {
                    let code = after_python
                        .strip_prefix("-c")
                        .and_then(|s| s.strip_prefix(' ').or_else(|| s.strip_prefix('\'')))
                        .and_then(|s| s.strip_suffix('\'').or_else(|| s.strip_suffix('"')))
                        .unwrap_or("");
                    return code.to_string();
                }
                return command.to_string();
            }

            // Extract script path
            let script_path = after_python.split_whitespace().next().unwrap_or("");
            if script_path.is_empty() {
                return command.to_string();
            }

            // For /tmp/ paths, try to read from content store first (session content mounting)
            if script_path.starts_with("/tmp/") {
                let content_name = &script_path[5..]; // Remove "/tmp/" prefix

                // Try to read from content store
                if let (Some(gw_dir), Some(sid)) = (gateway_dir, session_id) {
                    if let Ok(store) = crate::runtime::content_store::ContentStore::new(gw_dir) {
                        if let Ok(content) =
                            store.read_by_name_or_handle_hierarchical(sid, content_name)
                        {
                            if let Ok(content_str) = String::from_utf8(content) {
                                return content_str;
                            }
                        }
                        // Also try without hierarchical (direct session lookup)
                        if let Ok(content) = store.read_by_name(sid, content_name) {
                            if let Ok(content_str) = String::from_utf8(content) {
                                return content_str;
                            }
                        }
                    }
                }

                // Fallback: map sandbox /tmp/ path to host agent_dir
                let actual_path = agent_dir.join(&script_path[5..]);
                if let Ok(content) = std::fs::read_to_string(&actual_path) {
                    return content;
                }

                return command.to_string();
            }

            // Absolute path (not /tmp/)
            if script_path.starts_with('/') {
                let actual_path = std::path::PathBuf::from(script_path);
                if let Ok(content) = std::fs::read_to_string(&actual_path) {
                    return content;
                }
            } else {
                // Relative path
                let actual_path = agent_dir.join(script_path);
                if let Ok(content) = std::fs::read_to_string(&actual_path) {
                    return content;
                }
            }

            return command.to_string();
        }
    }

    command.to_string()
}

impl NativeTool for SandboxExecTool {
    fn name(&self) -> &'static str {
        "sandbox.exec"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::CodeExecution { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Execute an approved shell command in the configured sandbox driver. If remote access is detected, an approval request is created - retry with approval_ref after approval.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "dependencies": {
                        "type": "object",
                        "properties": {
                            "runtime": { "type": "string", "enum": ["python", "nodejs", "node"] },
                            "packages": {
                                "type": "array",
                                "items": { "type": "string" },
                                "minItems": 1
                            }
                        },
                        "required": ["runtime", "packages"]
                    },
                    "approval_ref": {
                        "type": "string",
                        "description": "Approval request ID (from previous approval_required response). Provide this after operator approval to execute code with remote access."
                    }
                },
                "required": ["command"],
                "additionalProperties": false
            }),
        }
    }

    fn execute(
        &self,
        manifest: &AgentManifest,
        policy: &PolicyEngine,
        agent_dir: &Path,
        gateway_dir: Option<&Path>,
        arguments_json: &str,
        session_id: Option<&str>,
        _turn_id: Option<&str>,
        config: Option<&autonoetic_types::config::GatewayConfig>,
    ) -> anyhow::Result<String> {
        let args: SandboxExecArgs = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;

        anyhow::ensure!(
            !args.command.trim().is_empty(),
            "sandbox command must not be empty"
        );

        // Check if this is a retry with approval_ref.
        // If validated, allow this invocation to proceed without creating a
        // second remote-access approval request for the same command.
        let mut approval_validated_for_command = false;
        if let Some(approval_ref) = args.approval_ref.as_ref() {
            if let Some(cfg) = config {
                let approved_path = crate::scheduler::store::approved_approvals_dir(cfg)
                    .join(format!("{approval_ref}.json"));
                if approved_path.exists() {
                    let decision: autonoetic_types::background::ApprovalDecision =
                        crate::scheduler::store::read_json_file(&approved_path)?;
                    match &decision.action {
                        autonoetic_types::background::ScheduledAction::SandboxExec {
                            command,
                            ..
                        } if command == &args.command => {
                            tracing::info!(
                                target: "sandbox.exec",
                                approval_ref = %approval_ref,
                                "Proceeding with approved sandbox execution"
                            );
                            approval_validated_for_command = true;
                        }
                        _ => {
                            return Err(tagged::Tagged::validation(anyhow::anyhow!(
                                "approval_ref '{}' does not match this sandbox.exec command",
                                approval_ref
                            ))
                            .into());
                        }
                    }
                } else {
                    return Err(tagged::Tagged::validation(anyhow::anyhow!(
                        "approval_ref '{}' not found in approved approvals; sandbox.exec must be approved first",
                        approval_ref
                    ))
                    .into());
                }
            }
        }

        // Check policy with detailed security analysis
        let (allowed, analysis) = policy.can_exec_shell_detailed(&args.command);
        if !allowed {
            let reason = match &analysis {
                Some(a) if !a.threats.is_empty() => {
                    format!(
                        "sandbox command denied by security policy: {}",
                        a.reason.as_deref().unwrap_or("security threats detected")
                    )
                }
                _ => "sandbox command denied by CodeExecution policy".to_string(),
            };
            anyhow::bail!(reason);
        }

        // Static analysis for remote access detection
        // Analyzes both the command AND the script content (if running a script file)
        // For /tmp/ paths, reads from content store (session content mounting)
        let code_to_analyze =
            extract_code_for_analysis(&args.command, agent_dir, gateway_dir, session_id);
        let remote_analysis =
            crate::runtime::remote_access::RemoteAccessAnalyzer::analyze_code(&code_to_analyze);
        if remote_analysis.requires_approval && !approval_validated_for_command {
            tracing::warn!(
                target: "sandbox",
                patterns = ?remote_analysis.detected_patterns,
                "Code requires remote access - operator approval required"
            );

            // Create an actual approval request so operator can approve
            if let Some(cfg) = config {
                let request_id = format!("apr-{}", &uuid::Uuid::new_v4().to_string()[..8]);
                let summary = format!(
                    "Sandbox exec: {}",
                    &args.command[..args.command.len().min(60)]
                );
                let action = autonoetic_types::background::ScheduledAction::SandboxExec {
                    command: args.command.clone(),
                    dependencies: args.dependencies.as_ref().map(|d| {
                        autonoetic_types::background::ScheduledActionDependencies {
                            runtime: d.runtime.clone(),
                            packages: d.packages.clone(),
                        }
                    }),
                    requires_approval: true,
                    evidence_ref: None,
                };
                let request = autonoetic_types::background::ApprovalRequest {
                    request_id: request_id.clone(),
                    agent_id: manifest.agent.id.clone(),
                    session_id: session_id.unwrap_or("").to_string(),
                    action,
                    created_at: chrono::Utc::now().to_rfc3339(),
                    reason: Some(format!(
                        "Remote access detected: {}",
                        remote_analysis.summary
                    )),
                    evidence_ref: None,
                };
                let pending_path = crate::scheduler::store::pending_approvals_dir(cfg)
                    .join(format!("{request_id}.json"));
                if let Err(e) = std::fs::create_dir_all(pending_path.parent().unwrap()) {
                    tracing::error!(target: "sandbox", error = %e, "Failed to create approval directory");
                } else if let Err(e) =
                    crate::scheduler::store::write_json_file(&pending_path, &request)
                {
                    tracing::error!(target: "sandbox", error = %e, "Failed to create approval request");
                }

                return serde_json::to_string(&serde_json::json!({
                    "ok": false,
                    "exit_code": null,
                    "stdout": "",
                    "stderr": format!("Remote access detected: {}. Operator approval required to execute code with network access.", remote_analysis.summary),
                    "approval_required": true,
                    "request_id": request_id,
                    "remote_access_detected": true,
                    "detected_patterns": remote_analysis.detected_patterns,
                    "message": format!("To approve: 1) Get approval from operator, 2) Retry sandbox.exec with the SAME command PLUS add approval_ref = '{}' to your JSON.", request_id),
                }))
                .map_err(Into::into);
            }

            // No config available - return basic response
            return serde_json::to_string(&serde_json::json!({
                "ok": false,
                "exit_code": null,
                "stdout": "",
                "stderr": format!("Remote access detected: {}. Operator approval required to execute code with network access.", remote_analysis.summary),
                "approval_required": true,
                "remote_access_detected": true,
                "detected_patterns": remote_analysis.detected_patterns,
            }))
            .map_err(Into::into);
        }

        let dep_plan = dependency_plan_from_args_or_lock(manifest, agent_dir, args.dependencies)?;
        let driver = SandboxDriverKind::parse(&manifest.runtime.sandbox)?;
        let agent_dir_str = agent_dir
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Agent directory is not valid UTF-8"))?;

        // Load session content mounts for seamless file access in sandbox
        let session_content_mounts =
            load_session_content_mounts(gateway_dir, session_id.unwrap_or(&manifest.agent.id))?;

        let runner = if session_content_mounts.is_empty() {
            // No session content - use original spawn method
            SandboxRunner::spawn_with_driver_and_dependencies(
                driver,
                agent_dir_str,
                &args.command,
                dep_plan.as_ref(),
            )?
        } else {
            // Has session content - mount files into sandbox at their original paths
            tracing::info!(
                target: "sandbox",
                mount_count = session_content_mounts.len(),
                "Mounting session content files into sandbox"
            );
            SandboxRunner::spawn_with_session_content(
                driver,
                agent_dir_str,
                &args.command,
                dep_plan.as_ref(),
                session_content_mounts,
            )?
        };

        let output = runner.process.wait_with_output()?;
        let ok = output.status.success();
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let mut body = serde_json::json!({
            "ok": ok,
            "exit_code": output.status.code(),
            "stdout": stdout,
            "stderr": stderr
        });

        // Classify known non-retryable sandbox environment failure so agents can
        // stop looping on identical retries and route to a different capability.
        if !ok
            && body["stderr"]
                .as_str()
                .unwrap_or("")
                .contains("bwrap: loopback: Failed RTM_NEWADDR: Operation not permitted")
        {
            body["error_kind"] =
                serde_json::Value::String("sandbox_network_namespace_unavailable".to_string());
            body["retry_recommended"] = serde_json::Value::Bool(false);
            body["diagnostic"] = serde_json::Value::String(
                "Sandbox cannot configure loopback networking on this host; retrying the same sandbox.exec command is unlikely to succeed."
                    .to_string(),
            );
        }
        serde_json::to_string(&body).map_err(Into::into)
    }
}

// ---------------------------------------------------------------------------
// Web Search Tool
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct WebSearchArgs {
    query: String,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    max_results: Option<usize>,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default)]
    engine_url: Option<String>,
    #[serde(default)]
    duckduckgo_engine_url: Option<String>,
    #[serde(default)]
    google_engine_url: Option<String>,
    #[serde(default)]
    google_engine_id: Option<String>,
    #[serde(default)]
    google_api_key_env: Option<String>,
    #[serde(default)]
    google_engine_id_env: Option<String>,
    #[serde(default)]
    cache_ttl_secs: Option<u64>,
}

fn default_web_search_engine_url() -> String {
    "https://duckduckgo.com/".to_string()
}

fn default_google_search_engine_url() -> String {
    "https://www.googleapis.com/customsearch/v1".to_string()
}

const GOOGLE_API_KEY_ENV_DEFAULT: &str = "AUTONOETIC_GOOGLE_SEARCH_API_KEY";
const GOOGLE_API_KEY_ENV_LEGACY: &str = "GOOGLE_SEARCH_API_KEY";
const GOOGLE_ENGINE_ID_ENV_DEFAULT: &str = "AUTONOETIC_GOOGLE_SEARCH_ENGINE_ID";
const GOOGLE_ENGINE_ID_ENV_LEGACY: &str = "GOOGLE_SEARCH_ENGINE_ID";
const GOOGLE_ENGINE_ID_ENV_LEGACY_ALT: &str = "GOOGLE_SEARCH_CX";
const WEB_SEARCH_CACHE_TTL_DEFAULT_SECS: u64 = 120;
const WEB_SEARCH_CACHE_TTL_MAX_SECS: u64 = 3_600;

#[derive(Debug, Clone)]
struct WebSearchCacheEntry {
    expires_at: Instant,
    payload: serde_json::Value,
}

static WEB_SEARCH_CACHE: LazyLock<Mutex<HashMap<String, WebSearchCacheEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WebSearchProvider {
    Auto,
    DuckDuckGo,
    Google,
}

impl WebSearchProvider {
    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::DuckDuckGo => "duckduckgo",
            Self::Google => "google",
        }
    }
}

fn parse_web_search_provider(raw: Option<&str>) -> anyhow::Result<WebSearchProvider> {
    let normalized = raw
        .map(|value| value.trim().to_ascii_lowercase())
        .unwrap_or_else(|| "auto".to_string());
    match normalized.as_str() {
        "auto" => Ok(WebSearchProvider::Auto),
        "duckduckgo" | "ddg" => Ok(WebSearchProvider::DuckDuckGo),
        "google" => Ok(WebSearchProvider::Google),
        other => Err(anyhow::Error::from(tagged::Tagged::validation(
            anyhow::anyhow!(
                "Unsupported web.search provider '{}'. Use 'auto', 'duckduckgo', or 'google'.",
                other
            ),
        ))),
    }
}

fn resolve_duckduckgo_engine_url(args: &WebSearchArgs) -> String {
    args.duckduckgo_engine_url
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .or_else(|| {
            args.engine_url
                .as_ref()
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
                .map(|value| value.to_string())
        })
        .unwrap_or_else(default_web_search_engine_url)
}

fn resolve_google_engine_url(args: &WebSearchArgs) -> String {
    args.google_engine_url
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .or_else(|| {
            args.engine_url
                .as_ref()
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
                .map(|value| value.to_string())
        })
        .unwrap_or_else(default_google_search_engine_url)
}

fn resolve_web_search_cache_ttl_secs(args: &WebSearchArgs) -> u64 {
    args.cache_ttl_secs
        .unwrap_or(WEB_SEARCH_CACHE_TTL_DEFAULT_SECS)
        .min(WEB_SEARCH_CACHE_TTL_MAX_SECS)
}

fn web_search_cache_key(
    args: &WebSearchArgs,
    provider: WebSearchProvider,
    requested_max_results: usize,
    timeout_secs: u64,
) -> String {
    let query = args.query.trim();
    let ddg_engine_url = resolve_duckduckgo_engine_url(args);
    let google_engine_url = resolve_google_engine_url(args);
    let google_engine_id = args
        .google_engine_id
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("");
    let google_api_key_env = args
        .google_api_key_env
        .as_deref()
        .unwrap_or(GOOGLE_API_KEY_ENV_DEFAULT);
    let google_engine_id_env = args
        .google_engine_id_env
        .as_deref()
        .unwrap_or(GOOGLE_ENGINE_ID_ENV_DEFAULT);
    format!(
        "provider={}|query={}|max_results={}|timeout_secs={}|ddg_engine_url={}|google_engine_url={}|google_engine_id={}|google_api_key_env={}|google_engine_id_env={}",
        provider.as_str(),
        query,
        requested_max_results,
        timeout_secs,
        ddg_engine_url,
        google_engine_url,
        google_engine_id,
        google_api_key_env,
        google_engine_id_env
    )
}

fn web_search_cache_get(key: &str) -> Option<serde_json::Value> {
    let now = Instant::now();
    let mut cache = WEB_SEARCH_CACHE.lock().ok()?;
    cache.retain(|_, entry| entry.expires_at > now);
    cache.get(key).map(|entry| entry.payload.clone())
}

fn web_search_cache_put(key: String, payload: serde_json::Value, ttl_secs: u64) {
    if ttl_secs == 0 {
        return;
    }
    if let Ok(mut cache) = WEB_SEARCH_CACHE.lock() {
        let now = Instant::now();
        cache.retain(|_, entry| entry.expires_at > now);
        cache.insert(
            key,
            WebSearchCacheEntry {
                expires_at: now + StdDuration::from_secs(ttl_secs),
                payload,
            },
        );
    }
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name).ok().and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn resolve_google_api_key(args: &WebSearchArgs) -> anyhow::Result<String> {
    let key_env = args
        .google_api_key_env
        .as_deref()
        .unwrap_or(GOOGLE_API_KEY_ENV_DEFAULT);
    let key = non_empty_env(key_env).or_else(|| {
        if args.google_api_key_env.is_none() {
            non_empty_env(GOOGLE_API_KEY_ENV_LEGACY)
        } else {
            None
        }
    });
    key.ok_or_else(|| {
        anyhow::Error::from(tagged::Tagged::validation(anyhow::anyhow!(
            "Google web.search requires API key env '{}'",
            key_env
        )))
    })
}

fn resolve_google_engine_id(args: &WebSearchArgs) -> anyhow::Result<String> {
    if let Some(explicit) = args
        .google_engine_id
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        return Ok(explicit.to_string());
    }
    let engine_id_env = args
        .google_engine_id_env
        .as_deref()
        .unwrap_or(GOOGLE_ENGINE_ID_ENV_DEFAULT);
    let engine_id = non_empty_env(engine_id_env).or_else(|| {
        if args.google_engine_id_env.is_none() {
            non_empty_env(GOOGLE_ENGINE_ID_ENV_LEGACY)
                .or_else(|| non_empty_env(GOOGLE_ENGINE_ID_ENV_LEGACY_ALT))
        } else {
            None
        }
    });
    engine_id.ok_or_else(|| {
        anyhow::Error::from(tagged::Tagged::validation(anyhow::anyhow!(
            "Google web.search requires engine id via argument 'google_engine_id' or env '{}'",
            engine_id_env
        )))
    })
}

fn normalize_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn collect_duckduckgo_results(
    payload: &serde_json::Value,
    max_results: usize,
) -> Vec<serde_json::Value> {
    fn maybe_push(
        out: &mut Vec<serde_json::Value>,
        seen_urls: &mut HashSet<String>,
        text: &str,
        url: &str,
        max_results: usize,
    ) {
        if out.len() >= max_results {
            return;
        }
        if text.trim().is_empty() || url.trim().is_empty() {
            return;
        }
        if !seen_urls.insert(url.to_string()) {
            return;
        }
        out.push(serde_json::json!({
            "title": normalize_text(text),
            "url": url,
            "snippet": normalize_text(text),
        }));
    }

    fn walk(
        node: &serde_json::Value,
        out: &mut Vec<serde_json::Value>,
        seen_urls: &mut HashSet<String>,
        max_results: usize,
    ) {
        if out.len() >= max_results {
            return;
        }

        if let Some(obj) = node.as_object() {
            if let (Some(text), Some(url)) = (
                obj.get("Text").and_then(|v| v.as_str()),
                obj.get("FirstURL").and_then(|v| v.as_str()),
            ) {
                maybe_push(out, seen_urls, text, url, max_results);
            }
            if let Some(topics) = obj.get("Topics").and_then(|v| v.as_array()) {
                for topic in topics {
                    walk(topic, out, seen_urls, max_results);
                    if out.len() >= max_results {
                        return;
                    }
                }
            }
            return;
        }

        if let Some(arr) = node.as_array() {
            for item in arr {
                walk(item, out, seen_urls, max_results);
                if out.len() >= max_results {
                    return;
                }
            }
        }
    }

    let mut out = Vec::new();
    let mut seen_urls = HashSet::new();

    if let Some(results) = payload.get("Results").and_then(|v| v.as_array()) {
        for result in results {
            walk(result, &mut out, &mut seen_urls, max_results);
            if out.len() >= max_results {
                return out;
            }
        }
    }
    if let Some(related) = payload.get("RelatedTopics").and_then(|v| v.as_array()) {
        for topic in related {
            walk(topic, &mut out, &mut seen_urls, max_results);
            if out.len() >= max_results {
                return out;
            }
        }
    }
    out
}

fn collect_google_results(
    payload: &serde_json::Value,
    max_results: usize,
) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    let mut seen_urls = HashSet::new();
    if let Some(items) = payload.get("items").and_then(|v| v.as_array()) {
        for item in items {
            if out.len() >= max_results {
                break;
            }
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let url = item
                .get("link")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let snippet = item
                .get("snippet")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if title.trim().is_empty() || url.trim().is_empty() {
                continue;
            }
            if !seen_urls.insert(url.to_string()) {
                continue;
            }
            out.push(serde_json::json!({
                "title": normalize_text(title),
                "url": url,
                "snippet": normalize_text(snippet),
            }));
        }
    }
    out
}

#[derive(Debug)]
struct WebSearchResponse {
    provider: WebSearchProvider,
    engine_url: String,
    status_code: u16,
    results: Vec<serde_json::Value>,
    abstract_text: Option<String>,
    total_results: Option<u64>,
}

fn execute_duckduckgo_search(
    policy: &PolicyEngine,
    query: &str,
    engine_url: String,
    max_results: usize,
    timeout_secs: u64,
) -> anyhow::Result<WebSearchResponse> {
    let engine_host = extract_host(&engine_url)?;
    if !policy.can_connect_net(&engine_host) {
        return Err(anyhow::Error::from(tagged::Tagged::permission(
            anyhow::anyhow!(
                "Permission Denied: NetworkAccess does not allow host '{}'",
                engine_host
            ),
        )));
    }

    let request_engine_url = engine_url.clone();
    let request_query = query.to_string();
    let (status_code, payload) = block_on_http(async move {
        let mut request_url = reqwest::Url::parse(&request_engine_url).map_err(|e| {
            anyhow::Error::from(tagged::Tagged::validation(anyhow::anyhow!(
                "Invalid search engine URL '{}': {}",
                request_engine_url,
                e
            )))
        })?;
        {
            let mut pairs = request_url.query_pairs_mut();
            pairs.append_pair("q", request_query.as_str());
            pairs.append_pair("format", "json");
            pairs.append_pair("no_html", "1");
            pairs.append_pair("skip_disambig", "1");
        }

        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| anyhow::anyhow!("web.search client build failed: {}", e))?;
        let response = client
            .get(request_url)
            .timeout(StdDuration::from_secs(timeout_secs))
            .send()
            .await
            .map_err(|e| {
                anyhow::Error::from(tagged::Tagged::resource(anyhow::anyhow!(
                    "web.search request failed: {}",
                    e
                )))
            })?;

        let status = response.status();
        if !status.is_success() {
            return Err(anyhow::Error::from(tagged::Tagged::resource(
                anyhow::anyhow!("web.search request failed with status {}", status),
            )));
        }
        let payload = response.json::<serde_json::Value>().await.map_err(|e| {
            anyhow::Error::from(tagged::Tagged::execution(anyhow::anyhow!(
                "web.search could not decode JSON response: {}",
                e
            )))
        })?;
        Ok((status.as_u16(), payload))
    })?;

    let results = collect_duckduckgo_results(&payload, max_results);
    let abstract_text = payload
        .get("AbstractText")
        .and_then(|v| v.as_str())
        .map(normalize_text)
        .filter(|text| !text.is_empty());

    Ok(WebSearchResponse {
        provider: WebSearchProvider::DuckDuckGo,
        engine_url,
        status_code,
        results,
        abstract_text,
        total_results: None,
    })
}

fn execute_google_search(
    policy: &PolicyEngine,
    query: &str,
    engine_url: String,
    api_key: String,
    engine_id: String,
    max_results: usize,
    timeout_secs: u64,
) -> anyhow::Result<WebSearchResponse> {
    let engine_host = extract_host(&engine_url)?;
    if !policy.can_connect_net(&engine_host) {
        return Err(anyhow::Error::from(tagged::Tagged::permission(
            anyhow::anyhow!(
                "Permission Denied: NetworkAccess does not allow host '{}'",
                engine_host
            ),
        )));
    }

    let request_engine_url = engine_url.clone();
    let request_query = query.to_string();
    let (status_code, payload) = block_on_http(async move {
        let mut request_url = reqwest::Url::parse(&request_engine_url).map_err(|e| {
            anyhow::Error::from(tagged::Tagged::validation(anyhow::anyhow!(
                "Invalid search engine URL '{}': {}",
                request_engine_url,
                e
            )))
        })?;
        {
            let mut pairs = request_url.query_pairs_mut();
            pairs.append_pair("q", request_query.as_str());
            pairs.append_pair("key", api_key.as_str());
            pairs.append_pair("cx", engine_id.as_str());
            pairs.append_pair("num", &max_results.to_string());
        }

        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| anyhow::anyhow!("web.search client build failed: {}", e))?;
        let response = client
            .get(request_url)
            .timeout(StdDuration::from_secs(timeout_secs))
            .send()
            .await
            .map_err(|e| {
                anyhow::Error::from(tagged::Tagged::resource(anyhow::anyhow!(
                    "web.search request failed: {}",
                    e
                )))
            })?;
        let status = response.status();
        if !status.is_success() {
            return Err(anyhow::Error::from(tagged::Tagged::resource(
                anyhow::anyhow!("web.search request failed with status {}", status),
            )));
        }
        let payload = response.json::<serde_json::Value>().await.map_err(|e| {
            anyhow::Error::from(tagged::Tagged::execution(anyhow::anyhow!(
                "web.search could not decode JSON response: {}",
                e
            )))
        })?;
        Ok((status.as_u16(), payload))
    })?;

    if let Some(error_payload) = payload.get("error") {
        let message = error_payload
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown google search error");
        return Err(anyhow::Error::from(tagged::Tagged::execution(
            anyhow::anyhow!("web.search google provider returned error: {}", message),
        )));
    }

    let results = collect_google_results(&payload, max_results);
    let total_results = payload
        .pointer("/searchInformation/totalResults")
        .and_then(|v| v.as_str())
        .and_then(|value| value.parse::<u64>().ok());

    Ok(WebSearchResponse {
        provider: WebSearchProvider::Google,
        engine_url,
        status_code,
        results,
        abstract_text: None,
        total_results,
    })
}

fn web_search_response_to_payload(query: &str, response: WebSearchResponse) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "ok": true,
        "provider": response.provider.as_str(),
        "query": query,
        "engine_url": response.engine_url,
        "status_code": response.status_code,
        "result_count": response.results.len(),
        "results": response.results
    });
    if let Some(abstract_text) = response.abstract_text {
        payload["abstract"] = serde_json::json!(abstract_text);
    }
    if let Some(total_results) = response.total_results {
        payload["total_results"] = serde_json::json!(total_results);
    }
    payload
}

pub struct WebSearchTool;

impl NativeTool for WebSearchTool {
    fn name(&self) -> &'static str {
        "web.search"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::NetworkAccess { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description:
                "Search the web via provider-backed JSON APIs (duckduckgo, google, or auto fallback)."
                    .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "provider": { "type": "string", "enum": ["auto", "duckduckgo", "google"] },
                    "max_results": { "type": "integer", "minimum": 1, "maximum": 20 },
                    "timeout_secs": { "type": "integer", "minimum": 5, "maximum": 120 },
                    "engine_url": { "type": "string" },
                    "duckduckgo_engine_url": { "type": "string" },
                    "google_engine_url": { "type": "string" },
                    "google_engine_id": { "type": "string" },
                    "google_api_key_env": { "type": "string" },
                    "google_engine_id_env": { "type": "string" },
                    "cache_ttl_secs": { "type": "integer", "minimum": 0, "maximum": 3600 }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        }
    }

    fn execute(
        &self,
        _manifest: &AgentManifest,
        policy: &PolicyEngine,
        _agent_dir: &Path,
        _gateway_dir: Option<&Path>,
        arguments_json: &str,
        _session_id: Option<&str>,
        _turn_id: Option<&str>,
        _config: Option<&autonoetic_types::config::GatewayConfig>,
    ) -> anyhow::Result<String> {
        let args: WebSearchArgs = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;

        anyhow::ensure!(!args.query.trim().is_empty(), "query must not be empty");
        let query = args.query.trim().to_string();
        let requested_provider = parse_web_search_provider(args.provider.as_deref())?;
        let timeout_secs = args.timeout_secs.unwrap_or(20).clamp(5, 120);
        let requested_max_results = args.max_results.unwrap_or(5).clamp(1, 20);
        let cache_ttl_secs = resolve_web_search_cache_ttl_secs(&args);
        let cache_key = web_search_cache_key(
            &args,
            requested_provider,
            requested_max_results,
            timeout_secs,
        );

        if cache_ttl_secs > 0 {
            if let Some(mut cached_payload) = web_search_cache_get(&cache_key) {
                cached_payload["cache_hit"] = serde_json::json!(true);
                cached_payload["cache_ttl_secs"] = serde_json::json!(cache_ttl_secs);
                return serde_json::to_string(&cached_payload).map_err(Into::into);
            }
        }

        let mut attempted_providers = Vec::new();
        let mut fallback_reason: Option<String> = None;

        let response = match requested_provider {
            WebSearchProvider::DuckDuckGo => {
                attempted_providers.push(WebSearchProvider::DuckDuckGo.as_str().to_string());
                execute_duckduckgo_search(
                    policy,
                    &query,
                    resolve_duckduckgo_engine_url(&args),
                    requested_max_results.clamp(1, 20),
                    timeout_secs,
                )?
            }
            WebSearchProvider::Google => {
                attempted_providers.push(WebSearchProvider::Google.as_str().to_string());
                let api_key = resolve_google_api_key(&args)?;
                let engine_id = resolve_google_engine_id(&args)?;
                execute_google_search(
                    policy,
                    &query,
                    resolve_google_engine_url(&args),
                    api_key,
                    engine_id,
                    requested_max_results.clamp(1, 10),
                    timeout_secs,
                )?
            }
            WebSearchProvider::Auto => {
                let ddg_engine_url = resolve_duckduckgo_engine_url(&args);
                let google_engine_url = resolve_google_engine_url(&args);
                let ddg_max_results = requested_max_results.clamp(1, 20);
                let google_max_results = requested_max_results.clamp(1, 10);

                let google_credentials = resolve_google_api_key(&args).and_then(|api_key| {
                    resolve_google_engine_id(&args).map(|engine_id| (api_key, engine_id))
                });

                match google_credentials {
                    Ok((api_key, engine_id)) => {
                        attempted_providers.push(WebSearchProvider::Google.as_str().to_string());
                        match execute_google_search(
                            policy,
                            &query,
                            google_engine_url,
                            api_key,
                            engine_id,
                            google_max_results,
                            timeout_secs,
                        ) {
                            Ok(google_response) if !google_response.results.is_empty() => {
                                google_response
                            }
                            Ok(_) => {
                                fallback_reason = Some("google returned no results".to_string());
                                attempted_providers
                                    .push(WebSearchProvider::DuckDuckGo.as_str().to_string());
                                execute_duckduckgo_search(
                                    policy,
                                    &query,
                                    ddg_engine_url,
                                    ddg_max_results,
                                    timeout_secs,
                                )?
                            }
                            Err(google_err) => {
                                let google_error_text = google_err.to_string();
                                fallback_reason =
                                    Some(format!("google provider failed: {google_error_text}"));
                                attempted_providers
                                    .push(WebSearchProvider::DuckDuckGo.as_str().to_string());
                                match execute_duckduckgo_search(
                                    policy,
                                    &query,
                                    ddg_engine_url,
                                    ddg_max_results,
                                    timeout_secs,
                                ) {
                                    Ok(ddg_response) => ddg_response,
                                    Err(ddg_err) => {
                                        return Err(anyhow::Error::from(tagged::Tagged::resource(
                                            anyhow::anyhow!(
                                                "web.search auto provider failed: google error: {}; duckduckgo error: {}",
                                                google_error_text,
                                                ddg_err
                                            ),
                                        )));
                                    }
                                }
                            }
                        }
                    }
                    Err(_) => {
                        fallback_reason =
                            Some("google credentials unavailable; used duckduckgo".to_string());
                        attempted_providers
                            .push(WebSearchProvider::DuckDuckGo.as_str().to_string());
                        execute_duckduckgo_search(
                            policy,
                            &query,
                            ddg_engine_url,
                            ddg_max_results,
                            timeout_secs,
                        )?
                    }
                }
            }
        };

        let mut payload = web_search_response_to_payload(&query, response);
        payload["requested_provider"] = serde_json::json!(requested_provider.as_str());
        payload["attempted_providers"] = serde_json::json!(attempted_providers);
        if let Some(reason) = fallback_reason {
            payload["fallback_reason"] = serde_json::json!(reason);
        }
        payload["cache_hit"] = serde_json::json!(false);
        payload["cache_ttl_secs"] = serde_json::json!(cache_ttl_secs);

        if cache_ttl_secs > 0 {
            web_search_cache_put(cache_key, payload.clone(), cache_ttl_secs);
        }

        serde_json::to_string(&payload).map_err(Into::into)
    }
}

// ---------------------------------------------------------------------------
// Web Fetch Tool
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct WebFetchArgs {
    url: String,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default)]
    max_chars: Option<usize>,
}

pub struct WebFetchTool;

impl NativeTool for WebFetchTool {
    fn name(&self) -> &'static str {
        "web.fetch"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::NetworkAccess { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Fetch a web page by URL and return its textual payload.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string" },
                    "timeout_secs": { "type": "integer", "minimum": 5, "maximum": 120 },
                    "max_chars": { "type": "integer", "minimum": 512, "maximum": 200000 }
                },
                "required": ["url"],
                "additionalProperties": false
            }),
        }
    }

    fn execute(
        &self,
        _manifest: &AgentManifest,
        policy: &PolicyEngine,
        _agent_dir: &Path,
        _gateway_dir: Option<&Path>,
        arguments_json: &str,
        _session_id: Option<&str>,
        _turn_id: Option<&str>,
        _config: Option<&autonoetic_types::config::GatewayConfig>,
    ) -> anyhow::Result<String> {
        let args: WebFetchArgs = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;

        anyhow::ensure!(!args.url.trim().is_empty(), "url must not be empty");
        let host = extract_host(&args.url)?;
        if !policy.can_connect_net(&host) {
            return Err(anyhow::Error::from(tagged::Tagged::permission(
                anyhow::anyhow!(
                    "Permission Denied: NetworkAccess does not allow host '{}'",
                    host
                ),
            )));
        }

        let timeout_secs = args.timeout_secs.unwrap_or(20).clamp(5, 120);
        let max_chars = args.max_chars.unwrap_or(20_000).clamp(512, 200_000);
        let fetch_url = args.url.clone();
        let (status_code, content_type, body) = block_on_http(async move {
            let client = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|e| anyhow::anyhow!("web.fetch client build failed: {}", e))?;
            let response = client
                .get(&fetch_url)
                .timeout(StdDuration::from_secs(timeout_secs))
                .send()
                .await
                .map_err(|e| {
                    anyhow::Error::from(tagged::Tagged::resource(anyhow::anyhow!(
                        "web.fetch request failed: {}",
                        e
                    )))
                })?;

            let status = response.status();
            if !status.is_success() {
                return Err(anyhow::Error::from(tagged::Tagged::resource(
                    anyhow::anyhow!("web.fetch request failed with status {}", status),
                )));
            }
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(|v| v.to_string());
            let body = response.text().await.map_err(|e| {
                anyhow::Error::from(tagged::Tagged::execution(anyhow::anyhow!(
                    "web.fetch could not decode text response: {}",
                    e
                )))
            })?;
            Ok((status.as_u16(), content_type, body))
        })?;

        let total_chars = body.chars().count();
        let truncated = total_chars > max_chars;
        let content = if truncated {
            body.chars().take(max_chars).collect::<String>()
        } else {
            body
        };

        serde_json::to_string(&serde_json::json!({
            "ok": true,
            "url": args.url,
            "status_code": status_code,
            "content_type": content_type,
            "truncated": truncated,
            "total_chars": total_chars,
            "content": content
        }))
        .map_err(Into::into)
    }
}

// ---------------------------------------------------------------------------
// Content Write Tool
// ---------------------------------------------------------------------------

/// Writes content to the gateway's content-addressable store.
///
/// This is the primary tool for agents to persist files, scripts, and data.
/// Content is stored by SHA-256 hash and can be retrieved by name or handle.
pub struct ContentWriteTool;

impl NativeTool for ContentWriteTool {
    fn name(&self) -> &'static str {
        "content.write"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        // Available to any agent with WriteAccess capability
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::WriteAccess { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Write content to the session's content store. Returns a content handle (SHA-256) that can be used to retrieve the content later. Content is automatically named in the session for easy retrieval.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "A name for this content (e.g., 'main.py', 'scripts/main.py'). Supports path-like names with slashes."
                    },
                    "content": {
                        "type": "string",
                        "description": "The content to store"
                    }
                },
                "required": ["name", "content"],
                "additionalProperties": false
            }),
        }
    }

    fn execute(
        &self,
        manifest: &AgentManifest,
        _policy: &PolicyEngine,
        _agent_dir: &Path,
        gateway_dir: Option<&Path>,
        arguments_json: &str,
        session_id: Option<&str>,
        _turn_id: Option<&str>,
        _config: Option<&autonoetic_types::config::GatewayConfig>,
    ) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            name: String,
            content: String,
        }
        let args: Args = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;

        anyhow::ensure!(!args.name.trim().is_empty(), "name must not be empty");
        // Allow alphanumeric, underscores, hyphens, dots, and slashes for path-like names
        anyhow::ensure!(
            args.name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.' || c == '/'),
            "name must contain only alphanumeric characters, underscores, hyphens, dots, or slashes"
        );

        let Some(gw_dir) = gateway_dir else {
            anyhow::bail!("Content store requires gateway directory to be configured");
        };

        let sid = session_id.unwrap_or(&manifest.agent.id);
        let store = crate::runtime::content_store::ContentStore::new(gw_dir)?;

        let handle = store.write(args.content.as_bytes())?;
        // Use hierarchical registration so parent sessions can see this content
        store.register_name_in_hierarchy(sid, &args.name, &handle)?;

        // Return short alias (8 hex chars) for LLM-friendly retrieval
        let short_alias = crate::runtime::content_store::ContentStore::get_short_alias(&handle);

        serde_json::to_string(&serde_json::json!({
            "ok": true,
            "handle": handle,
            "alias": short_alias,
            "name": args.name,
            "bytes_written": args.content.len(),
        }))
        .map_err(Into::into)
    }

    fn extract_metadata(&self, arguments_json: &str) -> ToolMetadata {
        let mut meta = ToolMetadata::default();
        if let Ok(parsed_args) = serde_json::from_str::<serde_json::Value>(arguments_json) {
            if let Some(name) = parsed_args.get("name").and_then(|v| v.as_str()) {
                meta.path = Some(name.to_string());
            }
        }
        meta
    }
}

// ---------------------------------------------------------------------------
// Content Read Tool
// ---------------------------------------------------------------------------

/// Reads content from the gateway's content-addressable store.
///
/// Can read by name (session-relative) or by content handle (SHA-256).
pub struct ContentReadTool;

impl NativeTool for ContentReadTool {
    fn name(&self) -> &'static str {
        "content.read"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        // Available to any agent with ReadAccess capability
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::ReadAccess { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Read content from the session's content store. Can read by name (e.g., 'main.py') or by content handle (e.g., 'sha256:abc123...').".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name_or_handle": {
                        "type": "string",
                        "description": "The content name (e.g., 'main.py') or content handle (e.g., 'sha256:abc123...') to read"
                    }
                },
                "required": ["name_or_handle"],
                "additionalProperties": false
            }),
        }
    }

    fn execute(
        &self,
        manifest: &AgentManifest,
        _policy: &PolicyEngine,
        _agent_dir: &Path,
        gateway_dir: Option<&Path>,
        arguments_json: &str,
        session_id: Option<&str>,
        _turn_id: Option<&str>,
        _config: Option<&autonoetic_types::config::GatewayConfig>,
    ) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            name_or_handle: String,
        }
        let args: Args = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;

        anyhow::ensure!(
            !args.name_or_handle.trim().is_empty(),
            "name_or_handle must not be empty"
        );

        let Some(gw_dir) = gateway_dir else {
            anyhow::bail!("Content store requires gateway directory to be configured");
        };

        let sid = session_id.unwrap_or(&manifest.agent.id);
        let store = crate::runtime::content_store::ContentStore::new(gw_dir)?;

        // Use hierarchical lookup so parent can read child's content
        let content = store.read_by_name_or_handle_hierarchical(sid, &args.name_or_handle)?;

        let content_str = String::from_utf8(content)
            .map_err(|e| anyhow::anyhow!("Content is not valid UTF-8: {}", e))?;

        serde_json::to_string(&serde_json::json!({
            "ok": true,
            "content": content_str,
        }))
        .map_err(Into::into)
    }

    fn extract_metadata(&self, arguments_json: &str) -> ToolMetadata {
        let mut meta = ToolMetadata::default();
        if let Ok(parsed_args) = serde_json::from_str::<serde_json::Value>(arguments_json) {
            if let Some(name) = parsed_args.get("name_or_handle").and_then(|v| v.as_str()) {
                meta.path = Some(name.to_string());
            }
        }
        meta
    }
}

// ---------------------------------------------------------------------------
// Content Persist Tool
// ---------------------------------------------------------------------------

/// Marks content as persistent, surviving session cleanup.
pub struct ContentPersistTool;

impl NativeTool for ContentPersistTool {
    fn name(&self) -> &'static str {
        "content.persist"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        // Available to any agent with WriteAccess capability
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::WriteAccess { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Mark content as persistent so it survives session cleanup. Use this for artifacts that should be available to future sessions (e.g., installed agents, published skills).".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "handle": {
                        "type": "string",
                        "description": "The content handle (e.g., 'sha256:abc123...') to persist"
                    }
                },
                "required": ["handle"],
                "additionalProperties": false
            }),
        }
    }

    fn execute(
        &self,
        manifest: &AgentManifest,
        _policy: &PolicyEngine,
        _agent_dir: &Path,
        gateway_dir: Option<&Path>,
        arguments_json: &str,
        session_id: Option<&str>,
        _turn_id: Option<&str>,
        _config: Option<&autonoetic_types::config::GatewayConfig>,
    ) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            handle: String,
        }
        let args: Args = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;

        anyhow::ensure!(
            args.handle.starts_with("sha256:"),
            "handle must be a SHA-256 content handle"
        );

        let Some(gw_dir) = gateway_dir else {
            anyhow::bail!("Content store requires gateway directory to be configured");
        };

        let sid = session_id.unwrap_or(&manifest.agent.id);
        let store = crate::runtime::content_store::ContentStore::new(gw_dir)?;

        store.persist(sid, &args.handle)?;

        serde_json::to_string(&serde_json::json!({
            "ok": true,
            "handle": args.handle,
            "persisted": true,
        }))
        .map_err(Into::into)
    }
}

// ---------------------------------------------------------------------------
// Knowledge Store Tool (renamed from memory.remember)
// ---------------------------------------------------------------------------

/// Stores a durable fact in the gateway's knowledge base (Tier 2 memory).
///
/// Knowledge is stored with full provenance tracking and can be shared across agents.
pub struct KnowledgeStoreTool;

impl NativeTool for KnowledgeStoreTool {
    fn name(&self) -> &'static str {
        "knowledge.store"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        // Uses WriteAccess capability (same as memory.remember)
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::WriteAccess { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Store a durable fact in the knowledge base. Knowledge persists across sessions and can be shared with other agents. Each fact includes provenance tracking (who wrote it, when, from what source).".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Unique identifier for this knowledge" },
                    "content": { "type": "string", "description": "The fact or information to store" },
                    "scope": { "type": "string", "description": "Category/namespace for organizing knowledge (e.g., 'api-keys', 'user-preferences')", "default": "general" },
                    "tags": { "type": "array", "items": { "type": "string" }, "description": "Tags for searchability" },
                    "confidence": { "type": "number", "description": "Confidence level (0.0 to 1.0)", "default": 1.0 }
                },
                "required": ["id", "content"],
                "additionalProperties": false
            }),
        }
    }

    fn execute(
        &self,
        manifest: &AgentManifest,
        _policy: &PolicyEngine,
        _agent_dir: &Path,
        gateway_dir: Option<&Path>,
        arguments_json: &str,
        session_id: Option<&str>,
        turn_id: Option<&str>,
        _config: Option<&autonoetic_types::config::GatewayConfig>,
    ) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            id: String,
            content: String,
            #[serde(default = "default_scope")]
            scope: String,
            #[serde(default)]
            tags: Vec<String>,
            #[serde(default = "default_confidence")]
            confidence: f64,
        }
        fn default_scope() -> String {
            "general".to_string()
        }
        fn default_confidence() -> f64 {
            1.0
        }

        let args: Args = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;

        anyhow::ensure!(!args.id.trim().is_empty(), "id must not be empty");
        anyhow::ensure!(!args.content.trim().is_empty(), "content must not be empty");
        anyhow::ensure!(
            args.confidence >= 0.0 && args.confidence <= 1.0,
            "confidence must be between 0.0 and 1.0"
        );

        let Some(gw_dir) = gateway_dir else {
            anyhow::bail!("Knowledge requires gateway directory to be configured");
        };

        let sid = session_id.unwrap_or(&manifest.agent.id);
        let source_ref = match turn_id {
            Some(tid) => format!("session:{}:turn:{}", sid, tid),
            None => format!("session:{}", sid),
        };

        let mem = crate::runtime::memory::Tier2Memory::new(gw_dir, &manifest.agent.id)?;
        let memory = mem.remember(
            &args.id,
            &args.scope,
            &manifest.agent.id,
            &source_ref,
            &args.content,
        )?;

        // Apply tags if provided
        if !args.tags.is_empty() {
            // Note: tags are set during remember via MemoryObject::new
            // For now, we just return success
        }

        serde_json::to_string(&serde_json::json!({
            "ok": true,
            "id": memory.memory_id,
            "scope": memory.scope,
            "content_hash": memory.content_hash,
            "created_at": memory.created_at,
        }))
        .map_err(Into::into)
    }
}

// ---------------------------------------------------------------------------
// Knowledge Recall Tool (renamed from memory.recall)
// ---------------------------------------------------------------------------

/// Retrieves a durable fact from the knowledge base by ID.
pub struct KnowledgeRecallTool;

impl NativeTool for KnowledgeRecallTool {
    fn name(&self) -> &'static str {
        "knowledge.recall"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        // Uses ReadAccess capability (same as memory.recall)
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::ReadAccess { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Recall a durable fact from the knowledge base by its ID. Respects visibility and access control - you can only recall knowledge you have access to.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "The knowledge ID to recall" }
                },
                "required": ["id"],
                "additionalProperties": false
            }),
        }
    }

    fn execute(
        &self,
        manifest: &AgentManifest,
        _policy: &PolicyEngine,
        _agent_dir: &Path,
        gateway_dir: Option<&Path>,
        arguments_json: &str,
        _session_id: Option<&str>,
        _turn_id: Option<&str>,
        _config: Option<&autonoetic_types::config::GatewayConfig>,
    ) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            id: String,
        }
        let args: Args = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;

        anyhow::ensure!(!args.id.trim().is_empty(), "id must not be empty");

        let Some(gw_dir) = gateway_dir else {
            anyhow::bail!("Knowledge requires gateway directory to be configured");
        };

        let mem = crate::runtime::memory::Tier2Memory::new(gw_dir, &manifest.agent.id)?;
        let memory = mem.recall(&args.id)?;

        serde_json::to_string(&serde_json::json!({
            "ok": true,
            "id": memory.memory_id,
            "content": memory.content,
            "scope": memory.scope,
            "writer": memory.writer_agent_id,
            "created_at": memory.created_at,
            "confidence": memory.confidence,
        }))
        .map_err(Into::into)
    }
}

// ---------------------------------------------------------------------------
// Knowledge Search Tool (renamed from memory.search)
// ---------------------------------------------------------------------------

/// Searches the knowledge base by scope and optional query.
pub struct KnowledgeSearchTool;

impl NativeTool for KnowledgeSearchTool {
    fn name(&self) -> &'static str {
        "knowledge.search"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        // Search is included in ReadAccess capability
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::ReadAccess { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Search the knowledge base by scope and optional query. Returns all knowledge in the scope that you have access to, optionally filtered by content matching the query.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "scope": { "type": "string", "description": "The scope/namespace to search in (e.g., 'api-keys', 'user-preferences')" },
                    "query": { "type": "string", "description": "Optional search term to filter by content" }
                },
                "required": ["scope"],
                "additionalProperties": false
            }),
        }
    }

    fn execute(
        &self,
        manifest: &AgentManifest,
        _policy: &PolicyEngine,
        _agent_dir: &Path,
        gateway_dir: Option<&Path>,
        arguments_json: &str,
        _session_id: Option<&str>,
        _turn_id: Option<&str>,
        _config: Option<&autonoetic_types::config::GatewayConfig>,
    ) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            scope: String,
            query: Option<String>,
        }
        let args: Args = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;

        anyhow::ensure!(!args.scope.trim().is_empty(), "scope must not be empty");

        let Some(gw_dir) = gateway_dir else {
            anyhow::bail!("Knowledge requires gateway directory to be configured");
        };

        let mem = crate::runtime::memory::Tier2Memory::new(gw_dir, &manifest.agent.id)?;
        let results = mem.search(&args.scope, args.query.as_deref())?;

        let items: Vec<serde_json::Value> = results
            .iter()
            .map(|m| {
                serde_json::json!({
                    "id": m.memory_id,
                    "content": m.content,
                    "writer": m.writer_agent_id,
                    "created_at": m.created_at,
                    "confidence": m.confidence,
                })
            })
            .collect();

        serde_json::to_string(&serde_json::json!({
            "ok": true,
            "scope": args.scope,
            "results": items,
            "count": items.len(),
        }))
        .map_err(Into::into)
    }
}

// ---------------------------------------------------------------------------
// Knowledge Share Tool (renamed from memory.share)
// ---------------------------------------------------------------------------

/// Shares knowledge with specific agents.
pub struct KnowledgeShareTool;

impl NativeTool for KnowledgeShareTool {
    fn name(&self) -> &'static str {
        "knowledge.share"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        // Sharing is included in WriteAccess capability
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::WriteAccess { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Share knowledge with specific agents. Requires ownership or write access to the knowledge. Once shared, the target agents can recall and search for this knowledge.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "The knowledge ID to share" },
                    "with_agents": { "type": "array", "items": { "type": "string" }, "description": "List of agent IDs to share with" }
                },
                "required": ["id", "with_agents"],
                "additionalProperties": false
            }),
        }
    }

    fn execute(
        &self,
        manifest: &AgentManifest,
        policy: &PolicyEngine,
        _agent_dir: &Path,
        gateway_dir: Option<&Path>,
        arguments_json: &str,
        _session_id: Option<&str>,
        _turn_id: Option<&str>,
        _config: Option<&autonoetic_types::config::GatewayConfig>,
    ) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            id: String,
            with_agents: Vec<String>,
        }
        let args: Args = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;

        anyhow::ensure!(!args.id.trim().is_empty(), "id must not be empty");
        anyhow::ensure!(
            !args.with_agents.is_empty(),
            "with_agents must not be empty"
        );

        // Check if agent is allowed to share with these targets
        for target in &args.with_agents {
            anyhow::ensure!(
                policy.can_share_memory(target),
                "Cannot share knowledge with agent '{}': not in allowed_targets",
                target
            );
        }

        let Some(gw_dir) = gateway_dir else {
            anyhow::bail!("Knowledge requires gateway directory to be configured");
        };

        let mem = crate::runtime::memory::Tier2Memory::new(gw_dir, &manifest.agent.id)?;
        let memory = mem.share_with(&args.id, args.with_agents.clone())?;

        serde_json::to_string(&serde_json::json!({
            "ok": true,
            "id": memory.memory_id,
            "visibility": "shared",
            "allowed_agents": memory.allowed_agents,
        }))
        .map_err(Into::into)
    }
}

// ---------------------------------------------------------------------------
// Skill Draft Tool
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Agent Install Tool
// ---------------------------------------------------------------------------

/// A file to be installed as part of an agent.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct InstallAgentFile {
    pub path: String,
    pub content: String,
}

fn collect_paths_with_prefix(files: &[InstallAgentFile], prefix: &str) -> Vec<String> {
    files
        .iter()
        .filter_map(|file| file.path.strip_prefix(prefix).map(|_| file.path.clone()))
        .collect()
}

fn ensure_output_contract_section(
    instructions: &str,
    files: &[InstallAgentFile],
    requires_contract: bool,
) -> String {
    if !requires_contract {
        return instructions.trim().to_string();
    }

    let trimmed = instructions.trim();
    if trimmed.to_ascii_lowercase().contains("## output contract") {
        return trimmed.to_string();
    }

    let state_files = collect_paths_with_prefix(files, "state/");
    let history_files = collect_paths_with_prefix(files, "history/");

    let mut section = String::new();
    section.push_str("\n\n## Output Contract\n\n");
    section.push_str("memory_keys:\n");
    section.push_str("- \"\"\n");
    section.push_str("state_files:\n");
    if state_files.is_empty() {
        section.push_str("- \"state/\"\n");
    } else {
        for path in state_files {
            section.push_str(&format!("- \"{}\"\n", path));
        }
    }
    section.push_str("history_files:\n");
    if history_files.is_empty() {
        section.push_str("- \"history/\"\n");
    } else {
        for path in history_files {
            section.push_str(&format!("- \"{}\"\n", path));
        }
    }
    section.push_str("return_schema:\n");
    section.push_str("  type: \"object\"\n");
    section.push_str("  properties:\n");
    section.push_str("    status:\n");
    section.push_str("      type: \"string\"\n");
    section.push_str("long_term_memory_mode: \"sdk_preferred_with_file_fallback\"\n");

    format!("{}{}", trimmed, section)
}

#[derive(Debug, Deserialize, Serialize)]
struct InstallAgentArgs {
    agent_id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    instructions: String,
    #[serde(default)]
    llm_config: Option<LlmConfig>,
    #[serde(default)]
    capabilities: Vec<Capability>,
    #[serde(default)]
    background: Option<BackgroundPolicy>,
    #[serde(default)]
    scheduled_action: Option<serde_json::Value>,
    #[serde(default)]
    files: Vec<InstallAgentFile>,
    #[serde(default)]
    runtime_lock_dependencies: Vec<LockedDependencySet>,
    #[serde(default)]
    promotion_gate: Option<InstallPromotionGate>,
    #[serde(default = "default_true")]
    arm_immediately: bool,
    #[serde(default = "default_true")]
    validate_on_install: bool,
    /// Execution mode: "script" for deterministic script-only agents, "reasoning" for LLM-driven (default)
    #[serde(default)]
    execution_mode: Option<ExecutionMode>,
    /// Entry script path for script mode (e.g., "scripts/main.py")
    #[serde(default)]
    script_entry: Option<String>,
    /// Remote gateway URL for distributed agents (optional)
    #[serde(default)]
    gateway_url: Option<String>,
    /// Authentication token for remote gateway (optional)
    #[serde(default)]
    gateway_token: Option<String>,
}

/// Promotion gate evidence for agent.install from evolution roles.
///
/// The `security_analysis` and `capability_analysis` fields are optional but
/// recommended. When present, they provide actual evidence from automated analysis.
/// When absent, only the boolean flags are checked (legacy behavior).
#[derive(Debug, Deserialize, Serialize)]
pub struct InstallPromotionGate {
    /// Evaluator passed (automated or human)
    pub evaluator_pass: bool,
    /// Auditor passed (automated or human)
    pub auditor_pass: bool,
    /// Override approval reference (for exceptional cases)
    #[serde(default)]
    pub override_approval_ref: Option<String>,
    /// Human approval reference (after operator approval)
    #[serde(default)]
    pub install_approval_ref: Option<String>,
    /// Security analysis results (recommended: actual evidence)
    #[serde(default)]
    pub security_analysis: Option<SecurityAnalysisEvidence>,
    /// Capability analysis results (recommended: actual evidence)
    #[serde(default)]
    pub capability_analysis: Option<CapabilityAnalysisEvidence>,
}

/// Security analysis evidence from automated code scanning.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SecurityAnalysisEvidence {
    pub passed: bool,
    pub threats_detected: Vec<String>,
    pub remote_access_detected: bool,
    #[serde(default)]
    pub analyzer_version: Option<String>,
}

/// Capability analysis evidence from automated inference.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CapabilityAnalysisEvidence {
    pub inferred_capabilities: Vec<String>,
    pub missing_capabilities: Vec<String>,
    pub declared_capabilities: Vec<String>,
    pub analysis_passed: bool,
}

fn normalize_string_set(values: &[String]) -> BTreeSet<String> {
    values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .collect()
}

fn format_string_set(values: &BTreeSet<String>) -> String {
    if values.is_empty() {
        "[]".to_string()
    } else {
        format!(
            "[{}]",
            values
                .iter()
                .map(|value| format!("\"{}\"", value))
                .collect::<Vec<String>>()
                .join(", ")
        )
    }
}

fn validate_promotion_gate_evidence(
    gate: &InstallPromotionGate,
    args: &InstallAgentArgs,
    capability_analysis: &crate::runtime::analysis::CapabilityAnalysis,
    security_analysis: &crate::runtime::analysis::SecurityAnalysis,
) -> anyhow::Result<()> {
    let security = gate.security_analysis.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "promotion gate evidence missing: provide promotion_gate.security_analysis with concrete analysis results"
        )
    })?;
    let capability = gate.capability_analysis.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "promotion gate evidence missing: provide promotion_gate.capability_analysis with concrete analysis results"
        )
    })?;

    anyhow::ensure!(
        security.passed,
        "promotion_gate.security_analysis.passed must be true for install"
    );
    anyhow::ensure!(
        capability.analysis_passed,
        "promotion_gate.capability_analysis.analysis_passed must be true for install"
    );
    anyhow::ensure!(
        capability.missing_capabilities.is_empty(),
        "promotion_gate.capability_analysis.missing_capabilities must be empty for install"
    );

    anyhow::ensure!(
        security_analysis.passed,
        "gateway security analysis failed; install is blocked"
    );
    anyhow::ensure!(
        !security_analysis.remote_access_detected || security.remote_access_detected,
        "promotion_gate.security_analysis.remote_access_detected must be true when gateway analysis detects remote access"
    );

    let expected_inferred = normalize_string_set(&capability_analysis.inferred_types);
    let provided_inferred = normalize_string_set(&capability.inferred_capabilities);
    anyhow::ensure!(
        expected_inferred.is_subset(&provided_inferred),
        "promotion_gate.capability_analysis.inferred_capabilities must include all gateway-inferred capabilities (expected subset {}, got {})",
        format_string_set(&expected_inferred),
        format_string_set(&provided_inferred)
    );

    let expected_missing = normalize_string_set(&capability_analysis.missing);
    let provided_missing = normalize_string_set(&capability.missing_capabilities);
    anyhow::ensure!(
        provided_missing == expected_missing,
        "promotion_gate.capability_analysis.missing_capabilities mismatch: expected {}, got {}",
        format_string_set(&expected_missing),
        format_string_set(&provided_missing)
    );

    let expected_declared = normalize_string_set(
        &args
            .capabilities
            .iter()
            .map(capability_type_name)
            .collect::<Vec<String>>(),
    );
    let provided_declared = normalize_string_set(&capability.declared_capabilities);
    anyhow::ensure!(
        provided_declared == expected_declared,
        "promotion_gate.capability_analysis.declared_capabilities mismatch: expected {}, got {}",
        format_string_set(&expected_declared),
        format_string_set(&provided_declared)
    );

    Ok(())
}

fn parse_install_scheduled_action(
    value: Option<serde_json::Value>,
) -> anyhow::Result<Option<ScheduledAction>> {
    let Some(value) = value else {
        return Ok(None);
    };

    if let Ok(action) = serde_json::from_value::<ScheduledAction>(value.clone()) {
        return Ok(Some(action));
    }

    let object = value.as_object().ok_or_else(|| {
        anyhow::anyhow!(
            "scheduled_action must be an object describing either a sandbox command or a file write"
        )
    })?;

    let requires_approval = object
        .get("requires_approval")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let evidence_ref = object
        .get("evidence_ref")
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned);

    if let Some(tool_use) = object.get("tool_use").and_then(|v| v.as_object()) {
        let tool_name = tool_use.get("name").and_then(|v| v.as_str());
        let arguments = tool_use
            .get("arguments")
            .and_then(|v| v.as_object())
            .ok_or_else(|| {
                anyhow::anyhow!("scheduled_action.tool_use.arguments must be an object")
            })?;

        if matches!(
            tool_name,
            Some("sandbox.exec") | Some("sandbox_exec") | None
        ) {
            if let Some(command) = arguments
                .get("command")
                .or_else(|| arguments.get("cmd"))
                .or_else(|| arguments.get("script"))
                .and_then(|v| v.as_str())
            {
                return Ok(Some(ScheduledAction::SandboxExec {
                    command: command.to_string(),
                    dependencies: None,
                    requires_approval,
                    evidence_ref,
                }));
            }
        }

        if matches!(
            tool_name,
            Some("memory.write") | Some("memory_write") | None
        ) {
            let path = arguments.get("path").and_then(|v| v.as_str());
            let content = arguments.get("content").and_then(|v| v.as_str());
            if let (Some(path), Some(content)) = (path, content) {
                return Ok(Some(ScheduledAction::WriteFile {
                    path: path.to_string(),
                    content: content.to_string(),
                    requires_approval,
                    evidence_ref,
                }));
            }
        }

        anyhow::bail!(
            "scheduled_action.tool_use must describe either sandbox.exec with command/cmd/script or memory.write with path/content"
        );
    }

    if let Some(command) = object.get("command").and_then(|v| v.as_str()) {
        let dependencies = object
            .get("dependencies")
            .cloned()
            .map(
                serde_json::from_value::<autonoetic_types::background::ScheduledActionDependencies>,
            )
            .transpose()
            .map_err(|e| anyhow::anyhow!("Invalid scheduled_action.dependencies: {}", e))?;
        return Ok(Some(ScheduledAction::SandboxExec {
            command: command.to_string(),
            dependencies,
            requires_approval,
            evidence_ref,
        }));
    }

    if let Some(script) = object.get("script").and_then(|v| v.as_str()) {
        return Ok(Some(ScheduledAction::SandboxExec {
            command: script.to_string(),
            dependencies: None,
            requires_approval,
            evidence_ref,
        }));
    }

    let path = object.get("path").and_then(|v| v.as_str());
    let content = object.get("content").and_then(|v| v.as_str());
    if let (Some(path), Some(content)) = (path, content) {
        return Ok(Some(ScheduledAction::WriteFile {
            path: path.to_string(),
            content: content.to_string(),
            requires_approval,
            evidence_ref,
        }));
    }

    anyhow::bail!(
        "scheduled_action must include either a tagged 'type', a 'command' or 'script' field, a supported 'tool_use' wrapper, or both 'path' and 'content' fields"
    )
}

fn parse_interval_hint_value(value: &serde_json::Value) -> Option<u64> {
    if let Some(interval_secs) = value.as_u64().filter(|secs| *secs > 0) {
        return Some(interval_secs);
    }

    let text = value.as_str()?.trim();
    if text.is_empty() {
        return None;
    }

    if let Ok(secs) = text.parse::<u64>() {
        return (secs > 0).then_some(secs);
    }

    let lowered = text.to_ascii_lowercase();
    let (digits, multiplier) = if let Some(raw) = lowered.strip_suffix("seconds") {
        (raw, 1_u64)
    } else if let Some(raw) = lowered.strip_suffix("second") {
        (raw, 1_u64)
    } else if let Some(raw) = lowered.strip_suffix("secs") {
        (raw, 1_u64)
    } else if let Some(raw) = lowered.strip_suffix("sec") {
        (raw, 1_u64)
    } else if let Some(raw) = lowered.strip_suffix('s') {
        (raw, 1_u64)
    } else if let Some(raw) = lowered.strip_suffix("minutes") {
        (raw, 60_u64)
    } else if let Some(raw) = lowered.strip_suffix("minute") {
        (raw, 60_u64)
    } else if let Some(raw) = lowered.strip_suffix("mins") {
        (raw, 60_u64)
    } else if let Some(raw) = lowered.strip_suffix("min") {
        (raw, 60_u64)
    } else if let Some(raw) = lowered.strip_suffix('m') {
        (raw, 60_u64)
    } else if let Some(raw) = lowered.strip_suffix("hours") {
        (raw, 3600_u64)
    } else if let Some(raw) = lowered.strip_suffix("hour") {
        (raw, 3600_u64)
    } else if let Some(raw) = lowered.strip_suffix("hrs") {
        (raw, 3600_u64)
    } else if let Some(raw) = lowered.strip_suffix("hr") {
        (raw, 3600_u64)
    } else if let Some(raw) = lowered.strip_suffix('h') {
        (raw, 3600_u64)
    } else {
        return None;
    };

    digits
        .trim()
        .parse::<u64>()
        .ok()
        .filter(|secs| *secs > 0)
        .map(|secs| secs * multiplier)
}

fn scheduled_action_interval_hint(value: &Option<serde_json::Value>) -> Option<u64> {
    value.as_ref().and_then(|raw| {
        raw.get("interval_secs")
            .and_then(parse_interval_hint_value)
            .or_else(|| raw.get("cadence").and_then(parse_interval_hint_value))
    })
}

fn normalize_install_background(
    background: Option<BackgroundPolicy>,
    scheduled_action_raw: &Option<serde_json::Value>,
) -> anyhow::Result<Option<BackgroundPolicy>> {
    let interval_hint = scheduled_action_interval_hint(scheduled_action_raw);
    match (background, interval_hint) {
        (Some(mut background), Some(interval_secs)) => {
            background.enabled = true;
            if background.interval_secs == 0 {
                background.interval_secs = interval_secs;
            }
            Ok(Some(background))
        }
        (Some(background), None) => Ok(Some(background)),
        (None, Some(interval_secs)) => Ok(Some(BackgroundPolicy {
            enabled: true,
            interval_secs,
            mode: BackgroundMode::Deterministic,
            wake_predicates: Default::default(),
            validate_on_install: true,
        })),
        (None, None) => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Session Snapshot Tool
// ---------------------------------------------------------------------------

/// Captures a snapshot of the current session's conversation history and stores
/// it in the content-addressable storage. Returns a content handle for later
/// retrieval or forking.
pub struct SessionSnapshotTool;

impl NativeTool for SessionSnapshotTool {
    fn name(&self) -> &'static str {
        "session.snapshot"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        // Available to any agent with WriteAccess capability
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::WriteAccess { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Capture a snapshot of the current session's conversation history. Returns a content handle that can be used to restore or fork this session later. The snapshot is persisted automatically.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Optional name for this snapshot (e.g., 'checkpoint-1')"
                    },
                    "persist": {
                        "type": "boolean",
                        "description": "Whether to persist the snapshot permanently (default: true)",
                        "default": true
                    }
                },
                "additionalProperties": false
            }),
        }
    }

    fn execute(
        &self,
        manifest: &AgentManifest,
        _policy: &PolicyEngine,
        agent_dir: &Path,
        gateway_dir: Option<&Path>,
        arguments_json: &str,
        session_id: Option<&str>,
        _turn_id: Option<&str>,
        _config: Option<&autonoetic_types::config::GatewayConfig>,
    ) -> anyhow::Result<String> {
        #[derive(Deserialize, Default)]
        struct Args {
            #[serde(default)]
            name: Option<String>,
            #[serde(default = "default_true")]
            persist: bool,
        }
        fn default_true() -> bool {
            true
        }

        let args: Args = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;

        let Some(gw_dir) = gateway_dir else {
            anyhow::bail!("session.snapshot requires gateway directory");
        };

        let sid = session_id.unwrap_or(&manifest.agent.id);

        // Load current history from session history file
        let history_path = agent_dir.join("history").join("causal_chain.jsonl");
        let history = if history_path.exists() {
            // Load from causal chain or session history
            load_history_from_session(agent_dir, sid)?
        } else {
            vec![]
        };

        // Count turns (approximate from messages)
        let turn_count = history.len() / 2; // Rough estimate: user+assistant pairs

        // Create snapshot
        let snapshot = crate::runtime::session_snapshot::SessionSnapshot::capture(
            sid, &history, turn_count,
            None, // session_context - TODO: load from session_context.rs
            None, // sdk_checkpoint
            gw_dir,
        )?;

        // Persist if requested
        if args.persist {
            snapshot.persist(sid, gw_dir)?;
        }

        // Register with custom name if provided
        if let Some(name) = &args.name {
            let store = crate::runtime::content_store::ContentStore::new(gw_dir)?;
            if let Some(handle) = snapshot.handle() {
                let handle_string = handle.to_string();
                store.register_name(sid, &format!("snapshot:{}", name), &handle_string)?;
            }
        }

        serde_json::to_string(&serde_json::json!({
            "ok": true,
            "handle": snapshot.handle(),
            "source_session_id": snapshot.source_session_id,
            "turn_count": snapshot.turn_count,
            "created_at": snapshot.created_at,
            "persisted": args.persist,
        }))
        .map_err(Into::into)
    }
}

/// Loads conversation history from a session directory.
fn load_history_from_session(
    agent_dir: &Path,
    session_id: &str,
) -> anyhow::Result<Vec<crate::llm::Message>> {
    // Try to load from session history file
    let history_file = agent_dir
        .join("history")
        .join(format!("{}.jsonl", session_id));
    if !history_file.exists() {
        return Ok(vec![]);
    }

    let content = std::fs::read_to_string(history_file)?;
    let mut messages = Vec::new();

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) {
            // Try to extract message from causal chain entry
            if let Some(action) = entry.get("action").and_then(|a| a.as_str()) {
                if action.contains("assistant") {
                    if let Some(payload) = entry.get("payload") {
                        if let Some(content) = payload.get("content").and_then(|c| c.as_str()) {
                            messages.push(crate::llm::Message::assistant(content));
                        }
                    }
                }
            }
        }
    }

    Ok(messages)
}

#[derive(Debug, Deserialize)]
struct SpawnAgentArgs {
    agent_id: String,
    message: String,
    #[serde(default)]
    metadata: Option<serde_json::Value>,
    #[serde(default)]
    session_id: Option<String>,
}

pub struct AgentSpawnTool;

impl NativeTool for AgentSpawnTool {
    fn name(&self) -> &'static str {
        "agent.spawn"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::AgentSpawn { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Delegate the current task to an existing specialist agent and receive its reply in-session.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "message": { "type": "string" },
                    "metadata": { "type": "object" },
                    "session_id": { "type": "string" }
                },
                "required": ["agent_id", "message"],
                "additionalProperties": false
            }),
        }
    }

    fn execute(
        &self,
        manifest: &AgentManifest,
        _policy: &PolicyEngine,
        agent_dir: &Path,
        _gateway_dir: Option<&Path>,
        arguments_json: &str,
        session_id: Option<&str>,
        _turn_id: Option<&str>,
        config: Option<&autonoetic_types::config::GatewayConfig>,
    ) -> anyhow::Result<String> {
        let args: SpawnAgentArgs = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;
        validate_agent_id(&args.agent_id)?;
        anyhow::ensure!(!args.message.trim().is_empty(), "message must not be empty");

        // Schema enforcement hook
        let default_enforcement_config = SchemaEnforcementConfig::default();
        let enforcement_config = config
            .map(|c| &c.schema_enforcement)
            .unwrap_or(&default_enforcement_config);

        if enforcement_config.mode != SchemaEnforcementMode::Disabled {
            let agents_dir = agent_dir.parent().ok_or_else(|| {
                anyhow::anyhow!("Agent directory is missing its agents root parent")
            })?;
            let target_agent_path = agents_dir.join(&args.agent_id).join("SKILL.md");

            if target_agent_path.exists() {
                if let Ok(manifest_content) = std::fs::read_to_string(&target_agent_path) {
                    if let Some(frontmatter) = manifest_content.split("---").nth(1) {
                        if let Ok(target_manifest) =
                            serde_yaml::from_str::<AgentManifest>(frontmatter)
                        {
                            if let Some(io) = &target_manifest.io {
                                if let Some(accepts) = &io.accepts {
                                    let enforcer = default_enforcer();
                                    let payload = serde_json::json!({
                                        "message": args.message,
                                        "metadata": args.metadata,
                                        "session_id": args.session_id,
                                    });

                                    match enforcer.enforce(&payload, accepts) {
                                        EnforcementResult::Reject(details) => {
                                            return Err(anyhow::anyhow!(
                                                "Schema validation failed: {}. Hint: {}",
                                                details.reason,
                                                details.hint.unwrap_or_default()
                                            ));
                                        }
                                        EnforcementResult::Coerced(details) => {
                                            if enforcement_config.audit {
                                                tracing::info!(
                                                    target: "schema_enforcement",
                                                    agent_id = %args.agent_id,
                                                    transformations = ?details.transformations,
                                                    "Schema enforcement: payload coerced"
                                                );
                                            }
                                        }
                                        EnforcementResult::Pass => {}
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        let resolved_session_id = args
            .session_id
            .clone()
            .or_else(|| session_id.map(ToOwned::to_owned))
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        let agents_dir = agent_dir
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Agent directory is missing its agents root parent"))?;
        let config = GatewayConfig {
            agents_dir: agents_dir.to_path_buf(),
            ..GatewayConfig::default()
        };
        let execution = crate::execution::GatewayExecutionService::new(config);

        let source_agent_id = manifest.agent.id.clone();
        let target_agent_id = args.agent_id.clone();
        let kickoff_message = match &args.metadata {
            Some(value) => format!("{}\n\nDelegation metadata: {}", args.message, value),
            None => args.message.clone(),
        };

        // Set up hierarchical content namespace for the child agent
        // The child gets a unique delegation path (e.g., "demo-session-1/coder-abc123")
        // so content written by the child is visible to the parent via the hierarchy
        let child_delegation_path = format!(
            "{}/{}-{}",
            resolved_session_id,
            args.agent_id,
            &uuid::Uuid::new_v4().to_string()[..8]
        );

        if let Ok(agents_dir) = agent_dir
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Agent directory missing parent"))
        {
            if let Ok(store) =
                crate::runtime::content_store::ContentStore::new(&agents_dir.join(".gateway"))
            {
                // Set parent relationship so child's content is visible to parent
                let _ = store.set_parent_session(&child_delegation_path, &resolved_session_id);
                tracing::info!(
                    target: "content_store",
                    parent_session = %resolved_session_id,
                    child_delegation = %child_delegation_path,
                    "Set up hierarchical content namespace for child agent"
                );
            }
        }

        let spawn_future = async move {
            execution
                .spawn_agent_once(
                    &target_agent_id,
                    &kickoff_message,
                    &child_delegation_path, // Use delegation path as session_id for content namespace
                    Some(&source_agent_id),
                    false,
                    None,
                    args.metadata.as_ref(),
                )
                .await
        };

        let result = if let Ok(handle) = tokio::runtime::Handle::try_current() {
            tokio::task::block_in_place(|| handle.block_on(spawn_future))?
        } else {
            tokio::runtime::Runtime::new()?.block_on(spawn_future)?
        };

        Ok(serde_json::json!({
            "ok": true,
            "status": "agent_spawned",
            "agent_id": result.agent_id,
            "session_id": result.session_id,
            "assistant_reply": result.assistant_reply,
            "artifacts": result.artifacts,
            // All named content written by the child — use name/handle/alias with content.read
            "files": result.files,
            "shared_knowledge": result.shared_knowledge,
        })
        .to_string())
    }
}

/// Provides helpful error context for capability-related deserialization errors.
fn capability_error_context(serde_error: &serde_json::Error) -> String {
    let err_str = serde_error.to_string();

    // Check for common capability format mistakes
    if err_str.contains("hosts") {
        return format!(
            "{}\n\nHELP: NetworkAccess capability requires 'hosts' field.\n\
            Correct format: {{\"type\": \"NetworkAccess\", \"hosts\": [\"api.example.com\"]}}\n\
            Use [\"*\"] to allow all hosts.",
            err_str
        );
    }

    if err_str.contains("allowed") {
        return format!(
            "{}\n\nHELP: SandboxFunctions capability requires 'allowed' field.\n\
            Correct format: {{\"type\": \"SandboxFunctions\", \"allowed\": [\"web.\", \"content.\"]}}",
            err_str
        );
    }

    if err_str.contains("scopes") {
        return format!(
            "{}\n\nHELP: ReadAccess or WriteAccess requires 'scopes' field.\n\
            Correct format: {{\"type\": \"ReadAccess\", \"scopes\": [\"self.*\"]}}",
            err_str
        );
    }

    if err_str.contains("max_children") {
        return format!(
            "{}\n\nHELP: AgentSpawn capability requires 'max_children' field.\n\
            Correct format: {{\"type\": \"AgentSpawn\", \"max_children\": 3}}",
            err_str
        );
    }

    if err_str.contains("unknown field") {
        return format!(
            "{}\n\nHELP: Unexpected field detected. Capability types only accept specific fields.\n\
            - NetworkAccess: type, hosts\n\
            - SandboxFunctions: type, allowed\n\
            - ReadAccess/WriteAccess: type, scopes\n\
            - AgentSpawn: type, max_children\n\
            - CodeExecution: type, patterns\n\
            Remove extra fields like 'description' or 'runtime'.",
            err_str
        );
    }

    err_str
}

pub struct AgentInstallTool;

impl NativeTool for AgentInstallTool {
    fn name(&self) -> &'static str {
        "agent.install"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        // Only evolution roles can install agents - prevents planner from bypassing specialized_builder
        is_evolution_role(&manifest.agent.id)
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Install a specialized child agent. Writes the agent's SKILL.md and files to disk. Only available to evolution roles (specialized_builder, evolution-steward).".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent_id": { 
                        "type": "string",
                        "description": "Unique identifier for the agent (e.g., 'weather-fetcher'). Use lowercase with hyphens."
                    },
                    "name": { 
                        "type": "string",
                        "description": "Display name for the agent."
                    },
                    "description": { 
                        "type": "string",
                        "description": "What this agent does."
                    },
                    "instructions": { 
                        "type": "string",
                        "description": "The agent's SKILL.md content with instructions."
                    },
                    "capabilities": {
                        "type": "array",
                        "description": "List of capabilities this agent needs.",
                        "items": {
                            "type": "object",
                            "description": "Capability object with 'type' and type-specific fields.",
                            "oneOf": [
                                {
                                    "type": "object",
                                    "properties": {
                                        "type": { "const": "NetworkAccess" },
                                        "hosts": { "type": "array", "items": { "type": "string" }, "description": "Allowed hosts. Use ['*'] for all." }
                                    },
                                    "required": ["type", "hosts"]
                                },
                                {
                                    "type": "object",
                                    "properties": {
                                        "type": { "const": "SandboxFunctions" },
                                        "allowed": { "type": "array", "items": { "type": "string" } }
                                    },
                                    "required": ["type", "allowed"]
                                },
                                {
                                    "type": "object",
                                    "properties": {
                                        "type": { "const": "ReadAccess" },
                                        "scopes": { "type": "array", "items": { "type": "string" } }
                                    },
                                    "required": ["type", "scopes"]
                                },
                                {
                                    "type": "object",
                                    "properties": {
                                        "type": { "const": "WriteAccess" },
                                        "scopes": { "type": "array", "items": { "type": "string" } }
                                    },
                                    "required": ["type", "scopes"]
                                },
                                {
                                    "type": "object",
                                    "properties": {
                                        "type": { "const": "AgentSpawn" },
                                        "max_children": { "type": "integer" }
                                    },
                                    "required": ["type", "max_children"]
                                },
                                {
                                    "type": "object",
                                    "properties": {
                                        "type": { "const": "CodeExecution" },
                                        "patterns": { "type": "array", "items": { "type": "string" } }
                                    },
                                    "required": ["type", "patterns"]
                                }
                            ]
                        }
                    },
                    "files": {
                        "type": "array",
                        "description": "Files to write to the agent's directory.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "path": {
                                    "type": "string",
                                    "description": "Relative path (e.g., 'scripts/main.py', 'SKILL.md')."
                                },
                                "content": {
                                    "type": "string",
                                    "description": "The file content."
                                }
                            },
                            "required": ["path", "content"]
                        }
                    },
                    "promotion_gate": {
                        "type": "object",
                        "description": "Required for evolution roles. Booleans alone are insufficient: provide concrete security_analysis and capability_analysis evidence.",
                        "properties": {
                            "evaluator_pass": { "type": "boolean" },
                            "auditor_pass": { "type": "boolean" },
                            "override_approval_ref": {
                                "type": "string",
                                "description": "Optional exceptional override reference."
                            },
                            "install_approval_ref": {
                                "type": "string",
                                "description": "Set when retrying after human approval."
                            },
                            "security_analysis": {
                                "type": "object",
                                "properties": {
                                    "passed": { "type": "boolean" },
                                    "threats_detected": { "type": "array", "items": { "type": "string" } },
                                    "remote_access_detected": { "type": "boolean" },
                                    "analyzer_version": { "type": "string" }
                                },
                                "required": ["passed", "threats_detected", "remote_access_detected"]
                            },
                            "capability_analysis": {
                                "type": "object",
                                "properties": {
                                    "inferred_capabilities": { "type": "array", "items": { "type": "string" } },
                                    "missing_capabilities": { "type": "array", "items": { "type": "string" } },
                                    "declared_capabilities": { "type": "array", "items": { "type": "string" } },
                                    "analysis_passed": { "type": "boolean" }
                                },
                                "required": [
                                    "inferred_capabilities",
                                    "missing_capabilities",
                                    "declared_capabilities",
                                    "analysis_passed"
                                ]
                            }
                        },
                        "required": ["evaluator_pass", "auditor_pass"]
                    },
                    "execution_mode": {
                        "type": "string",
                        "enum": ["script", "reasoning"],
                        "description": "Script mode runs code without LLM. Reasoning mode uses LLM (default)."
                    },
                    "script_entry": {
                        "type": "string",
                        "description": "Entry script path when execution_mode is 'script' (e.g., 'scripts/main.py')."
                    }
                },
                "required": ["agent_id", "instructions"],
                "additionalProperties": false
            }),
        }
    }

    fn execute(
        &self,
        manifest: &AgentManifest,
        _policy: &PolicyEngine,
        agent_dir: &Path,
        _gateway_dir: Option<&Path>,
        arguments_json: &str,
        session_id: Option<&str>, // used when creating approval request
        _turn_id: Option<&str>,
        config: Option<&autonoetic_types::config::GatewayConfig>,
    ) -> anyhow::Result<String> {
        let mut args: InstallAgentArgs = serde_json::from_str(arguments_json).map_err(|e| {
            let context = capability_error_context(&e);
            anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), context)
        })?;
        let mut scheduled_action = parse_install_scheduled_action(args.scheduled_action.clone())?;
        let mut background =
            normalize_install_background(args.background.clone(), &args.scheduled_action)?;

        validate_agent_id(&args.agent_id)?;
        anyhow::ensure!(
            !args.instructions.trim().is_empty(),
            "instructions must not be empty"
        );

        // ─────────────────────────────────────────────────────────────────
        // Pluggable Code Analysis: Analyze code for capabilities and security
        // Provider is selected via GatewayConfig.code_analysis
        // ─────────────────────────────────────────────────────────────────
        use crate::runtime::analysis::{AnalysisProvider, AnalysisProviderFactory, FileToAnalyze};

        // Build files for analysis
        let mut files_for_analysis: Vec<FileToAnalyze> = args
            .files
            .iter()
            .map(|f| FileToAnalyze {
                path: f.path.clone(),
                content: f.content.clone(),
            })
            .collect();

        // Also analyze instructions as a potential source of capability hints
        files_for_analysis.push(FileToAnalyze {
            path: "SKILL.md".to_string(),
            content: args.instructions.clone(),
        });

        // Get the capability analysis provider from config (or default to pattern)
        let provider_type = config
            .and_then(|c| {
                serde_json::from_str::<autonoetic_types::config::CodeAnalysisConfig>(
                    &serde_json::to_string(&c.code_analysis).unwrap_or_default(),
                )
                .ok()
            })
            .map(|c| match c.capability_provider.as_str() {
                "llm" => crate::runtime::analysis::AnalysisProviderType::Llm,
                "composite" => crate::runtime::analysis::AnalysisProviderType::Composite,
                "none" => crate::runtime::analysis::AnalysisProviderType::None,
                _ => crate::runtime::analysis::AnalysisProviderType::Pattern,
            })
            .unwrap_or(crate::runtime::analysis::AnalysisProviderType::Pattern);

        let analyzer = AnalysisProviderFactory::create_capability_provider(&provider_type);
        tracing::info!(
            target: "agent.install",
            provider = analyzer.name(),
            "Running capability analysis"
        );

        let capability_analysis = analyzer.analyze_capabilities(&files_for_analysis);

        // Calculate missing and excessive capabilities
        let missing: Vec<String> = capability_analysis
            .inferred_types
            .iter()
            .filter(|inferred_type| {
                !args.capabilities.iter().any(|cap| {
                    let cap_type = match cap {
                        autonoetic_types::capability::Capability::NetworkAccess { .. } => {
                            "NetworkAccess"
                        }
                        autonoetic_types::capability::Capability::ReadAccess { .. } => "ReadAccess",
                        autonoetic_types::capability::Capability::WriteAccess { .. } => {
                            "WriteAccess"
                        }
                        autonoetic_types::capability::Capability::CodeExecution { .. } => {
                            "CodeExecution"
                        }
                        autonoetic_types::capability::Capability::AgentSpawn { .. } => "AgentSpawn",
                        autonoetic_types::capability::Capability::AgentMessage { .. } => {
                            "AgentMessage"
                        }
                        autonoetic_types::capability::Capability::SandboxFunctions { .. } => {
                            "SandboxFunctions"
                        }
                        autonoetic_types::capability::Capability::BackgroundReevaluation {
                            ..
                        } => "BackgroundReevaluation",
                    };
                    cap_type == inferred_type.as_str()
                })
            })
            .cloned()
            .collect();

        // Check if capabilities are required by config
        let require_caps = config
            .map(|c| c.code_analysis.require_capabilities)
            .unwrap_or(true);

        if require_caps && !missing.is_empty() {
            let missing_str = missing.join(", ");
            return Err(tagged::Tagged::validation(anyhow::anyhow!(
                "Capability mismatch: code requires {} but {} not declared in capabilities. \
                 Add these capabilities to your install request. \
                 (Analyzer: {})",
                missing_str,
                if missing.len() == 1 {
                    "it was"
                } else {
                    "they were"
                },
                analyzer.name()
            ))
            .into());
        }

        // Also run security analysis
        let security_analyzer = AnalysisProviderFactory::create_security_provider(
            &config
                .map(|c| match c.code_analysis.security_provider.as_str() {
                    "llm" => crate::runtime::analysis::AnalysisProviderType::Llm,
                    "composite" => crate::runtime::analysis::AnalysisProviderType::Composite,
                    "none" => crate::runtime::analysis::AnalysisProviderType::None,
                    _ => crate::runtime::analysis::AnalysisProviderType::Pattern,
                })
                .unwrap_or(crate::runtime::analysis::AnalysisProviderType::Pattern),
        );

        let security_analysis = security_analyzer.analyze_security(&files_for_analysis);

        tracing::info!(
            target: "agent.install",
            security_provider = security_analyzer.name(),
            passed = security_analysis.passed,
            threats = security_analysis.threats.len(),
            remote_access = security_analysis.remote_access_detected,
            "Security analysis complete"
        );

        // ─────────────────────────────────────────────────────────────────
        // Promotion Gate validation (for evolution roles)
        // ─────────────────────────────────────────────────────────────────
        if requires_promotion_gate(&manifest.agent.id) {
            let gate = args.promotion_gate.as_ref().ok_or_else(|| {
                tagged::Tagged::validation(anyhow::anyhow!(
                    "agent.install from '{}' requires promotion_gate evidence",
                    manifest.agent.id
                ))
            })?;
            let has_override = gate
                .override_approval_ref
                .as_ref()
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false);
            if has_override {
                tracing::warn!(
                    target: "agent.install",
                    installer = %manifest.agent.id,
                    "promotion gate override_approval_ref provided; skipping strict evidence checks"
                );
            } else {
                anyhow::ensure!(
                    gate.evaluator_pass && gate.auditor_pass,
                    "promotion gate failed: set evaluator_pass=true and auditor_pass=true, or provide override_approval_ref"
                );
                if config.is_some() {
                    validate_promotion_gate_evidence(
                        gate,
                        &args,
                        &capability_analysis,
                        &security_analysis,
                    )
                    .map_err(|error| tagged::Tagged::validation(error))?;
                }
            }
        }

        // Human approval gate: when policy requires it, create pending request or validate install_approval_ref.
        if let Some(cfg) = config {
            let policy = cfg.agent_install_approval_policy;
            let high_risk = is_install_high_risk(&args, &scheduled_action, &background);
            let need_approval = matches!(policy, AgentInstallApprovalPolicy::Always)
                || (matches!(policy, AgentInstallApprovalPolicy::RiskBased) && high_risk);
            let install_fingerprint =
                install_request_fingerprint(&args, &scheduled_action, &background)?;

            if need_approval {
                let install_approval_ref = args
                    .promotion_gate
                    .as_ref()
                    .and_then(|g| g.install_approval_ref.as_ref())
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty());

                if let Some(request_id) = install_approval_ref {
                    // Validate that this request was approved and is for this install.
                    let approved_path = crate::scheduler::store::approved_approvals_dir(cfg)
                        .join(format!("{request_id}.json"));
                    if !approved_path.exists() {
                        return Err(tagged::Tagged::validation(anyhow::anyhow!(
                            "install_approval_ref '{}' not found in approved approvals; install must be approved first",
                            request_id
                        ))
                        .into());
                    }
                    let decision: autonoetic_types::background::ApprovalDecision =
                        crate::scheduler::store::read_json_file(&approved_path)?;
                    match &decision.action {
                        ScheduledAction::AgentInstall {
                            agent_id: approved_agent_id,
                            requested_by_agent_id,
                            install_fingerprint: _,
                            ..
                        } if approved_agent_id == &args.agent_id
                            && requested_by_agent_id == &manifest.agent.id =>
                        {
                            // Valid approval - try to load stored payload for deterministic retry
                            if let Some(mut stored_args) = load_install_payload(cfg, request_id)? {
                                tracing::info!(
                                    target: "agent.install",
                                    request_id = %request_id,
                                    "Using stored install payload for deterministic retry"
                                );
                                // Preserve the install_approval_ref from original args (needed for cleanup)
                                if let Some(ref original_gate) = args.promotion_gate {
                                    if let Some(ref approval_ref) =
                                        original_gate.install_approval_ref
                                    {
                                        if let Some(ref mut gate) = stored_args.promotion_gate {
                                            gate.install_approval_ref = Some(approval_ref.clone());
                                        }
                                    }
                                }
                                // Replace args with stored payload
                                args = stored_args;
                                // Re-parse scheduled_action and background from stored args
                                scheduled_action =
                                    parse_install_scheduled_action(args.scheduled_action.clone())?;
                                background = normalize_install_background(
                                    args.background.clone(),
                                    &args.scheduled_action,
                                )?;
                            }
                        }
                        ScheduledAction::AgentInstall {
                            agent_id: approved_agent_id,
                            ..
                        } if approved_agent_id != &args.agent_id => {
                            return Err(tagged::Tagged::validation(anyhow::anyhow!(
                                "install_approval_ref '{}' does not match: agent_id mismatch (approved: {}, requested: {})",
                                request_id,
                                approved_agent_id,
                                args.agent_id,
                            ))
                            .into());
                        }
                        ScheduledAction::AgentInstall {
                            requested_by_agent_id,
                            ..
                        } if requested_by_agent_id != &manifest.agent.id => {
                            return Err(tagged::Tagged::validation(anyhow::anyhow!(
                                "install_approval_ref '{}' does not match: requester mismatch (approved: {}, current: {})",
                                request_id,
                                requested_by_agent_id,
                                manifest.agent.id,
                            ))
                            .into());
                        }
                        _ => {
                            return Err(tagged::Tagged::validation(anyhow::anyhow!(
                                "install_approval_ref '{}' does not match this install request",
                                request_id,
                            ))
                            .into());
                        }
                    }
                    // Proceed with install.
                } else {
                    // Create pending approval request and return structured response.
                    // Use short 8-char ID for human-friendliness (avoids LLM truncation issues)
                    let request_id = format!("apr-{}", &uuid::Uuid::new_v4().to_string()[..8]);
                    let summary = args
                        .instructions
                        .lines()
                        .next()
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|| args.agent_id.clone());
                    let request = ApprovalRequest {
                        request_id: request_id.clone(),
                        agent_id: manifest.agent.id.clone(),
                        session_id: session_id.unwrap_or("").to_string(),
                        action: ScheduledAction::AgentInstall {
                            agent_id: args.agent_id.clone(),
                            summary,
                            requested_by_agent_id: manifest.agent.id.clone(),
                            install_fingerprint: install_fingerprint.clone(),
                        },
                        created_at: Utc::now().to_rfc3339(),
                        reason: Some("agent.install requires human approval".to_string()),
                        evidence_ref: None,
                    };
                    let pending_path = crate::scheduler::store::pending_approvals_dir(cfg)
                        .join(format!("{request_id}.json"));
                    std::fs::create_dir_all(pending_path.parent().unwrap())?;
                    crate::scheduler::store::write_json_file(&pending_path, &request)?;

                    // Store the exact install payload for deterministic retry.
                    // This ensures the caller can retry with install_approval_ref
                    // and the gateway will use the approved payload (avoiding fingerprint mismatch).
                    if let Err(e) = store_install_payload(cfg, &request_id, &args) {
                        tracing::warn!(
                            target: "agent.install",
                            request_id = %request_id,
                            error = %e,
                            "Failed to store install payload for retry (non-fatal)"
                        );
                    }

                    return Ok(serde_json::json!({
                        "ok": false,
                        "approval_required": true,
                        "request_id": request_id,
                        "message": format!("Install requires approval. To proceed: 1) Get the request approved by an operator, 2) Retry agent.install with the EXACT same payload PLUS add promotion_gate.install_approval_ref = '{}' to your JSON.", request_id),
                        "retry_instruction": format!("Add this to your promotion_gate: \"install_approval_ref\": \"{}\"", request_id)
                    })
                    .to_string());
                }
            }
        }

        let agents_dir = agent_dir
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Agent directory is missing its agents root parent"))?;
        let child_dir = agents_dir.join(&args.agent_id);
        anyhow::ensure!(
            !child_dir.exists(),
            "child agent '{}' already exists",
            args.agent_id
        );
        let install_tmp_dir = agents_dir.join(format!(
            ".installing-{}-{}",
            args.agent_id,
            uuid::Uuid::new_v4()
        ));
        anyhow::ensure!(
            !install_tmp_dir.exists(),
            "temporary install directory '{}' already exists",
            install_tmp_dir.display()
        );

        if scheduled_action.is_some() {
            anyhow::ensure!(
                background.as_ref().map(|bg| bg.enabled).unwrap_or(false),
                "scheduled_action requires background.enabled = true"
            );
        }

        let mut capabilities = args.capabilities.clone();
        if let Some(background) = &background {
            anyhow::ensure!(
                background.interval_secs > 0,
                "background.interval_secs must be > 0"
            );
            let allow_reasoning = matches!(background.mode, BackgroundMode::Reasoning);
            if !capabilities.iter().any(|cap| {
                matches!(
                    cap,
                    Capability::BackgroundReevaluation {
                        min_interval_secs: _,
                        allow_reasoning: existing_allow_reasoning
                    } if *existing_allow_reasoning == allow_reasoning
                )
            }) {
                capabilities.push(Capability::BackgroundReevaluation {
                    min_interval_secs: background.interval_secs,
                    allow_reasoning,
                });
            }
        }

        if let Some(action) = &scheduled_action {
            match action {
                ScheduledAction::SandboxExec { command, .. } => {
                    if !capabilities.iter().any(|cap| {
                        matches!(cap, Capability::CodeExecution { patterns } if patterns.iter().any(|pattern| pattern == command))
                    }) {
                        capabilities.push(Capability::CodeExecution {
                            patterns: vec![command.clone()],
                        });
                    }
                }
                ScheduledAction::WriteFile { path, .. } => {
                    if !capabilities.iter().any(|cap| {
                        matches!(cap, Capability::WriteAccess { scopes } if scopes.iter().any(|scope| path.starts_with(scope.trim_end_matches('*'))))
                    }) {
                        capabilities.push(Capability::WriteAccess {
                            scopes: vec![path.clone()],
                        });
                    }
                }
                ScheduledAction::AgentInstall { .. } => {}
            }
        }

        // Determine execution mode: use provided value or default to Reasoning
        let execution_mode = args.execution_mode.unwrap_or_default();

        // Validate script mode requirements
        if matches!(execution_mode, ExecutionMode::Script) {
            anyhow::ensure!(
                args.script_entry
                    .as_ref()
                    .map(|s| !s.trim().is_empty())
                    .unwrap_or(false),
                "script execution mode requires a non-empty script_entry path"
            );
        }

        // Resolve llm_config:
        // - For Reasoning mode: use provided or inherit from parent
        // - For Script mode: llm_config is optional (scripts don't need LLM)
        let llm_config = args
            .llm_config
            .clone()
            .or_else(|| manifest.llm_config.clone());

        // Only require llm_config for reasoning background agents
        if matches!(
            background.as_ref().map(|bg| &bg.mode),
            Some(BackgroundMode::Reasoning)
        ) {
            anyhow::ensure!(
                llm_config.is_some(),
                "reasoning background agents require llm_config or an inheritable parent llm_config"
            );
        }

        let child_manifest = AgentManifest {
            version: manifest.version.clone(),
            runtime: manifest.runtime.clone(),
            agent: AgentIdentity {
                id: args.agent_id.clone(),
                name: args.name.clone().unwrap_or_else(|| args.agent_id.clone()),
                description: args
                    .description
                    .clone()
                    .unwrap_or_else(|| format!("Specialized agent {}", args.agent_id)),
            },
            capabilities,
            llm_config,
            limits: None,
            background: background.clone(),
            disclosure: None,
            io: None,
            middleware: None,
            execution_mode,
            script_entry: args.script_entry.clone(),
            gateway_url: args.gateway_url.clone(),
            gateway_token: args.gateway_token.clone(),
        };

        let mut install_validation_error: Option<String> = None;
        let staged_result = (|| -> anyhow::Result<()> {
            std::fs::create_dir_all(install_tmp_dir.join("state"))?;
            std::fs::create_dir_all(install_tmp_dir.join("history"))?;
            std::fs::create_dir_all(install_tmp_dir.join("skills"))?;
            std::fs::create_dir_all(install_tmp_dir.join("scripts"))?;

            let instruction_body = ensure_output_contract_section(
                &args.instructions,
                &args.files,
                scheduled_action.is_some(),
            );
            let skill_yaml = render_skill_frontmatter(&child_manifest)?;
            let skill_body = format!("---\n{}---\n{}\n", skill_yaml, instruction_body);
            std::fs::write(install_tmp_dir.join("SKILL.md"), skill_body)?;

            let runtime_lock = RuntimeLock {
                gateway: LockedGateway {
                    artifact: "autonoetic-gateway".to_string(),
                    version: manifest.runtime.gateway_version.clone(),
                    sha256: "unmanaged".to_string(),
                    signature: None,
                },
                sdk: LockedSdk {
                    version: manifest.runtime.sdk_version.clone(),
                },
                sandbox: LockedSandbox {
                    backend: manifest.runtime.sandbox.clone(),
                },
                dependencies: args.runtime_lock_dependencies.clone(),
                artifacts: Vec::new(),
            };
            std::fs::write(
                install_tmp_dir.join(&child_manifest.runtime.runtime_lock),
                serde_yaml::to_string(&runtime_lock)?,
            )?;

            for file in &args.files {
                validate_relative_agent_path(&file.path)?;
                anyhow::ensure!(
                    file.path != "SKILL.md" && file.path != child_manifest.runtime.runtime_lock,
                    "files may not overwrite generated SKILL.md or runtime.lock"
                );
                let target = install_tmp_dir.join(&file.path);
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(target, &file.content)?;
            }

            if let Some(action) = scheduled_action.clone() {
                persist_reevaluation_state(&install_tmp_dir, |state| {
                    state.pending_scheduled_action = Some(action.clone());
                    state.last_outcome = Some("installed".to_string());
                    state.retry_not_before = None;
                })?;

                if args.validate_on_install {
                    let registry = default_registry();
                    match execute_scheduled_action(
                        &child_manifest,
                        &install_tmp_dir,
                        &action,
                        &registry,
                        config,
                    ) {
                        Ok(_) => {
                            persist_reevaluation_state(&install_tmp_dir, |state| {
                                state.last_outcome = Some("install_validation_success".to_string());
                            })?;
                        }
                        Err(e) => {
                            persist_reevaluation_state(&install_tmp_dir, |state| {
                                state.last_outcome =
                                    Some(format!("install_validation_failed:{}", e));
                            })?;
                            let tool_error: autonoetic_types::tool_error::ToolError = e.into();
                            if !tool_error.is_recoverable() {
                                return Err(anyhow::anyhow!(
                                    "Fatal install validation error in {}: {}",
                                    action.kind(),
                                    tool_error.message
                                ));
                            }
                            install_validation_error = Some(
                                serde_json::to_string(&tool_error).map_err(anyhow::Error::from)?,
                            );
                        }
                    }
                }
            }

            Ok(())
        })();

        if let Err(e) = staged_result {
            let _ = std::fs::remove_dir_all(&install_tmp_dir);
            return Err(e);
        }
        if let Some(error_json) = install_validation_error {
            let _ = std::fs::remove_dir_all(&install_tmp_dir);
            return Ok(error_json);
        }
        if let Err(e) = std::fs::rename(&install_tmp_dir, &child_dir) {
            let _ = std::fs::remove_dir_all(&install_tmp_dir);
            if child_dir.exists() {
                anyhow::bail!("child agent '{}' already exists", args.agent_id);
            }
            return Err(e.into());
        }

        if let Some(background) = &child_manifest.background {
            if background.enabled {
                let next_due_at = if args.arm_immediately {
                    Utc::now()
                } else {
                    Utc::now() + Duration::seconds(background.interval_secs.max(1) as i64)
                };
                let background_state = BackgroundState {
                    agent_id: child_manifest.agent.id.clone(),
                    session_id: format!("background::{}", child_manifest.agent.id),
                    next_due_at: Some(next_due_at.to_rfc3339()),
                    ..BackgroundState::default()
                };
                let background_state_path =
                    background_state_file_for_child(agent_dir, &child_manifest.agent.id)?;
                if let Some(parent) = background_state_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(
                    background_state_path,
                    serde_json::to_string_pretty(&background_state)?,
                )?;
            }
        }

        // Cleanup stored install payload after successful install
        if let Some(cfg) = config {
            if let Some(approval_ref) = args
                .promotion_gate
                .as_ref()
                .and_then(|g| g.install_approval_ref.as_ref())
            {
                cleanup_install_payload(cfg, approval_ref);
            }
        }

        serde_json::to_string(&serde_json::json!({
            "ok": true,
            "status": "agent_installed",
            "agent_id": child_manifest.agent.id,
            "background_enabled": child_manifest.background.as_ref().map(|bg| bg.enabled).unwrap_or(false),
            "scheduled_action_kind": scheduled_action.as_ref().map(|action| action.kind()),
            "arm_immediately": args.arm_immediately,
        }))
        .map_err(Into::into)
    }
}

#[derive(Debug, Deserialize)]
struct AgentExistsArgs {
    agent_id: String,
}

pub struct AgentExistsTool;

impl NativeTool for AgentExistsTool {
    fn name(&self) -> &'static str {
        "agent.exists"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::AgentSpawn { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Check if an agent with the given ID already exists in the repository. Use this before agent.install to avoid duplicate installation attempts.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string", "description": "The agent ID to check for existence" }
                },
                "required": ["agent_id"],
                "additionalProperties": false
            }),
        }
    }

    fn execute(
        &self,
        _manifest: &AgentManifest,
        _policy: &PolicyEngine,
        agent_dir: &Path,
        _gateway_dir: Option<&Path>,
        arguments_json: &str,
        _session_id: Option<&str>,
        _turn_id: Option<&str>,
        _config: Option<&autonoetic_types::config::GatewayConfig>,
    ) -> anyhow::Result<String> {
        let args: AgentExistsArgs = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;

        validate_agent_id(&args.agent_id)?;

        let agents_dir = agent_dir
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Agent directory is missing its agents root parent"))?;

        let repo = crate::agent::AgentRepository::new(agents_dir.to_path_buf());

        match repo.get_sync(&args.agent_id) {
            Ok(_) => Ok(serde_json::json!({
                "ok": true,
                "exists": true,
                "agent_id": args.agent_id,
                "status": "healthy",
            })
            .to_string()),
            Err(e) => {
                let error_msg = e.to_string();
                if error_msg.contains("not found") {
                    Ok(serde_json::json!({
                        "ok": true,
                        "exists": false,
                        "agent_id": args.agent_id,
                        "status": "not_found",
                    })
                    .to_string())
                } else if error_msg.contains("identity mismatch") {
                    Ok(serde_json::json!({
                        "ok": true,
                        "exists": true,
                        "agent_id": args.agent_id,
                        "status": "identity_mismatch",
                        "error": error_msg,
                        "message": "Agent directory exists but manifest ID does not match directory name. This agent needs to be fixed before use."
                    })
                    .to_string())
                } else {
                    Ok(serde_json::json!({
                        "ok": true,
                        "exists": true,
                        "agent_id": args.agent_id,
                        "status": "load_error",
                        "error": error_msg,
                        "message": "Agent exists but cannot be loaded. Check SKILL.md syntax or file permissions."
                    })
                    .to_string())
                }
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct AgentDiscoverArgs {
    intent: String,
    #[serde(default)]
    required_capabilities: Vec<String>,
    #[serde(default)]
    exclude_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
struct AgentDiscoveryResult {
    score: f64,
    agent_id: String,
    name: String,
    description: String,
    capabilities: Vec<String>,
    match_reasons: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    io: Option<serde_json::Value>,
}

pub struct AgentDiscoverTool;

impl NativeTool for AgentDiscoverTool {
    fn name(&self) -> &'static str {
        "agent.discover"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::AgentSpawn { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Discover existing agents that match the given intent and capabilities. Returns ranked candidates with match scores and reasons. Use this before deciding to install a new agent to prefer reuse over creation.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "intent": { "type": "string", "description": "The task intent or goal to match against agent descriptions" },
                    "required_capabilities": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "List of required capability types (e.g., 'CodeExecution', 'WriteAccess', 'NetworkAccess')"
                    },
                    "exclude_ids": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Agent IDs to exclude from results"
                    }
                },
                "required": ["intent"],
                "additionalProperties": false
            }),
        }
    }

    fn execute(
        &self,
        _manifest: &AgentManifest,
        _policy: &PolicyEngine,
        agent_dir: &Path,
        _gateway_dir: Option<&Path>,
        arguments_json: &str,
        _session_id: Option<&str>,
        _turn_id: Option<&str>,
        _config: Option<&autonoetic_types::config::GatewayConfig>,
    ) -> anyhow::Result<String> {
        let args: AgentDiscoverArgs = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;

        anyhow::ensure!(!args.intent.trim().is_empty(), "intent must not be empty");

        let agents_dir = agent_dir
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Agent directory is missing its agents root parent"))?;

        let repo = crate::agent::AgentRepository::new(agents_dir.to_path_buf());
        let loaded_agents = repo.list_loaded_sync()?;

        let mut results: Vec<AgentDiscoveryResult> = loaded_agents
            .into_iter()
            .filter(|agent| !args.exclude_ids.contains(&agent.id().to_string()))
            .map(|agent| {
                let mut score = 0.0;
                let mut match_reasons = Vec::new();

                let description_lower = agent.instructions.to_lowercase();
                let intent_lower = args.intent.to_lowercase();

                if description_lower.contains(&intent_lower) {
                    score += 30.0;
                    match_reasons.push("exact intent match in description".to_string());
                } else {
                    let keywords: Vec<String> = intent_lower
                        .split_whitespace()
                        .filter(|w| w.len() > 3)
                        .map(|s| s.to_string())
                        .collect();
                    let matched_keywords: Vec<String> = keywords
                        .iter()
                        .filter(|k| description_lower.contains(*k))
                        .cloned()
                        .collect();
                    if !matched_keywords.is_empty() {
                        let keyword_score =
                            (matched_keywords.len() as f64 / keywords.len() as f64) * 20.0;
                        score += keyword_score;
                        match_reasons.push(format!("keyword match: {:?}", matched_keywords));
                    }
                }

                let agent_cap_types: Vec<String> = agent
                    .manifest
                    .capabilities
                    .iter()
                    .map(|c| capability_type_name(c))
                    .collect();

                for req_cap in &args.required_capabilities {
                    if agent_cap_types.iter().any(|cap| cap == req_cap) {
                        score += 15.0;
                        match_reasons.push(format!("has required capability: {}", req_cap));
                    }
                }

                if agent
                    .manifest
                    .background
                    .as_ref()
                    .map(|b| b.enabled)
                    .unwrap_or(false)
                {
                    score += 5.0;
                    match_reasons.push("supports background execution".to_string());
                }

                let io_schema = agent.manifest.io.as_ref().map(|io| {
                    serde_json::json!({
                        "accepts": io.accepts,
                        "returns": io.returns,
                    })
                });

                AgentDiscoveryResult {
                    score,
                    agent_id: agent.id().to_string(),
                    name: agent.manifest.agent.name,
                    description: agent.manifest.agent.description,
                    capabilities: agent_cap_types,
                    match_reasons,
                    io: io_schema,
                }
            })
            .filter(|r| r.score > 0.0)
            .collect();

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(serde_json::json!({
            "ok": true,
            "query": {
                "intent": args.intent,
                "required_capabilities": args.required_capabilities,
            },
            "results": results,
            "result_count": results.len(),
        })
        .to_string())
    }
}

fn capability_type_name(cap: &Capability) -> String {
    match cap {
        Capability::SandboxFunctions { .. } => "SandboxFunctions".to_string(),
        Capability::ReadAccess { .. } => "ReadAccess".to_string(),
        Capability::WriteAccess { .. } => "WriteAccess".to_string(),
        Capability::NetworkAccess { .. } => "NetworkAccess".to_string(),
        Capability::AgentSpawn { .. } => "AgentSpawn".to_string(),
        Capability::AgentMessage { .. } => "AgentMessage".to_string(),
        Capability::BackgroundReevaluation { .. } => "BackgroundReevaluation".to_string(),
        Capability::CodeExecution { .. } => "CodeExecution".to_string(),
    }
}

/// Builds the default registry with the core native tools.

#[derive(Debug, Deserialize)]
pub(crate) struct SandboxExecArgs {
    command: String,
    #[serde(default)]
    dependencies: Option<SandboxExecDependencies>,
    #[serde(default)]
    approval_ref: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SandboxExecDependencies {
    pub runtime: String,
    pub packages: Vec<String>,
}

fn dependency_plan_from_args_or_lock(
    manifest: &AgentManifest,
    agent_dir: &Path,
    deps: Option<SandboxExecDependencies>,
) -> anyhow::Result<Option<DependencyPlan>> {
    if let Some(deps) = deps {
        return parse_dependency_plan(deps.runtime.as_str(), deps.packages).map(Some);
    }

    let lock_path = agent_dir.join(&manifest.runtime.runtime_lock);
    if !lock_path.exists() {
        return Ok(None);
    }
    let lock = crate::runtime_lock::resolve_runtime_lock(&lock_path)?;
    if lock.dependencies.is_empty() {
        return Ok(None);
    }
    anyhow::ensure!(
        lock.dependencies.len() == 1,
        "runtime.lock currently supports exactly one dependency set"
    );
    let locked = &lock.dependencies[0];
    parse_dependency_plan(locked.runtime.as_str(), locked.packages.clone()).map(Some)
}
fn parse_dependency_plan(runtime: &str, packages: Vec<String>) -> anyhow::Result<DependencyPlan> {
    let runtime = match runtime.to_ascii_lowercase().as_str() {
        "python" => DependencyRuntime::Python,
        "nodejs" | "node" => DependencyRuntime::NodeJs,
        other => anyhow::bail!("Unsupported dependency runtime '{}'", other),
    };
    // Empty packages is OK - means use runtime from sandbox without extra packages
    Ok(DependencyPlan { runtime, packages })
}

pub fn default_registry() -> NativeToolRegistry {
    let mut registry = NativeToolRegistry::new();
    registry.register(Box::new(SandboxExecTool));
    registry.register(Box::new(WebSearchTool));
    registry.register(Box::new(WebFetchTool));
    // Content-addressable storage tools
    registry.register(Box::new(ContentWriteTool));
    registry.register(Box::new(ContentReadTool));
    registry.register(Box::new(ContentPersistTool));
    // Knowledge tools (durable facts with provenance)
    registry.register(Box::new(KnowledgeStoreTool));
    registry.register(Box::new(KnowledgeRecallTool));
    registry.register(Box::new(KnowledgeSearchTool));
    registry.register(Box::new(KnowledgeShareTool));
    // Session tools
    registry.register(Box::new(SessionSnapshotTool));
    // Agent tools
    registry.register(Box::new(AgentSpawnTool));
    registry.register(Box::new(AgentInstallTool));
    registry.register(Box::new(AgentExistsTool));
    registry.register(Box::new(AgentDiscoverTool));
    registry
}

#[cfg(test)]
mod tests {
    use super::*;
    use autonoetic_types::agent::{AgentIdentity, RuntimeDeclaration};
    use autonoetic_types::capability::Capability;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::thread;
    use tempfile::tempdir;

    fn test_manifest(capabilities: Vec<Capability>) -> AgentManifest {
        test_manifest_with_id("test-agent", capabilities)
    }

    /// Creates a manifest for an evolution role (specialized_builder or evolution-steward).
    /// These roles have access to agent.install.
    fn test_evolution_manifest(capabilities: Vec<Capability>) -> AgentManifest {
        test_manifest_with_id("specialized_builder.default", capabilities)
    }

    fn test_manifest_with_id(agent_id: &str, capabilities: Vec<Capability>) -> AgentManifest {
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
                id: agent_id.to_string(),
                name: agent_id.to_string(),
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

    fn spawn_one_shot_http_server(
        status: &str,
        content_type: &str,
        body: String,
    ) -> (String, thread::JoinHandle<()>) {
        let status = status.to_string();
        let content_type = content_type.to_string();
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let addr = listener
            .local_addr()
            .expect("listener should expose local addr");
        let handle = thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut request_buf = [0_u8; 2048];
                let _ = stream.read(&mut request_buf);
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        (format!("http://{}", addr), handle)
    }

    fn spawn_counting_http_server(
        status: &str,
        content_type: &str,
        body: String,
        expected_requests: usize,
    ) -> (String, Arc<AtomicUsize>, thread::JoinHandle<()>) {
        let status = status.to_string();
        let content_type = content_type.to_string();
        let hits = Arc::new(AtomicUsize::new(0));
        let hits_clone = Arc::clone(&hits);
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let addr = listener
            .local_addr()
            .expect("listener should expose local addr");
        let handle = thread::spawn(move || {
            for _ in 0..expected_requests {
                if let Ok((mut stream, _)) = listener.accept() {
                    hits_clone.fetch_add(1, Ordering::SeqCst);
                    let mut request_buf = [0_u8; 2048];
                    let _ = stream.read(&mut request_buf);
                    let response = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                }
            }
        });
        (format!("http://{}", addr), hits, handle)
    }

    #[test]
    fn test_native_tool_registry_availability() {
        let registry = default_registry();
        let manifest_none = test_manifest(vec![]);
        assert_eq!(registry.available_definitions(&manifest_none).len(), 0);

        let manifest_shell = test_manifest(vec![Capability::CodeExecution {
            patterns: vec!["*".into()],
        }]);
        let defs = registry.available_definitions(&manifest_shell);
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "sandbox.exec");

        let manifest_all = test_manifest(vec![
            Capability::CodeExecution { patterns: vec![] },
            Capability::ReadAccess { scopes: vec![] },
            Capability::WriteAccess { scopes: vec![] },
        ]);
        let defs_all = registry.available_definitions(&manifest_all);
        // sandbox.exec (1) + content.write, content.read, content.persist (3) +
        // knowledge.store, knowledge.recall, knowledge.search (3) +
        // session.snapshot (1) + knowledge.share (1) = 9
        assert_eq!(defs_all.len(), 9);

        let manifest_spawn = test_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
        let defs_spawn = registry.available_definitions(&manifest_spawn);
        // agent.spawn, agent.exists, agent.discover = 3 (agent.install is evolution-role only)
        assert_eq!(defs_spawn.len(), 3);
        assert!(defs_spawn.iter().any(|d| d.name == "agent.spawn"));
        assert!(
            !defs_spawn.iter().any(|d| d.name == "agent.install"),
            "agent.install should NOT be available to non-evolution roles"
        );
        assert!(defs_spawn.iter().any(|d| d.name == "agent.exists"));
        assert!(defs_spawn.iter().any(|d| d.name == "agent.discover"));

        // agent.install should be available to evolution roles
        let manifest_evolution =
            test_evolution_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
        let defs_evolution = registry.available_definitions(&manifest_evolution);
        assert!(
            defs_evolution.iter().any(|d| d.name == "agent.install"),
            "agent.install should be available to evolution roles"
        );

        let manifest_net = test_manifest(vec![Capability::NetworkAccess {
            hosts: vec!["*".to_string()],
        }]);
        let defs_net = registry.available_definitions(&manifest_net);
        assert_eq!(defs_net.len(), 2);
        assert!(defs_net.iter().any(|d| d.name == "web.search"));
        assert!(defs_net.iter().any(|d| d.name == "web.fetch"));
    }

    #[test]
    fn test_web_fetch_tool_roundtrip_local_server() {
        let manifest = test_manifest(vec![Capability::NetworkAccess {
            hosts: vec!["127.0.0.1".to_string()],
        }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let (base_url, handle) = spawn_one_shot_http_server(
            "200 OK",
            "text/plain; charset=utf-8",
            "hello web fetch".to_string(),
        );

        let args = serde_json::json!({
            "url": format!("{}/doc", base_url),
            "timeout_secs": 10,
            "max_chars": 4096
        });

        let registry = default_registry();
        let result = registry
            .execute(
                "web.fetch",
                &manifest,
                &policy,
                temp.path(),
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
                None,
            )
            .expect("web.fetch should succeed");

        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("web.fetch result should decode");
        assert_eq!(parsed.get("ok"), Some(&serde_json::json!(true)));
        assert!(parsed
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .contains("hello web fetch"));

        handle.join().expect("server thread should join");
    }

    #[test]
    fn test_web_fetch_tool_denied_by_netconnect_policy() {
        let manifest = test_manifest(vec![Capability::NetworkAccess {
            hosts: vec!["example.com".to_string()],
        }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");

        let args = serde_json::json!({
            "url": "http://127.0.0.1:65535/forbidden"
        });

        let registry = default_registry();
        let err = registry
            .execute(
                "web.fetch",
                &manifest,
                &policy,
                temp.path(),
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
                None,
            )
            .expect_err("web.fetch should be denied");
        assert!(err.to_string().contains("NetworkAccess"));
    }

    #[test]
    fn test_web_search_tool_denied_by_netconnect_policy() {
        let manifest = test_manifest(vec![Capability::NetworkAccess {
            hosts: vec!["example.com".to_string()],
        }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");

        let args = serde_json::json!({
            "query": "rust",
            "engine_url": "http://127.0.0.1:65535/search"
        });

        let registry = default_registry();
        let err = registry
            .execute(
                "web.search",
                &manifest,
                &policy,
                temp.path(),
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
                None,
            )
            .expect_err("web.search should be denied");
        assert!(err.to_string().contains("NetworkAccess"));
    }

    #[test]
    fn test_web_search_tool_roundtrip_local_engine() {
        let manifest = test_manifest(vec![Capability::NetworkAccess {
            hosts: vec!["127.0.0.1".to_string()],
        }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let body = serde_json::json!({
            "Results": [],
            "RelatedTopics": [
                {
                    "Text": "Rust language homepage",
                    "FirstURL": "https://www.rust-lang.org/"
                },
                {
                    "Name": "Docs",
                    "Topics": [
                        {
                            "Text": "The Rust book",
                            "FirstURL": "https://doc.rust-lang.org/book/"
                        }
                    ]
                }
            ]
        })
        .to_string();
        let (engine_url, handle) = spawn_one_shot_http_server("200 OK", "application/json", body);

        let args = serde_json::json!({
            "query": "rust language",
            "provider": "duckduckgo",
            "engine_url": engine_url,
            "max_results": 5
        });

        let registry = default_registry();
        let result = registry
            .execute(
                "web.search",
                &manifest,
                &policy,
                temp.path(),
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
                None,
            )
            .expect("web.search should succeed");

        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("web.search result should decode");
        assert_eq!(parsed.get("ok"), Some(&serde_json::json!(true)));
        assert!(
            parsed
                .get("result_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
                >= 2
        );

        handle.join().expect("server thread should join");
    }

    #[test]
    fn test_web_search_google_requires_api_key_env() {
        let manifest = test_manifest(vec![Capability::NetworkAccess {
            hosts: vec!["127.0.0.1".to_string()],
        }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");

        let args = serde_json::json!({
            "query": "rust",
            "provider": "google",
            "engine_url": "http://127.0.0.1:65535/search",
            "google_engine_id": "cx-test",
            "google_api_key_env": "AUTONOETIC_TEST_GOOGLE_KEY_MISSING"
        });

        let registry = default_registry();
        let err = registry
            .execute(
                "web.search",
                &manifest,
                &policy,
                temp.path(),
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
                None,
            )
            .expect_err("google search without key should fail");
        assert!(err.to_string().contains("requires API key env"));
    }

    #[test]
    fn test_web_search_google_roundtrip_local_engine() {
        let manifest = test_manifest(vec![Capability::NetworkAccess {
            hosts: vec!["127.0.0.1".to_string()],
        }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let body = serde_json::json!({
            "searchInformation": {
                "totalResults": "123"
            },
            "items": [
                {
                    "title": "Rust language",
                    "link": "https://www.rust-lang.org/",
                    "snippet": "Rust empowers everyone."
                },
                {
                    "title": "The Rust Book",
                    "link": "https://doc.rust-lang.org/book/",
                    "snippet": "Learn Rust."
                }
            ]
        })
        .to_string();
        let (engine_url, handle) = spawn_one_shot_http_server("200 OK", "application/json", body);

        let key_env = "AUTONOETIC_TEST_GOOGLE_KEY_OK";
        let cx_env = "AUTONOETIC_TEST_GOOGLE_CX_OK";
        let prior_key = std::env::var(key_env).ok();
        let prior_cx = std::env::var(cx_env).ok();
        std::env::set_var(key_env, "test-api-key");
        std::env::set_var(cx_env, "test-cx-id");

        let args = serde_json::json!({
            "query": "rust language",
            "provider": "google",
            "engine_url": engine_url,
            "google_api_key_env": key_env,
            "google_engine_id_env": cx_env
        });

        let registry = default_registry();
        let result = registry
            .execute(
                "web.search",
                &manifest,
                &policy,
                temp.path(),
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
                None,
            )
            .expect("google web.search should succeed");

        match prior_key {
            Some(value) => std::env::set_var(key_env, value),
            None => std::env::remove_var(key_env),
        }
        match prior_cx {
            Some(value) => std::env::set_var(cx_env, value),
            None => std::env::remove_var(cx_env),
        }
        handle.join().expect("server thread should join");

        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("web.search result should decode");
        assert_eq!(parsed.get("ok"), Some(&serde_json::json!(true)));
        assert_eq!(parsed.get("provider"), Some(&serde_json::json!("google")));
        assert_eq!(parsed.get("total_results"), Some(&serde_json::json!(123)));
        assert_eq!(parsed.get("result_count"), Some(&serde_json::json!(2)));
    }

    #[test]
    fn test_web_search_google_legacy_cx_env_alias_roundtrip() {
        let manifest = test_manifest(vec![Capability::NetworkAccess {
            hosts: vec!["127.0.0.1".to_string()],
        }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");

        let body = serde_json::json!({
            "searchInformation": {
                "totalResults": "7"
            },
            "items": [
                {
                    "title": "Example result",
                    "link": "https://example.com/",
                    "snippet": "example"
                }
            ]
        })
        .to_string();
        let (engine_url, handle) = spawn_one_shot_http_server("200 OK", "application/json", body);

        let key_env = "GOOGLE_SEARCH_API_KEY";
        let cx_env = "GOOGLE_SEARCH_CX";
        let prior_key = std::env::var(key_env).ok();
        let prior_cx = std::env::var(cx_env).ok();
        std::env::set_var(key_env, "legacy-test-api-key");
        std::env::set_var(cx_env, "legacy-test-cx");

        let args = serde_json::json!({
            "query": "legacy cx alias",
            "provider": "google",
            "engine_url": engine_url
        });

        let registry = default_registry();
        let result = registry
            .execute(
                "web.search",
                &manifest,
                &policy,
                temp.path(),
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
                None,
            )
            .expect("google web.search should accept GOOGLE_SEARCH_CX legacy alias");

        match prior_key {
            Some(value) => std::env::set_var(key_env, value),
            None => std::env::remove_var(key_env),
        }
        match prior_cx {
            Some(value) => std::env::set_var(cx_env, value),
            None => std::env::remove_var(cx_env),
        }
        handle.join().expect("server thread should join");

        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("web.search result should decode");
        assert_eq!(parsed.get("ok"), Some(&serde_json::json!(true)));
        assert_eq!(parsed.get("provider"), Some(&serde_json::json!("google")));
        assert_eq!(parsed.get("result_count"), Some(&serde_json::json!(1)));
    }

    #[test]
    fn test_web_search_auto_falls_back_to_duckduckgo_when_google_fails() {
        let manifest = test_manifest(vec![Capability::NetworkAccess {
            hosts: vec!["127.0.0.1".to_string()],
        }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");

        let google_body = serde_json::json!({
            "error": { "message": "quota exceeded" }
        })
        .to_string();
        let (google_engine_url, google_handle) = spawn_one_shot_http_server(
            "500 Internal Server Error",
            "application/json",
            google_body,
        );

        let ddg_body = serde_json::json!({
            "Results": [],
            "RelatedTopics": [
                {
                    "Text": "Rust official site",
                    "FirstURL": "https://www.rust-lang.org/"
                }
            ]
        })
        .to_string();
        let (duckduckgo_engine_url, ddg_handle) =
            spawn_one_shot_http_server("200 OK", "application/json", ddg_body);

        let key_env = "AUTONOETIC_TEST_GOOGLE_KEY_AUTO";
        let cx_env = "AUTONOETIC_TEST_GOOGLE_CX_AUTO";
        let prior_key = std::env::var(key_env).ok();
        let prior_cx = std::env::var(cx_env).ok();
        std::env::set_var(key_env, "test-api-key");
        std::env::set_var(cx_env, "test-cx-id");

        let args = serde_json::json!({
            "query": "rust language",
            "provider": "auto",
            "google_engine_url": google_engine_url,
            "duckduckgo_engine_url": duckduckgo_engine_url,
            "google_api_key_env": key_env,
            "google_engine_id_env": cx_env
        });

        let registry = default_registry();
        let result = registry
            .execute(
                "web.search",
                &manifest,
                &policy,
                temp.path(),
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
                None,
            )
            .expect("auto provider should fall back to duckduckgo");

        match prior_key {
            Some(value) => std::env::set_var(key_env, value),
            None => std::env::remove_var(key_env),
        }
        match prior_cx {
            Some(value) => std::env::set_var(cx_env, value),
            None => std::env::remove_var(cx_env),
        }

        google_handle
            .join()
            .expect("google server thread should join");
        ddg_handle.join().expect("ddg server thread should join");

        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("web.search result should decode");
        assert_eq!(parsed.get("ok"), Some(&serde_json::json!(true)));
        assert_eq!(
            parsed.get("requested_provider"),
            Some(&serde_json::json!("auto"))
        );
        assert_eq!(
            parsed.get("provider"),
            Some(&serde_json::json!("duckduckgo"))
        );
        let attempted = parsed
            .get("attempted_providers")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        assert!(attempted.contains(&serde_json::json!("google")));
        assert!(attempted.contains(&serde_json::json!("duckduckgo")));
        assert!(parsed
            .get("fallback_reason")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .contains("google provider failed"));
    }

    #[test]
    fn test_web_search_cache_hits_without_second_network_call() {
        let manifest = test_manifest(vec![Capability::NetworkAccess {
            hosts: vec!["127.0.0.1".to_string()],
        }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");

        let body = serde_json::json!({
            "Results": [],
            "RelatedTopics": [
                {
                    "Text": "Rust language homepage",
                    "FirstURL": "https://www.rust-lang.org/"
                }
            ]
        })
        .to_string();
        let (engine_url, hits, handle) =
            spawn_counting_http_server("200 OK", "application/json", body, 1);

        let unique_query = format!(
            "rust cache {}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be after unix epoch")
                .as_nanos()
        );
        let args = serde_json::json!({
            "query": unique_query,
            "provider": "duckduckgo",
            "duckduckgo_engine_url": engine_url,
            "cache_ttl_secs": 300
        });

        let registry = default_registry();
        let first = registry
            .execute(
                "web.search",
                &manifest,
                &policy,
                temp.path(),
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
                None,
            )
            .expect("first web.search call should succeed");
        let second = registry
            .execute(
                "web.search",
                &manifest,
                &policy,
                temp.path(),
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
                None,
            )
            .expect("second web.search call should succeed");

        let first_parsed: serde_json::Value =
            serde_json::from_str(&first).expect("first response should decode");
        let second_parsed: serde_json::Value =
            serde_json::from_str(&second).expect("second response should decode");
        assert_eq!(
            first_parsed.get("cache_hit"),
            Some(&serde_json::json!(false))
        );
        assert_eq!(
            second_parsed.get("cache_hit"),
            Some(&serde_json::json!(true))
        );
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        handle.join().expect("server thread should join");
    }

    #[test]
    fn test_agent_spawn_tool_validates_non_empty_message() {
        let manifest = test_manifest(vec![Capability::AgentSpawn { max_children: 2 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("planner.default");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        let args = serde_json::json!({
            "agent_id": "researcher.default",
            "message": ""
        });

        let registry = default_registry();
        let err = registry
            .execute(
                "agent.spawn",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                Some("session-1"),
                None,
                None,
            )
            .expect_err("empty message should be rejected");
        assert!(err.to_string().contains("message must not be empty"));
    }

    #[test]
    fn test_agent_spawn_tool_accepts_metadata_argument() {
        let manifest = test_manifest(vec![Capability::AgentSpawn { max_children: 2 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("planner.default");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        let args = serde_json::json!({
            "agent_id": "researcher.default",
            "message": "",
            "metadata": {
                "delegated_role": "researcher",
                "expected_outputs": ["summary.md", "sources.json"]
            }
        });

        let registry = default_registry();
        let err = registry
            .execute(
                "agent.spawn",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                Some("session-1"),
                None,
                None,
            )
            .expect_err("empty message should still be rejected");
        assert!(err.to_string().contains("message must not be empty"));
    }

    #[test]
    fn test_agent_install_requires_promotion_gate_for_evolution_roles() {
        let manifest = test_manifest_with_id(
            "specialized_builder.default",
            vec![Capability::AgentSpawn { max_children: 4 }],
        );
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("specialized_builder.default");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        let args = serde_json::json!({
            "agent_id": "child.worker",
            "instructions": "# Child Worker\nDo one job."
        });

        let registry = default_registry();
        let err = registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
                None,
            )
            .expect_err("install should require promotion gate");
        assert!(err.to_string().contains("promotion_gate"));
    }

    #[test]
    fn test_agent_install_with_net_connect_requires_approval() {
        let manifest = test_evolution_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("builder");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        let args = serde_json::json!({
            "agent_id": "net.worker",
            "instructions": "Worker with network access.",
            "capabilities": [
                { "type": "NetworkAccess", "hosts": ["api.example.com"] }
            ],
            "promotion_gate": {
                "evaluator_pass": true,
                "auditor_pass": true,
                "security_analysis": {
                    "passed": true,
                    "threats_detected": [],
                    "remote_access_detected": true
                },
                "capability_analysis": {
                    "inferred_capabilities": ["NetworkAccess"],
                    "missing_capabilities": [],
                    "declared_capabilities": ["NetworkAccess"],
                    "analysis_passed": true
                }
            }
        });

        let config = GatewayConfig {
            agents_dir: agents_dir.clone(),
            agent_install_approval_policy: AgentInstallApprovalPolicy::RiskBased,
            ..Default::default()
        };

        let registry = default_registry();
        let result = registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
                Some(&config),
            )
            .expect("install should return structured approval request");

        let parsed: serde_json::Value = serde_json::from_str(&result).expect("json");
        assert_eq!(parsed.get("ok").and_then(|v| v.as_bool()), Some(false));
        assert_eq!(
            parsed.get("approval_required").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert!(parsed.get("request_id").is_some());
    }

    #[test]
    fn test_agent_install_allows_promotion_gate_for_evolution_roles() {
        let manifest = test_manifest_with_id(
            "specialized_builder.default",
            vec![Capability::AgentSpawn { max_children: 4 }],
        );
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("specialized_builder.default");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        let args = serde_json::json!({
            "agent_id": "child.worker",
            "instructions": "# Child Worker\nDo one job.",
            "promotion_gate": {
                "evaluator_pass": true,
                "auditor_pass": true
            }
        });

        let registry = default_registry();
        let result = registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
                None,
            )
            .expect("install should pass with promotion gate");
        assert!(result.contains("\"ok\":true"));
        assert!(agents_dir.join("child.worker").join("SKILL.md").exists());
    }

    #[test]
    fn test_agent_install_tool_creates_background_child_agent() {
        let manifest = test_evolution_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("builder_agent");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        let args = serde_json::json!({
            "agent_id": "fib_worker",
            "name": "fib_worker",
            "description": "Computes Fibonacci values on a schedule",
            "instructions": "# Fibonacci Worker\nMaintain the worker assets already installed in this directory.",
            "capabilities": [
                {"type": "ReadAccess", "scopes": ["self.*"]},
                {"type": "WriteAccess", "scopes": ["self.*"]}
            ],
            "background": {
                "enabled": true,
                "interval_secs": 20,
                "mode": "deterministic",
                "wake_predicates": {
                    "timer": true,
                    "new_messages": false,
                    "task_completions": false,
                    "queued_work": false,
                    "stale_goals": false,
                    "retryable_failures": false,
                    "approval_resolved": false
                }
            },
            "scheduled_action": {
                "type": "sandbox_exec",
                "command": "python3 scripts/fibonacci_worker.py"
            },
            "validate_on_install": false,
            "files": [
                {
                    "path": "scripts/fibonacci_worker.py",
                    "content": "import json\nfrom pathlib import Path\nstate_path = Path('state/fib.json')\nstate = json.loads(state_path.read_text())\nstate['previous'], state['current'] = state['current'], state['previous'] + state['current']\nstate['index'] += 1\nstate_path.write_text(json.dumps(state))\n"
                },
                {
                    "path": "state/fib.json",
                    "content": "{\"previous\": 0, \"current\": 1, \"index\": 1}"
                }
            ],
            "promotion_gate": {
                "evaluator_pass": true,
                "auditor_pass": true
            }
        });

        let registry = default_registry();
        registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
                None,
            )
            .expect("agent install should succeed");

        let child_dir = agents_dir.join("fib_worker");
        assert!(child_dir.join("SKILL.md").exists());
        assert!(child_dir.join("runtime.lock").exists());
        assert!(child_dir
            .join("scripts")
            .join("fibonacci_worker.py")
            .exists());
        assert!(child_dir.join("state").join("fib.json").exists());
        assert!(child_dir.join("state").join("reevaluation.json").exists());

        let skill = std::fs::read_to_string(child_dir.join("SKILL.md")).expect("skill should read");
        assert!(skill.contains("name: fib_worker"));
        assert!(skill.contains("metadata:\n  autonoetic:"));
        assert!(skill.contains("agent:\n      id: fib_worker"));
        assert!(skill.contains("type: BackgroundReevaluation"));
        assert!(skill.contains("type: CodeExecution"));
        assert!(skill.contains("## Output Contract"));

        let background_state = std::fs::read_to_string(
            agents_dir
                .join(".gateway")
                .join("scheduler")
                .join("agents")
                .join("fib_worker.json"),
        )
        .expect("background state should exist");
        assert!(background_state.contains("background::fib_worker"));
    }

    #[test]
    fn test_agent_install_tool_allows_dotted_agent_ids() {
        let manifest = test_evolution_manifest(vec![Capability::AgentSpawn { max_children: 2 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("planner.default");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        let args = serde_json::json!({
            "agent_id": "researcher.default",
            "name": "Researcher Default",
            "description": "Research specialist",
            "instructions": "# Researcher Default\nCollect evidence and summarize it.",
            "promotion_gate": {
                "evaluator_pass": true,
                "auditor_pass": true
            }
        });

        let registry = default_registry();
        registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
                None,
            )
            .expect("agent install should accept dotted IDs");

        let child_dir = agents_dir.join("researcher.default");
        assert!(child_dir.join("SKILL.md").exists());
        assert!(child_dir.join("runtime.lock").exists());

        let skill = std::fs::read_to_string(child_dir.join("SKILL.md")).expect("skill should read");
        assert!(skill.contains("id: researcher.default"));
    }

    #[test]
    fn test_agent_install_tool_accepts_scheduled_action_shorthand() {
        let manifest = test_evolution_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("builder_agent");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        let args = serde_json::json!({
            "agent_id": "fib_worker",
            "instructions": "# Worker\nInstalled from shorthand scheduled_action.",
            "background": {
                "enabled": true,
                "interval_secs": 20,
                "mode": "deterministic",
                "wake_predicates": {
                    "timer": true,
                    "new_messages": false,
                    "task_completions": false,
                    "queued_work": false,
                    "stale_goals": false,
                    "retryable_failures": false,
                    "approval_resolved": false
                }
            },
            "scheduled_action": {
                "command": "python3 scripts/fibonacci_worker.py"
            },
            "validate_on_install": false,
            "promotion_gate": {
                "evaluator_pass": true,
                "auditor_pass": true
            }
        });

        let registry = default_registry();
        let result = registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
                None,
            )
            .expect("agent install should accept shorthand");

        assert!(result.contains("sandbox_exec"));

        let reevaluation = std::fs::read_to_string(
            agents_dir
                .join("fib_worker")
                .join("state")
                .join("reevaluation.json"),
        )
        .expect("reevaluation state should exist");
        assert!(reevaluation.contains("python3 scripts/fibonacci_worker.py"));
    }

    #[test]
    fn test_agent_install_tool_accepts_script_and_interval_shorthand() {
        let manifest = test_evolution_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("builder_agent");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        let args = serde_json::json!({
            "agent_id": "fib_worker",
            "instructions": "# Worker\nInstalled from real model shorthand.",
            "background": {
                "mode": "deterministic"
            },
            "scheduled_action": {
                "interval_secs": 20,
                "script": "python3 scripts/fibonacci_worker.py"
            },
            "validate_on_install": false,
            "promotion_gate": {
                "evaluator_pass": true,
                "auditor_pass": true
            }
        });

        let registry = default_registry();
        let result = registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
                None,
            )
            .expect("agent install should accept script+interval shorthand");

        assert!(result.contains("sandbox_exec"));

        let child_skill = std::fs::read_to_string(agents_dir.join("fib_worker").join("SKILL.md"))
            .expect("child skill should exist");
        assert!(child_skill.contains("metadata:\n  autonoetic:"));
        assert!(child_skill.contains("enabled: true"));
        assert!(child_skill.contains("interval_secs: 20"));
        assert!(child_skill.contains("## Output Contract"));

        let reevaluation = std::fs::read_to_string(
            agents_dir
                .join("fib_worker")
                .join("state")
                .join("reevaluation.json"),
        )
        .expect("reevaluation state should exist");
        assert!(reevaluation.contains("python3 scripts/fibonacci_worker.py"));
    }

    #[test]
    fn test_agent_install_tool_accepts_tool_use_and_cadence_shorthand() {
        let manifest = test_evolution_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("builder_agent");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        let args = serde_json::json!({
            "agent_id": "sequence_worker",
            "instructions": "# Worker\nInstalled from tool_use shorthand.",
            "background": {
                "mode": "deterministic"
            },
            "scheduled_action": {
                "cadence": "20s",
                "tool_use": {
                    "name": "sandbox.exec",
                    "arguments": {
                        "cmd": "python3 scripts/compute.py"
                    }
                }
            },
            "validate_on_install": false,
            "promotion_gate": {
                "evaluator_pass": true,
                "auditor_pass": true
            }
        });

        let registry = default_registry();
        let result = registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
                None,
            )
            .expect("agent install should accept tool_use+cadence shorthand");

        assert!(result.contains("sandbox_exec"));

        let child_skill =
            std::fs::read_to_string(agents_dir.join("sequence_worker").join("SKILL.md"))
                .expect("child skill should exist");
        assert!(child_skill.contains("metadata:\n  autonoetic:"));
        assert!(child_skill.contains("enabled: true"));
        assert!(child_skill.contains("interval_secs: 20"));
        assert!(child_skill.contains("## Output Contract"));

        let reevaluation = std::fs::read_to_string(
            agents_dir
                .join("sequence_worker")
                .join("state")
                .join("reevaluation.json"),
        )
        .expect("reevaluation state should exist");
        assert!(reevaluation.contains("python3 scripts/compute.py"));
    }

    #[test]
    fn test_dependency_plan_from_args_python() {
        let manifest = test_manifest(vec![]);
        let temp = tempdir().expect("tempdir should create");
        let plan = dependency_plan_from_args_or_lock(
            &manifest,
            temp.path(),
            Some(SandboxExecDependencies {
                runtime: "python".to_string(),
                packages: vec!["requests==2.32.3".to_string()],
            }),
        )
        .expect("plan should parse")
        .expect("plan should exist");
        assert_eq!(plan.runtime, DependencyRuntime::Python);
        assert_eq!(plan.packages.len(), 1);
    }

    #[test]
    fn test_dependency_plan_from_args_unsupported_runtime() {
        let manifest = test_manifest(vec![]);
        let temp = tempdir().expect("tempdir should create");
        let err = dependency_plan_from_args_or_lock(
            &manifest,
            temp.path(),
            Some(SandboxExecDependencies {
                runtime: "ruby".to_string(),
                packages: vec!["rack".to_string()],
            }),
        )
        .expect_err("unsupported runtime should fail");
        assert!(err.to_string().contains("Unsupported dependency runtime"));
    }

    #[test]
    fn test_dependency_plan_from_runtime_lock_default() {
        let manifest = test_manifest(vec![]);
        let temp = tempdir().expect("tempdir should create");
        let lock_path = temp.path().join("runtime.lock");
        std::fs::write(
            &lock_path,
            r#"
gateway:
  artifact: "autonoetic-gateway"
  version: "0.1.0"
  sha256: "abc"
sdk:
  version: "0.1.0"
sandbox:
  backend: "bubblewrap"
dependencies:
  - runtime: "python"
    packages:
      - "requests==2.32.3"
"#,
        )
        .expect("runtime.lock should write");

        let plan = dependency_plan_from_args_or_lock(&manifest, temp.path(), None)
            .expect("plan should parse")
            .expect("plan should exist");
        assert_eq!(plan.runtime, DependencyRuntime::Python);
        assert_eq!(plan.packages, vec!["requests==2.32.3".to_string()]);
    }

    #[test]
    fn test_dependency_plan_from_args_overrides_runtime_lock() {
        let manifest = test_manifest(vec![]);
        let temp = tempdir().expect("tempdir should create");
        let lock_path = temp.path().join("runtime.lock");
        std::fs::write(
            &lock_path,
            r#"
gateway:
  artifact: "autonoetic-gateway"
  version: "0.1.0"
  sha256: "abc"
sdk:
  version: "0.1.0"
sandbox:
  backend: "bubblewrap"
dependencies:
  - runtime: "python"
    packages:
      - "requests==2.32.3"
"#,
        )
        .expect("runtime.lock should write");

        let plan = dependency_plan_from_args_or_lock(
            &manifest,
            temp.path(),
            Some(SandboxExecDependencies {
                runtime: "nodejs".to_string(),
                packages: vec!["lodash@4.17.21".to_string()],
            }),
        )
        .expect("plan should parse")
        .expect("plan should exist");
        assert_eq!(plan.runtime, DependencyRuntime::NodeJs);
        assert_eq!(plan.packages, vec!["lodash@4.17.21".to_string()]);
    }

    #[test]
    fn test_execute_sandbox_tool_call_denied_by_policy() {
        let manifest = test_manifest(vec![Capability::CodeExecution {
            patterns: vec!["python3 scripts/*".to_string()],
        }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let args = serde_json::json!({
            "command": "echo should_fail"
        });

        let tool = default_registry();
        let err = tool
            .execute(
                "sandbox.exec",
                &manifest,
                &policy,
                temp.path(),
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
                None,
            )
            .expect_err("policy should deny command");
        assert!(err
            .to_string()
            .contains("sandbox command denied by CodeExecution policy"));
    }

    #[test]
    fn test_sandbox_exec_approved_retry_skips_second_remote_approval_gate() {
        let manifest = test_manifest(vec![Capability::CodeExecution {
            patterns: vec!["curl *".to_string()],
        }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let agent_dir = agents_dir.join("tester");
        std::fs::create_dir_all(&agent_dir).expect("agent dir should create");

        let config = GatewayConfig {
            agents_dir: agents_dir.clone(),
            ..Default::default()
        };

        // Include an invalid dependency runtime so we can prove the second call
        // passes the remote approval gate and reaches dependency parsing.
        let command = "curl https://api.open-meteo.com";
        let base_args = serde_json::json!({
            "command": command,
            "dependencies": {
                "runtime": "invalid-runtime",
                "packages": ["foo"]
            }
        });

        let registry = default_registry();

        // First call (no approval_ref): should stop at approval_required before dependency parsing.
        let first = registry
            .execute(
                "sandbox.exec",
                &manifest,
                &policy,
                &agent_dir,
                None,
                &serde_json::to_string(&base_args).expect("json should encode"),
                Some("test-session"),
                None,
                Some(&config),
            )
            .expect("first call should return approval response");
        let first_json: serde_json::Value =
            serde_json::from_str(&first).expect("json should parse");
        assert_eq!(
            first_json
                .get("approval_required")
                .and_then(|v| v.as_bool()),
            Some(true)
        );

        // Write an approved decision for the same command.
        let request_id = "apr-test1234";
        let approved_dir = agents_dir
            .join(".gateway")
            .join("scheduler")
            .join("approvals")
            .join("approved");
        std::fs::create_dir_all(&approved_dir).expect("approved dir should create");

        let approval_decision = serde_json::json!({
            "request_id": request_id,
            "agent_id": manifest.agent.id,
            "session_id": "test-session",
            "action": {
                "type": "sandbox_exec",
                "command": command,
                "requires_approval": true
            },
            "status": "approved",
            "decided_at": "2026-03-18T14:00:00Z",
            "decided_by": "test-user"
        });
        std::fs::write(
            approved_dir.join(format!("{request_id}.json")),
            serde_json::to_string(&approval_decision).expect("json"),
        )
        .expect("write approval decision");

        // Retry with approval_ref: should NOT request approval again.
        // It should continue and fail on dependency parsing instead.
        let retry_args = serde_json::json!({
            "command": command,
            "approval_ref": request_id,
            "dependencies": {
                "runtime": "invalid-runtime",
                "packages": ["foo"]
            }
        });
        let err = registry
            .execute(
                "sandbox.exec",
                &manifest,
                &policy,
                &agent_dir,
                None,
                &serde_json::to_string(&retry_args).expect("json should encode"),
                Some("test-session"),
                None,
                Some(&config),
            )
            .expect_err("retry should reach dependency parsing and fail");
        assert!(err.to_string().contains("Unsupported dependency runtime"));
    }

    #[test]
    fn test_install_time_validation_successful_first_run() {
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("builder_agent");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        let manifest = test_evolution_manifest(vec![Capability::AgentSpawn { max_children: 10 }]);

        let registry = default_registry();
        let policy = PolicyEngine::new(manifest.clone());

        let args = serde_json::json!({
            "agent_id": "test_worker",
            "instructions": "A test worker agent.",
            "background": {
                "enabled": true,
                "interval_secs": 60
            },
            "scheduled_action": {
                "type": "write_file",
                "path": "state/init.json",
                "content": "{\"initialized\": true}"
            },
            "validate_on_install": true,
            "promotion_gate": {
                "evaluator_pass": true,
                "auditor_pass": true
            }
        });

        let result = registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &args.to_string(),
                None,
                None,
                None,
            )
            .expect("install with validation should succeed");

        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("result should be json");
        assert_eq!(parsed.get("ok").unwrap(), true);
        assert_eq!(parsed.get("status").unwrap(), "agent_installed");

        let reevaluation_path = agents_dir
            .join("test_worker")
            .join("state")
            .join("reevaluation.json");
        let reevaluation =
            std::fs::read_to_string(&reevaluation_path).expect("reevaluation state should exist");
        assert!(reevaluation.contains("install_validation_success"));

        let init_file = agents_dir
            .join("test_worker")
            .join("state")
            .join("init.json");
        assert!(
            init_file.exists(),
            "init file should be created during validation"
        );
    }

    #[test]
    fn test_install_time_validation_structured_error_on_failure() {
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("builder_agent");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        let manifest = test_evolution_manifest(vec![Capability::AgentSpawn { max_children: 10 }]);

        let registry = default_registry();
        let policy = PolicyEngine::new(manifest.clone());

        let args = serde_json::json!({
            "agent_id": "failing_worker",
            "instructions": "A worker that fails validation.",
            "background": {
                "enabled": true,
                "interval_secs": 60
            },
            "scheduled_action": {
                "type": "sandbox_exec",
                "command": "exit 1"
            },
            "validate_on_install": true,
            "promotion_gate": {
                "evaluator_pass": true,
                "auditor_pass": true
            }
        });

        let result = registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &args.to_string(),
                None,
                None,
                None,
            )
            .expect("should return structured error, not panic");

        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("result should be json");
        assert_eq!(parsed.get("ok").unwrap(), false);
        assert_eq!(parsed.get("error_type").unwrap(), "execution");
        assert!(parsed.get("message").is_some());
        assert!(parsed.get("repair_hint").is_some() || parsed.get("message").is_some());

        let child_dir = agents_dir.join("failing_worker");
        assert!(
            !child_dir.exists(),
            "failed install validation should not leave a partial agent directory"
        );
    }

    #[test]
    fn test_agent_install_does_not_leave_partial_child_on_file_overwrite_error() {
        let manifest = test_evolution_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("builder_agent");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        let args = serde_json::json!({
            "agent_id": "broken_worker",
            "instructions": "# Broken Worker\nThis install should fail.",
            "files": [
                {
                    "path": "SKILL.md",
                    "content": "should fail because SKILL.md is generated"
                }
            ],
            "promotion_gate": {
                "evaluator_pass": true,
                "auditor_pass": true
            }
        });

        let registry = default_registry();
        let err = registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
                None,
            )
            .expect_err("install should fail when files overwrite generated SKILL.md");
        assert!(err
            .to_string()
            .contains("files may not overwrite generated SKILL.md or runtime.lock"));
        assert!(
            !agents_dir.join("broken_worker").exists(),
            "failed install should not leave partial child directory"
        );
    }

    /// Regression: malformed agent.install capabilities (e.g. missing required fields) yield a
    /// validation error; correcting the payload and retrying leads to successful install without
    /// leaving a partial directory or requiring LoopGuard.
    #[test]
    fn test_agent_install_malformed_capabilities_then_repaired_payload_succeeds() {
        let manifest = test_manifest_with_id(
            "specialized_builder.default",
            vec![Capability::AgentSpawn { max_children: 4 }],
        );
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("specialized_builder.default");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        // Malformed: NetworkAccess without required "hosts" field
        let malformed_args = serde_json::json!({
            "agent_id": "repaired.worker",
            "instructions": "# Repaired Worker\nMinimal specialist.",
            "promotion_gate": { "evaluator_pass": true, "auditor_pass": true },
            "capabilities": [
                { "type": "NetworkAccess" }
            ]
        });

        let registry = default_registry();
        let err = registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &serde_json::to_string(&malformed_args).expect("json"),
                None,
                None,
                None,
            )
            .expect_err("malformed capabilities should yield validation/parse error");
        let err_str = err.to_string();
        assert!(
            err_str.contains("agent.install")
                || err_str.contains("Invalid JSON")
                || err_str.contains("capabilities"),
            "error should mention agent.install or invalid JSON or capabilities: {}",
            err_str
        );
        assert!(
            !agents_dir.join("repaired.worker").exists(),
            "failed install must not leave partial child directory"
        );

        // Repaired payload: add required "hosts" for NetworkAccess
        let repaired_args = serde_json::json!({
            "agent_id": "repaired.worker",
            "instructions": "# Repaired Worker\nMinimal specialist.",
            "promotion_gate": { "evaluator_pass": true, "auditor_pass": true },
            "capabilities": [
                { "type": "NetworkAccess", "hosts": ["example.com"] }
            ]
        });

        let result = registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &serde_json::to_string(&repaired_args).expect("json"),
                None,
                None,
                None,
            )
            .expect("repaired payload should succeed");
        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("result should be json");
        assert_eq!(parsed.get("ok").and_then(|v| v.as_bool()), Some(true));
        assert!(
            agents_dir.join("repaired.worker").join("SKILL.md").exists(),
            "successful install must create child agent with SKILL.md"
        );
    }

    #[test]
    fn test_install_validate_on_install_opt_out() {
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("builder_agent");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        let manifest = test_evolution_manifest(vec![Capability::AgentSpawn { max_children: 10 }]);

        let registry = default_registry();
        let policy = PolicyEngine::new(manifest.clone());

        let args = serde_json::json!({
            "agent_id": "deferred_worker",
            "instructions": "A worker with deferred validation.",
            "background": {
                "enabled": true,
                "interval_secs": 60
            },
            "scheduled_action": {
                "type": "sandbox_exec",
                "command": "exit 1"
            },
            "validate_on_install": false,
            "promotion_gate": {
                "evaluator_pass": true,
                "auditor_pass": true
            }
        });

        let result = registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &args.to_string(),
                None,
                None,
                None,
            )
            .expect("install with validate_on_install=false should succeed");

        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("result should be json");
        assert_eq!(parsed.get("ok").unwrap(), true);
        assert_eq!(parsed.get("status").unwrap(), "agent_installed");

        let reevaluation_path = agents_dir
            .join("deferred_worker")
            .join("state")
            .join("reevaluation.json");
        let reevaluation =
            std::fs::read_to_string(&reevaluation_path).expect("reevaluation state should exist");
        assert!(reevaluation.contains("installed"));
        assert!(
            !reevaluation.contains("install_validation"),
            "validation should not have run"
        );
    }

    #[test]
    fn test_agent_exists_returns_true_for_existing_agent() {
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let caller_dir = agents_dir.join("planner.default");
        std::fs::create_dir_all(&caller_dir).expect("caller dir should create");

        let existing_agent_dir = agents_dir.join("researcher.default");
        std::fs::create_dir_all(existing_agent_dir.join("state")).expect("agent dir should create");
        let skill_md = r#"---
name: "researcher.default"
description: "Research specialist for gathering information"
metadata:
  autonoetic:
    version: "1.0"
    runtime:
      engine: "autonoetic"
      gateway_version: "0.1.0"
      sdk_version: "0.1.0"
      type: "stateful"
      sandbox: "bubblewrap"
      runtime_lock: "runtime.lock"
    agent:
      id: "researcher.default"
      name: "researcher.default"
      description: "Research specialist for gathering information"
    capabilities: []
---
# Researcher
Research agent instructions.
"#;
        std::fs::write(existing_agent_dir.join("SKILL.md"), skill_md)
            .expect("skill.md should write");

        let manifest = test_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let registry = default_registry();

        let args = serde_json::json!({
            "agent_id": "researcher.default"
        });

        let result = registry
            .execute(
                "agent.exists",
                &manifest,
                &policy,
                &caller_dir,
                None,
                &args.to_string(),
                None,
                None,
                None,
            )
            .expect("agent.exists should succeed");

        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("result should be json");
        assert_eq!(parsed.get("ok").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(parsed.get("exists").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(
            parsed.get("agent_id").and_then(|v| v.as_str()),
            Some("researcher.default")
        );
    }

    #[test]
    fn test_agent_exists_returns_false_for_nonexistent_agent() {
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let caller_dir = agents_dir.join("planner.default");
        std::fs::create_dir_all(&caller_dir).expect("caller dir should create");

        let manifest = test_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let registry = default_registry();

        let args = serde_json::json!({
            "agent_id": "nonexistent.agent"
        });

        let result = registry
            .execute(
                "agent.exists",
                &manifest,
                &policy,
                &caller_dir,
                None,
                &args.to_string(),
                None,
                None,
                None,
            )
            .expect("agent.exists should succeed");

        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("result should be json");
        assert_eq!(parsed.get("ok").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(parsed.get("exists").and_then(|v| v.as_bool()), Some(false));
        assert_eq!(
            parsed.get("status").and_then(|v| v.as_str()),
            Some("not_found")
        );
    }

    #[test]
    fn test_agent_exists_reports_identity_mismatch() {
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let caller_dir = agents_dir.join("planner.default");
        std::fs::create_dir_all(&caller_dir).expect("caller dir should create");

        let mismatched_agent_dir = agents_dir.join("dir_name");
        std::fs::create_dir_all(mismatched_agent_dir.join("state"))
            .expect("agent dir should create");
        let skill_md = r#"---
name: "different_id"
description: "Agent with mismatched ID"
metadata:
  autonoetic:
    version: "1.0"
    runtime:
      engine: "autonoetic"
      gateway_version: "0.1.0"
      sdk_version: "0.1.0"
      type: "stateful"
      sandbox: "bubblewrap"
      runtime_lock: "runtime.lock"
    agent:
      id: "different_id"
      name: "different_id"
      description: "Agent with mismatched ID"
    capabilities: []
---
# different_id
Instructions.
"#;
        std::fs::write(mismatched_agent_dir.join("SKILL.md"), skill_md)
            .expect("skill.md should write");

        let manifest = test_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let registry = default_registry();

        let args = serde_json::json!({
            "agent_id": "dir_name"
        });

        let result = registry
            .execute(
                "agent.exists",
                &manifest,
                &policy,
                &caller_dir,
                None,
                &args.to_string(),
                None,
                None,
                None,
            )
            .expect("agent.exists should succeed");

        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("result should be json");
        assert_eq!(parsed.get("ok").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(parsed.get("exists").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(
            parsed.get("status").and_then(|v| v.as_str()),
            Some("identity_mismatch")
        );
        assert!(parsed
            .get("message")
            .map(|m| m.as_str().unwrap().contains("needs to be fixed"))
            .unwrap_or(false));
    }

    #[test]
    fn test_agent_discover_returns_ranked_candidates() {
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let caller_dir = agents_dir.join("planner.default");
        std::fs::create_dir_all(&caller_dir).expect("caller dir should create");

        let create_agent = |id: &str, desc: &str, caps: &[&str]| {
            let agent_dir = agents_dir.join(id);
            std::fs::create_dir_all(agent_dir.join("state")).expect("agent dir should create");
            let caps_json = caps
                .iter()
                .map(|c| match *c {
                    "CodeExecution" => r#"{"type":"CodeExecution","patterns":["*"]}"#.to_string(),
                    "WriteAccess" => r#"{"type":"WriteAccess","scopes":["*"]}"#.to_string(),
                    _ => format!(r#"{{"type":"SandboxFunctions","allowed":["{}"]}}"#, c),
                })
                .collect::<Vec<_>>()
                .join(",");
            let skill_md = format!(
                r#"---
name: "{}"
description: "{}"
metadata:
  autonoetic:
    version: "1.0"
    runtime:
      engine: "autonoetic"
      gateway_version: "0.1.0"
      sdk_version: "0.1.0"
      type: "stateful"
      sandbox: "bubblewrap"
      runtime_lock: "runtime.lock"
    agent:
      id: "{}"
      name: "{}"
      description: "{}"
    capabilities: [{}]
---
# {}
{} agent instructions.
"#,
                id, desc, id, id, desc, caps_json, id, desc
            );
            std::fs::write(agent_dir.join("SKILL.md"), skill_md).expect("skill.md should write");
        };

        create_agent(
            "researcher.default",
            "Web research and information gathering specialist",
            &["SandboxFunctions"],
        );
        create_agent(
            "coder.default",
            "Code generation and software development specialist with CodeExecution",
            &["CodeExecution", "WriteAccess"],
        );
        create_agent(
            "auditor.default",
            "Security audit and compliance specialist",
            &["ReadAccess"],
        );

        let manifest = test_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let registry = default_registry();

        let args = serde_json::json!({
            "intent": "code generation",
            "required_capabilities": ["CodeExecution"],
            "exclude_ids": []
        });

        let result = registry
            .execute(
                "agent.discover",
                &manifest,
                &policy,
                &caller_dir,
                None,
                &args.to_string(),
                None,
                None,
                None,
            )
            .expect("agent.discover should succeed");

        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("result should be json");
        assert_eq!(parsed.get("ok").and_then(|v| v.as_bool()), Some(true));

        let results = parsed
            .get("results")
            .expect("results should exist")
            .as_array()
            .expect("results should be array");
        assert!(
            !results.is_empty(),
            "should find at least one matching agent"
        );

        let first_result = results.get(0).expect("first result should exist");
        assert!(
            first_result
                .get("score")
                .expect("score should exist")
                .as_f64()
                .expect("score should be number")
                > 0.0
        );
        assert_eq!(
            first_result.get("agent_id").and_then(|v| v.as_str()),
            Some("coder.default")
        );
        assert!(
            first_result
                .get("match_reasons")
                .expect("match_reasons should exist")
                .as_array()
                .expect("match_reasons should be array")
                .is_empty()
                == false
        );
    }

    #[test]
    fn test_agent_discover_exclude_ids() {
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let caller_dir = agents_dir.join("planner.default");
        std::fs::create_dir_all(&caller_dir).expect("caller dir should create");

        let create_agent = |id: &str, desc: &str| {
            let agent_dir = agents_dir.join(id);
            std::fs::create_dir_all(agent_dir.join("state")).expect("agent dir should create");
            let skill_md = format!(
                r#"---
name: "{}"
description: "{}"
metadata:
  autonoetic:
    version: "1.0"
    runtime:
      engine: "autonoetic"
      gateway_version: "0.1.0"
      sdk_version: "0.1.0"
      type: "stateful"
      sandbox: "bubblewrap"
      runtime_lock: "runtime.lock"
    agent:
      id: "{}"
      name: "{}"
      description: "{}"
    capabilities: []
---
# {}
{} agent instructions.
"#,
                id, desc, id, id, desc, id, desc
            );
            std::fs::write(agent_dir.join("SKILL.md"), skill_md).expect("skill.md should write");
        };

        create_agent("researcher.default", "Research and web scraping specialist");
        create_agent("coder.default", "Code generation specialist");

        let manifest = test_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let registry = default_registry();

        let args = serde_json::json!({
            "intent": "specialist",
            "required_capabilities": [],
            "exclude_ids": ["researcher.default"]
        });

        let result = registry
            .execute(
                "agent.discover",
                &manifest,
                &policy,
                &caller_dir,
                None,
                &args.to_string(),
                None,
                None,
                None,
            )
            .expect("agent.discover should succeed");

        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("result should be json");
        let results = parsed
            .get("results")
            .expect("results should exist")
            .as_array()
            .expect("results should be array");

        let agent_ids: Vec<&str> = results
            .iter()
            .map(|r| r.get("agent_id").unwrap().as_str().unwrap())
            .collect();
        assert!(
            !agent_ids.contains(&"researcher.default"),
            "excluded agent should not appear in results"
        );
        assert!(
            agent_ids.contains(&"coder.default"),
            "non-excluded agent should appear in results"
        );
    }

    #[test]
    fn test_agent_discover_includes_io_schema_in_results() {
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let caller_dir = agents_dir.join("planner.default");
        std::fs::create_dir_all(&caller_dir).expect("caller dir should create");

        let agent_dir = agents_dir.join("researcher.default");
        std::fs::create_dir_all(agent_dir.join("state")).expect("agent dir should create");
        let skill_md = r#"---
name: "researcher.default"
description: "Research specialist with schema"
metadata:
  autonoetic:
    version: "1.0"
    runtime:
      engine: "autonoetic"
      gateway_version: "0.1.0"
      sdk_version: "0.1.0"
      type: "stateful"
      sandbox: "bubblewrap"
      runtime_lock: "runtime.lock"
    agent:
      id: "researcher.default"
      name: "researcher.default"
      description: "Research specialist with schema"
    capabilities: []
    io:
      accepts:
        type: object
        required: [query]
      returns:
        type: object
        required: [findings]
---
# Researcher
Research agent instructions.
"#;
        std::fs::write(agent_dir.join("SKILL.md"), skill_md).expect("skill.md should write");

        let manifest = test_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let registry = default_registry();

        let args = serde_json::json!({
            "intent": "research",
            "required_capabilities": [],
            "exclude_ids": []
        });
        let result = registry
            .execute(
                "agent.discover",
                &manifest,
                &policy,
                &caller_dir,
                None,
                &args.to_string(),
                None,
                None,
                None,
            )
            .expect("agent.discover should succeed");

        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("result should be json");
        let results = parsed
            .get("results")
            .and_then(|v| v.as_array())
            .expect("results should be an array");
        let first = results.first().expect("one agent should be discovered");
        let io = first.get("io").expect("io should be present");
        assert_eq!(
            io.get("accepts").and_then(|v| v.get("type")),
            Some(&serde_json::json!("object"))
        );
        assert_eq!(
            io.get("returns").and_then(|v| v.get("type")),
            Some(&serde_json::json!("object"))
        );
    }

    #[test]
    fn test_agent_install_script_mode_creates_script_agent() {
        let manifest = test_evolution_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("builder_agent");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        let args = serde_json::json!({
            "agent_id": "weather.script.default",
            "name": "Weather Script Agent",
            "description": "Deterministic weather API agent",
            "instructions": "# Weather Script\nFetch weather data from public API.",
            "execution_mode": "script",
            "script_entry": "scripts/fetch_weather.py",
            "files": [
                {
                    "path": "scripts/fetch_weather.py",
                    "content": "import json\nimport sys\n\ndef main():\n    city = sys.argv[1] if len(sys.argv) > 1 else 'Paris'\n    # In real implementation, this would call open-meteo API\n    result = {'city': city, 'temp': 15.5, 'condition': 'sunny'}\n    print(json.dumps(result))\n\nif __name__ == '__main__':\n    main()\n"
                }
            ],
            "promotion_gate": {
                "evaluator_pass": true,
                "auditor_pass": true
            }
        });

        let registry = default_registry();
        let result = registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
                None,
            )
            .expect("agent install should succeed");

        // Verify install succeeded
        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("result should be json");
        assert_eq!(
            parsed.get("status").and_then(|v| v.as_str()),
            Some("agent_installed")
        );

        // Verify child agent directory was created
        let child_dir = agents_dir.join("weather.script.default");
        assert!(child_dir.join("SKILL.md").exists(), "SKILL.md should exist");
        assert!(
            child_dir.join("runtime.lock").exists(),
            "runtime.lock should exist"
        );
        assert!(
            child_dir.join("scripts").join("fetch_weather.py").exists(),
            "script should exist"
        );

        // Verify SKILL.md contains correct execution_mode and script_entry
        let skill = std::fs::read_to_string(child_dir.join("SKILL.md")).expect("skill should read");
        assert!(
            skill.contains("execution_mode: script"),
            "SKILL.md should contain execution_mode: script"
        );
        assert!(
            skill.contains("script_entry: scripts/fetch_weather.py"),
            "SKILL.md should contain script_entry"
        );
    }

    #[test]
    fn test_agent_install_script_mode_requires_script_entry() {
        let manifest = test_evolution_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("builder_agent");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        // Script mode without script_entry should fail
        let args = serde_json::json!({
            "agent_id": "bad.script.agent",
            "instructions": "# Bad Script Agent\nMissing script_entry.",
            "execution_mode": "script",
            "promotion_gate": {
                "evaluator_pass": true,
                "auditor_pass": true
            }
        });

        let registry = default_registry();
        let result = registry.execute(
            "agent.install",
            &manifest,
            &policy,
            &parent_dir,
            None,
            &serde_json::to_string(&args).expect("json should encode"),
            None,
            None,
            None,
        );

        assert!(result.is_err(), "install should fail without script_entry");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("script_entry"),
            "error should mention script_entry"
        );
    }

    #[test]
    fn test_agent_install_script_mode_allows_no_llm_config() {
        let manifest = test_evolution_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("builder_agent");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        // Script mode should work without llm_config
        let args = serde_json::json!({
            "agent_id": "no.llm.script",
            "instructions": "# Script Agent\nNo LLM needed.",
            "execution_mode": "script",
            "script_entry": "scripts/main.py",
            "files": [
                {
                    "path": "scripts/main.py",
                    "content": "print('hello')"
                }
            ],
            "promotion_gate": {
                "evaluator_pass": true,
                "auditor_pass": true
            }
        });

        let registry = default_registry();
        let result = registry.execute(
            "agent.install",
            &manifest,
            &policy,
            &parent_dir,
            None,
            &serde_json::to_string(&args).expect("json should encode"),
            None,
            None,
            None,
        );

        // Should succeed even without llm_config
        assert!(
            result.is_ok(),
            "script mode should not require llm_config: {:?}",
            result.err()
        );

        // Verify child agent exists
        let child_dir = agents_dir.join("no.llm.script");
        assert!(child_dir.join("SKILL.md").exists());
    }

    #[test]
    fn test_agent_install_reasoning_mode_with_llm_config() {
        let manifest = test_evolution_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("builder_agent");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        let args = serde_json::json!({
            "agent_id": "reasoning.agent",
            "instructions": "# Reasoning Agent\nUses LLM for reasoning.",
            "execution_mode": "reasoning",
            "llm_config": {
                "provider": "openai",
                "model": "gpt-4o"
            },
            "promotion_gate": {
                "evaluator_pass": true,
                "auditor_pass": true
            }
        });

        let registry = default_registry();
        let result = registry.execute(
            "agent.install",
            &manifest,
            &policy,
            &parent_dir,
            None,
            &serde_json::to_string(&args).expect("json should encode"),
            None,
            None,
            None,
        );

        assert!(result.is_ok(), "reasoning mode should succeed");

        // Verify child agent exists with SKILL.md
        let child_dir = agents_dir.join("reasoning.agent");
        let skill = std::fs::read_to_string(child_dir.join("SKILL.md")).expect("skill should read");
        // Note: execution_mode: reasoning is the default and may be omitted from frontmatter
        // We just verify the agent was created successfully and has llm_config
        assert!(
            skill.contains("provider: openai"),
            "SKILL.md should contain llm_config provider"
        );
        assert!(
            skill.contains("model: gpt-4o"),
            "SKILL.md should contain llm_config model"
        );
    }

    #[test]
    fn test_agent_install_blocks_without_approval() {
        // This test verifies that an agent install is blocked when approval is required
        // and returns a proper approval request response.

        let manifest = test_evolution_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("builder");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        // Try to install with approval-required capability (NetworkAccess is high-risk)
        let args = serde_json::json!({
            "agent_id": "pending.worker",
            "instructions": "Worker that requires approval.",
            "capabilities": [
                { "type": "NetworkAccess", "hosts": ["api.example.com"] }
            ],
            "promotion_gate": {
                "evaluator_pass": true,
                "auditor_pass": true,
                "security_analysis": {
                    "passed": true,
                    "threats_detected": [],
                    "remote_access_detected": true
                },
                "capability_analysis": {
                    "inferred_capabilities": ["NetworkAccess"],
                    "missing_capabilities": [],
                    "declared_capabilities": ["NetworkAccess"],
                    "analysis_passed": true
                }
            }
        });

        let config = GatewayConfig {
            agents_dir: agents_dir.clone(),
            agent_install_approval_policy: AgentInstallApprovalPolicy::Always,
            ..Default::default()
        };

        let registry = default_registry();
        let result = registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
                Some(&config),
            )
            .expect("install should return approval request, not error");

        // Verify approval_required response
        let parsed: serde_json::Value = serde_json::from_str(&result).expect("json");
        assert_eq!(parsed.get("ok").and_then(|v| v.as_bool()), Some(false));
        assert_eq!(
            parsed.get("approval_required").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert!(parsed.get("request_id").is_some());
        assert!(
            parsed
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .contains("approval"),
            "message should mention approval"
        );

        // Verify agent was NOT installed
        let child_dir = agents_dir.join("pending.worker");
        assert!(
            !child_dir.exists(),
            "agent should not be installed while approval is pending"
        );
    }

    #[test]
    fn test_agent_install_rejects_invalid_approval_ref() {
        // This test verifies that an agent cannot be installed with an approval_ref
        // that doesn't exist in the approved directory.

        let manifest = test_evolution_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("builder");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        // Try to install with a fake approval_ref that doesn't exist
        let args = serde_json::json!({
            "agent_id": "fake.approval.worker",
            "instructions": "Worker with fake approval.",
            "capabilities": [
                { "type": "NetworkAccess", "hosts": ["api.example.com"] }
            ],
            "promotion_gate": {
                "evaluator_pass": true,
                "auditor_pass": true,
                "security_analysis": {
                    "passed": true,
                    "threats_detected": [],
                    "remote_access_detected": true
                },
                "capability_analysis": {
                    "inferred_capabilities": ["NetworkAccess"],
                    "missing_capabilities": [],
                    "declared_capabilities": ["NetworkAccess"],
                    "analysis_passed": true
                },
                "install_approval_ref": "non-existent-request-id"
            }
        });

        let config = GatewayConfig {
            agents_dir: agents_dir.clone(),
            agent_install_approval_policy: AgentInstallApprovalPolicy::Always,
            ..Default::default()
        };

        let registry = default_registry();
        let result = registry.execute(
            "agent.install",
            &manifest,
            &policy,
            &parent_dir,
            None,
            &serde_json::to_string(&args).expect("json should encode"),
            None,
            None,
            Some(&config),
        );

        // Should fail because the approval_ref doesn't exist
        assert!(
            result.is_err(),
            "install should fail with invalid approval_ref"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("not found")
                || err_msg.contains("not approved")
                || err_msg.contains("approval"),
            "error should mention approval issue: {}",
            err_msg
        );

        // Verify agent was NOT installed
        let child_dir = agents_dir.join("fake.approval.worker");
        assert!(
            !child_dir.exists(),
            "agent should not be installed with invalid approval"
        );
    }

    #[test]
    fn test_agent_install_no_approval_needed_for_low_risk() {
        // This test verifies that low-risk agents (no NetworkAccess, no background)
        // can be installed without approval when using RiskBased policy.

        let manifest = test_evolution_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("builder");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        // Low-risk install: no NetworkAccess, no background
        let args = serde_json::json!({
            "agent_id": "simple.worker",
            "instructions": "A simple worker.",
            "promotion_gate": {
                "evaluator_pass": true,
                "auditor_pass": true,
                "security_analysis": {
                    "passed": true,
                    "threats_detected": [],
                    "remote_access_detected": false
                },
                "capability_analysis": {
                    "inferred_capabilities": [],
                    "missing_capabilities": [],
                    "declared_capabilities": [],
                    "analysis_passed": true
                }
            }
        });

        let config = GatewayConfig {
            agents_dir: agents_dir.clone(),
            agent_install_approval_policy: AgentInstallApprovalPolicy::RiskBased,
            ..Default::default()
        };

        let registry = default_registry();
        let result = registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
                Some(&config),
            )
            .expect("low-risk install should succeed without approval");

        // Verify successful install
        let parsed: serde_json::Value = serde_json::from_str(&result).expect("json");
        assert_eq!(parsed.get("ok").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(
            parsed.get("status").and_then(|v| v.as_str()),
            Some("agent_installed")
        );

        // Verify agent was installed
        let child_dir = agents_dir.join("simple.worker");
        assert!(child_dir.exists(), "agent should be installed");
    }

    #[test]
    fn test_agent_install_stores_payload_on_approval() {
        // This test verifies that when an install requires approval, the payload is stored
        // in a file that can be used for deterministic retry.

        let manifest = test_evolution_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("builder");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        let args = serde_json::json!({
            "agent_id": "stored.payload.worker",
            "instructions": "Worker for payload storage test.",
            "capabilities": [
                { "type": "NetworkAccess", "hosts": ["api.example.com"] }
            ],
            "promotion_gate": {
                "evaluator_pass": true,
                "auditor_pass": true,
                "security_analysis": {
                    "passed": true,
                    "threats_detected": [],
                    "remote_access_detected": true
                },
                "capability_analysis": {
                    "inferred_capabilities": ["NetworkAccess"],
                    "missing_capabilities": [],
                    "declared_capabilities": ["NetworkAccess"],
                    "analysis_passed": true
                }
            }
        });

        let config = GatewayConfig {
            agents_dir: agents_dir.clone(),
            agent_install_approval_policy: AgentInstallApprovalPolicy::Always,
            ..Default::default()
        };

        let registry = default_registry();
        let result = registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
                Some(&config),
            )
            .expect("install should return approval request");

        // Get the request_id
        let parsed: serde_json::Value = serde_json::from_str(&result).expect("json");
        let request_id = parsed
            .get("request_id")
            .and_then(|v| v.as_str())
            .expect("request_id should exist");

        // Verify payload file was created
        let payload_path = agents_dir
            .join(".gateway")
            .join("scheduler")
            .join("approvals")
            .join("pending")
            .join(format!("{}_payload.json", request_id));
        assert!(
            payload_path.exists(),
            "payload file should exist at {:?}",
            payload_path
        );

        // Verify payload content
        let payload_content =
            std::fs::read_to_string(&payload_path).expect("payload should be readable");
        let stored_args: serde_json::Value =
            serde_json::from_str(&payload_content).expect("payload should be valid JSON");
        assert_eq!(
            stored_args.get("agent_id").and_then(|v| v.as_str()),
            Some("stored.payload.worker")
        );
        assert!(stored_args.get("instructions").is_some());
    }

    #[test]
    fn test_agent_install_uses_stored_payload_on_retry() {
        // This test verifies that when retrying with install_approval_ref,
        // the gateway uses the stored payload (ensuring fingerprint match).

        let manifest = test_evolution_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("builder");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        // Original install payload
        let original_args = serde_json::json!({
            "agent_id": "retry.test.worker",
            "instructions": "# Original Instructions\nThis is the original payload.",
            "capabilities": [
                { "type": "NetworkAccess", "hosts": ["api.example.com"] }
            ],
            "promotion_gate": {
                "evaluator_pass": true,
                "auditor_pass": true,
                "security_analysis": {
                    "passed": true,
                    "threats_detected": [],
                    "remote_access_detected": true
                },
                "capability_analysis": {
                    "inferred_capabilities": ["NetworkAccess"],
                    "missing_capabilities": [],
                    "declared_capabilities": ["NetworkAccess"],
                    "analysis_passed": true
                }
            }
        });

        let config = GatewayConfig {
            agents_dir: agents_dir.clone(),
            agent_install_approval_policy: AgentInstallApprovalPolicy::Always,
            ..Default::default()
        };

        let registry = default_registry();

        // First call: creates approval and stores payload
        let result = registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &serde_json::to_string(&original_args).expect("json should encode"),
                None,
                None,
                Some(&config),
            )
            .expect("install should return approval request");

        let parsed: serde_json::Value = serde_json::from_str(&result).expect("json");
        let request_id = parsed.get("request_id").and_then(|v| v.as_str()).unwrap();

        // Manually approve the request (simulating user approval)
        let approved_dir = agents_dir
            .join(".gateway")
            .join("scheduler")
            .join("approvals")
            .join("approved");
        std::fs::create_dir_all(&approved_dir).expect("approved dir should create");

        let approval_decision = serde_json::json!({
            "request_id": request_id,
            "agent_id": "specialized_builder.default",
            "session_id": "test-session",
            "action": {
                "type": "agent_install",
                "agent_id": "retry.test.worker",
                "summary": "# Original Instructions",
                "requested_by_agent_id": "specialized_builder.default",
                "install_fingerprint": "fake_fingerprint"
            },
            "status": "approved",
            "decided_at": "2026-03-13T12:01:00Z",
            "decided_by": "test-user"
        });

        std::fs::write(
            approved_dir.join(format!("{}.json", request_id)),
            serde_json::to_string(&approval_decision).expect("json"),
        )
        .expect("write approval decision");

        // Retry with DIFFERENT payload but same approval_ref
        // The gateway should use the STORED payload, not this one
        let retry_args = serde_json::json!({
            "agent_id": "retry.test.worker",
            "instructions": "# CHANGED Instructions\nThis is a different payload that should be ignored!",
            "capabilities": [
                { "type": "NetworkAccess", "hosts": ["different.api.com"] }
            ],
            "promotion_gate": {
                "evaluator_pass": true,
                "auditor_pass": true,
                "security_analysis": {
                    "passed": true,
                    "threats_detected": [],
                    "remote_access_detected": true
                },
                "capability_analysis": {
                    "inferred_capabilities": ["NetworkAccess"],
                    "missing_capabilities": [],
                    "declared_capabilities": ["NetworkAccess"],
                    "analysis_passed": true
                },
                "install_approval_ref": request_id
            }
        });

        let retry_result = registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &serde_json::to_string(&retry_args).expect("json should encode"),
                None,
                None,
                Some(&config),
            )
            .expect("retry should succeed with stored payload");

        let retry_parsed: serde_json::Value = serde_json::from_str(&retry_result).expect("json");
        assert_eq!(retry_parsed.get("ok").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(
            retry_parsed.get("status").and_then(|v| v.as_str()),
            Some("agent_installed")
        );

        // Verify agent was installed
        let child_dir = agents_dir.join("retry.test.worker");
        assert!(child_dir.exists(), "agent should be installed");

        // Verify SKILL.md contains ORIGINAL instructions, not the changed ones
        let skill =
            std::fs::read_to_string(child_dir.join("SKILL.md")).expect("skill should exist");
        assert!(
            skill.contains("Original Instructions"),
            "should use stored payload, not changed payload"
        );
    }

    #[test]
    fn test_agent_install_cleans_up_payload_after_success() {
        // This test verifies that the stored payload file is cleaned up after successful install.

        let manifest = test_evolution_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let parent_dir = agents_dir.join("builder");
        std::fs::create_dir_all(&parent_dir).expect("parent dir should create");

        let args = serde_json::json!({
            "agent_id": "cleanup.test.worker",
            "instructions": "Worker for cleanup test.",
            "capabilities": [
                { "type": "NetworkAccess", "hosts": ["api.example.com"] }
            ],
            "promotion_gate": {
                "evaluator_pass": true,
                "auditor_pass": true,
                "security_analysis": {
                    "passed": true,
                    "threats_detected": [],
                    "remote_access_detected": true
                },
                "capability_analysis": {
                    "inferred_capabilities": ["NetworkAccess"],
                    "missing_capabilities": [],
                    "declared_capabilities": ["NetworkAccess"],
                    "analysis_passed": true
                }
            }
        });

        let config = GatewayConfig {
            agents_dir: agents_dir.clone(),
            agent_install_approval_policy: AgentInstallApprovalPolicy::Always,
            ..Default::default()
        };

        let registry = default_registry();

        // First call: creates approval and stores payload
        let result = registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &serde_json::to_string(&args).expect("json should encode"),
                None,
                None,
                Some(&config),
            )
            .expect("install should return approval request");

        let parsed: serde_json::Value = serde_json::from_str(&result).expect("json");
        let request_id = parsed.get("request_id").and_then(|v| v.as_str()).unwrap();

        // Verify payload file exists
        let payload_path = agents_dir
            .join(".gateway")
            .join("scheduler")
            .join("approvals")
            .join("pending")
            .join(format!("{}_payload.json", request_id));
        assert!(
            payload_path.exists(),
            "payload file should exist before retry"
        );

        // Manually approve
        let approved_dir = agents_dir
            .join(".gateway")
            .join("scheduler")
            .join("approvals")
            .join("approved");
        std::fs::create_dir_all(&approved_dir).expect("approved dir should create");

        let approval_decision = serde_json::json!({
            "request_id": request_id,
            "agent_id": "specialized_builder.default",
            "session_id": "test-session",
            "action": {
                "type": "agent_install",
                "agent_id": "cleanup.test.worker",
                "summary": "Worker for cleanup test.",
                "requested_by_agent_id": "specialized_builder.default",
                "install_fingerprint": "fake_fingerprint"
            },
            "status": "approved",
            "decided_at": "2026-03-13T12:01:00Z",
            "decided_by": "test-user"
        });

        std::fs::write(
            approved_dir.join(format!("{}.json", request_id)),
            serde_json::to_string(&approval_decision).expect("json"),
        )
        .expect("write approval decision");

        // Retry with approval_ref
        let retry_args = serde_json::json!({
            "agent_id": "cleanup.test.worker",
            "instructions": "Worker for cleanup test.",
            "capabilities": [
                { "type": "NetworkAccess", "hosts": ["api.example.com"] }
            ],
            "promotion_gate": {
                "evaluator_pass": true,
                "auditor_pass": true,
                "security_analysis": {
                    "passed": true,
                    "threats_detected": [],
                    "remote_access_detected": true
                },
                "capability_analysis": {
                    "inferred_capabilities": ["NetworkAccess"],
                    "missing_capabilities": [],
                    "declared_capabilities": ["NetworkAccess"],
                    "analysis_passed": true
                },
                "install_approval_ref": request_id
            }
        });

        registry
            .execute(
                "agent.install",
                &manifest,
                &policy,
                &parent_dir,
                None,
                &serde_json::to_string(&retry_args).expect("json should encode"),
                None,
                None,
                Some(&config),
            )
            .expect("retry should succeed");

        // Verify payload file was cleaned up
        assert!(
            !payload_path.exists(),
            "payload file should be cleaned up after success"
        );
    }
}
