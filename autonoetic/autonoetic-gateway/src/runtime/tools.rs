use crate::llm::ToolDefinition;
use crate::policy::PolicyEngine;
use crate::runtime::reevaluation_state::persist_reevaluation_state;
use crate::sandbox::{DependencyPlan, DependencyRuntime, SandboxDriverKind, SandboxRunner};
use autonoetic_types::agent::{AgentIdentity, AgentManifest, LlmConfig};
use autonoetic_types::background::{
    BackgroundMode, BackgroundPolicy, BackgroundState, ScheduledAction,
};
use autonoetic_types::capability::Capability;
use autonoetic_types::runtime_lock::{
    LockedDependencySet, LockedGateway, LockedSandbox, LockedSdk, RuntimeLock,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Metadata extracted from a tool call for disclosure, audit, and logging.
#[derive(Debug, Default)]
pub struct ToolMetadata {
    pub path: Option<String>,
}

/// Extracts the `path` field from tool arguments JSON.
/// Shared helper for file-backed native tools (memory.read, memory.write, skill.draft).
fn extract_path_from_args(arguments_json: &str) -> ToolMetadata {
    let mut meta = ToolMetadata::default();
    if let Ok(parsed_args) = serde_json::from_str::<serde_json::Value>(arguments_json) {
        if let Some(path) = parsed_args.get("path").and_then(|v| v.as_str()) {
            meta.path = Some(path.to_string());
        }
    }
    meta
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
        agent_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'),
        "agent_id may only contain ASCII letters, digits, '-' and '_'"
    );
    Ok(())
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

fn capabilities_are_empty(capabilities: &&[Capability]) -> bool {
    capabilities.is_empty()
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

    /// Executes the tool call.
    fn execute(
        &self,
        manifest: &AgentManifest,
        policy: &PolicyEngine,
        agent_dir: &Path,
        gateway_dir: Option<&Path>,
        arguments_json: &str,
        session_id: Option<&str>,
        turn_id: Option<&str>,
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

impl NativeTool for SandboxExecTool {
    fn name(&self) -> &'static str {
        "sandbox.exec"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::ShellExec { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Execute an approved shell command in the configured sandbox driver"
                .to_string(),
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
        _gateway_dir: Option<&Path>,
        arguments_json: &str,
        _session_id: Option<&str>,
        _turn_id: Option<&str>,
    ) -> anyhow::Result<String> {
        let args: SandboxExecArgs = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;

        anyhow::ensure!(
            !args.command.trim().is_empty(),
            "sandbox command must not be empty"
        );
        anyhow::ensure!(
            policy.can_exec_shell(&args.command),
            "sandbox command denied by ShellExec policy"
        );

        let dep_plan = dependency_plan_from_args_or_lock(manifest, agent_dir, args.dependencies)?;
        let driver = SandboxDriverKind::parse(&manifest.runtime.sandbox)?;
        let agent_dir_str = agent_dir
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Agent directory is not valid UTF-8"))?;

        let runner = SandboxRunner::spawn_with_driver_and_dependencies(
            driver,
            agent_dir_str,
            &args.command,
            dep_plan.as_ref(),
        )?;
        let output = runner.process.wait_with_output()?;
        let body = serde_json::json!({
            "ok": output.status.success(),
            "exit_code": output.status.code(),
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr)
        });
        serde_json::to_string(&body).map_err(Into::into)
    }
}

// ---------------------------------------------------------------------------
// Memory Read File Tool
// ---------------------------------------------------------------------------

pub struct MemoryReadTool;

impl NativeTool for MemoryReadTool {
    fn name(&self) -> &'static str {
        "memory.read"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::MemoryRead { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Read the contents of a file from the agent's memory state. If the file does not exist and a default_value is provided, the default_value will be returned instead of an error.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "default_value": { "type": "string" }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
        }
    }

    fn execute(
        &self,
        _manifest: &AgentManifest,
        policy: &PolicyEngine,
        agent_dir: &Path,
        _gateway_dir: Option<&Path>,
        arguments_json: &str,
        _session_id: Option<&str>,
        _turn_id: Option<&str>,
    ) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            path: String,
            default_value: Option<String>,
        }
        let args: Args = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;

        anyhow::ensure!(!args.path.trim().is_empty(), "path must not be empty");
        anyhow::ensure!(
            policy.can_read_path(&args.path),
            "memory read denied by policy"
        );

        let mem = crate::runtime::memory::Tier1Memory::new(agent_dir)?;
        match mem.read_file(&args.path) {
            Ok(content) => Ok(content),
            Err(e) => {
                if let Some(default) = args.default_value {
                    Ok(default)
                } else {
                    Err(e)
                }
            }
        }
    }

    fn extract_metadata(&self, arguments_json: &str) -> ToolMetadata {
        extract_path_from_args(arguments_json)
    }
}

// ---------------------------------------------------------------------------
// Memory Write File Tool
// ---------------------------------------------------------------------------

pub struct MemoryWriteTool;

impl NativeTool for MemoryWriteTool {
    fn name(&self) -> &'static str {
        "memory.write"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::MemoryWrite { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Write content to a file in the agent's memory state".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"],
                "additionalProperties": false
            }),
        }
    }

    fn execute(
        &self,
        _manifest: &AgentManifest,
        policy: &PolicyEngine,
        agent_dir: &Path,
        _gateway_dir: Option<&Path>,
        arguments_json: &str,
        _session_id: Option<&str>,
        _turn_id: Option<&str>,
    ) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            path: String,
            content: String,
        }
        let args: Args = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;

        anyhow::ensure!(!args.path.trim().is_empty(), "path must not be empty");
        anyhow::ensure!(
            policy.can_write_path(&args.path),
            "memory write denied by policy"
        );

        let mem = crate::runtime::memory::Tier1Memory::new(agent_dir)?;
        mem.write_file(&args.path, &args.content)?;
        serde_json::to_string(&serde_json::json!({
            "ok": true,
            "bytes_written": args.content.len(),
        }))
        .map_err(Into::into)
    }

    fn extract_metadata(&self, arguments_json: &str) -> ToolMetadata {
        extract_path_from_args(arguments_json)
    }
}

// ---------------------------------------------------------------------------
// Tier 2 Memory Remember Tool (Gateway-managed long-term memory)
// ---------------------------------------------------------------------------

pub struct MemoryRememberTool;

impl NativeTool for MemoryRememberTool {
    fn name(&self) -> &'static str {
        "memory.remember"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::MemoryWrite { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Store a fact in long-term memory with full provenance tracking. The memory will be stored in the gateway-managed Tier 2 memory substrate and can be shared across agents with proper authorization.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Unique identifier for this memory" },
                    "scope": { "type": "string", "description": "Scope/namespace for organizing memory (e.g., 'facts', 'preferences', 'context')" },
                    "content": { "type": "string", "description": "The fact or information to remember" },
                    "confidence": { "type": "number", "description": "Confidence score (0.0-1.0) for the fact's reliability" },
                    "tags": { "type": "array", "items": { "type": "string" }, "description": "Tags for categorization" }
                },
                "required": ["id", "scope", "content"],
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
        session_id: Option<&str>,
        turn_id: Option<&str>,
    ) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            id: String,
            scope: String,
            content: String,
            #[serde(default)]
            confidence: Option<f64>,
            #[serde(default)]
            tags: Vec<String>,
        }
        let args: Args = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;

        anyhow::ensure!(!args.id.trim().is_empty(), "id must not be empty");
        anyhow::ensure!(!args.scope.trim().is_empty(), "scope must not be empty");
        anyhow::ensure!(!args.content.trim().is_empty(), "content must not be empty");

        // Enforce scope-level policy check
        anyhow::ensure!(
            policy.can_write_memory_scope(&args.scope),
            "Cannot write to scope '{}': not in MemoryWrite.scopes capability",
            args.scope
        );

        let Some(gw_dir) = gateway_dir else {
            anyhow::bail!("Tier 2 memory requires gateway directory to be configured");
        };

        // Build source_ref from session/turn context for proper traceability
        let source_ref = if let Some(sid) = session_id {
            if let Some(tid) = turn_id {
                format!("session:{}:turn:{}", sid, tid)
            } else {
                format!("session:{}", sid)
            }
        } else {
            format!("agent:{}:direct", manifest.agent.id)
        };

        let mem = crate::runtime::memory::Tier2Memory::new(gw_dir, &manifest.agent.id)?;
        let mut memory = mem.remember(
            &args.id,
            &args.scope,
            &manifest.agent.id,
            &source_ref,
            &args.content,
        )?;

        // Apply confidence and tags if provided
        if let Some(conf) = args.confidence {
            memory.confidence = Some(conf);
        }
        if !args.tags.is_empty() {
            memory.tags = args.tags;
        }

        // Persist the updated memory with confidence and tags
        mem.save_memory(&memory)?;

        serde_json::to_string(&serde_json::json!({
            "ok": true,
            "memory_id": memory.memory_id,
            "scope": memory.scope,
            "created_at": memory.created_at,
            "content_hash": memory.content_hash,
            "confidence": memory.confidence,
            "tags": memory.tags,
        }))
        .map_err(Into::into)
    }
}

// ---------------------------------------------------------------------------
// Tier 2 Memory Recall Tool
// ---------------------------------------------------------------------------

pub struct MemoryRecallTool;

impl NativeTool for MemoryRecallTool {
    fn name(&self) -> &'static str {
        "memory.recall"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::MemoryRead { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Recall a fact from long-term memory by its ID. Access is controlled by visibility and ACL rules.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "The unique identifier of the memory to recall" }
                },
                "required": ["id"],
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
    ) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            id: String,
        }
        let args: Args = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;

        anyhow::ensure!(!args.id.trim().is_empty(), "id must not be empty");

        let Some(gw_dir) = gateway_dir else {
            anyhow::bail!("Tier 2 memory requires gateway directory to be configured");
        };

        let mem = crate::runtime::memory::Tier2Memory::new(gw_dir, &manifest.agent.id)?;
        let memory = mem.recall(&args.id)?;

        // Enforce scope-level policy check on the recalled memory
        anyhow::ensure!(
            policy.can_read_memory_scope(&memory.scope),
            "Cannot read from scope '{}': not in MemoryRead.scopes capability",
            memory.scope
        );

        serde_json::to_string(&serde_json::json!({
            "ok": true,
            "memory_id": memory.memory_id,
            "scope": memory.scope,
            "content": memory.content,
            "owner_agent_id": memory.owner_agent_id,
            "writer_agent_id": memory.writer_agent_id,
            "source_ref": memory.source_ref,
            "visibility": serde_json::to_value(&memory.visibility)?,
            "created_at": memory.created_at,
            "updated_at": memory.updated_at,
        }))
        .map_err(Into::into)
    }
}

// ---------------------------------------------------------------------------
// Tier 2 Memory Search Tool
// ---------------------------------------------------------------------------

pub struct MemorySearchTool;

impl NativeTool for MemorySearchTool {
    fn name(&self) -> &'static str {
        "memory.search"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::MemorySearch { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Search long-term memory by scope and optional query. Returns memories visible to this agent.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "scope": { "type": "string", "description": "Scope to search within" },
                    "query": { "type": "string", "description": "Optional search query (substring match)" }
                },
                "required": ["scope"],
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
    ) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            scope: String,
            #[serde(default)]
            query: Option<String>,
        }
        let args: Args = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;

        anyhow::ensure!(!args.scope.trim().is_empty(), "scope must not be empty");

        // Enforce scope-level policy check
        anyhow::ensure!(
            policy.can_search_memory(&args.scope),
            "Cannot search scope '{}': not in MemorySearch.scopes capability",
            args.scope
        );

        let Some(gw_dir) = gateway_dir else {
            anyhow::bail!("Tier 2 memory requires gateway directory to be configured");
        };

        let mem = crate::runtime::memory::Tier2Memory::new(gw_dir, &manifest.agent.id)?;
        let results = mem.search(&args.scope, args.query.as_deref())?;

        serde_json::to_string(&serde_json::json!({
            "ok": true,
            "count": results.len(),
            "memories": results.iter().map(|m| serde_json::json!({
                "memory_id": m.memory_id,
                "scope": m.scope,
                "content": m.content,
                "owner_agent_id": m.owner_agent_id,
                "visibility": serde_json::to_value(&m.visibility).unwrap_or_default(),
            })).collect::<Vec<_>>()
        }))
        .map_err(Into::into)
    }
}

// ---------------------------------------------------------------------------
// Tier 2 Memory Share Tool
// ---------------------------------------------------------------------------

pub struct MemoryShareTool;

impl NativeTool for MemoryShareTool {
    fn name(&self) -> &'static str {
        "memory.share"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::MemoryShare { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Share a memory record with specific agents. Requires ownership or write access to the memory.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "The memory ID to share" },
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
                "Cannot share memory with agent '{}': not in allowed_targets",
                target
            );
        }

        let Some(gw_dir) = gateway_dir else {
            anyhow::bail!("Tier 2 memory requires gateway directory to be configured");
        };

        let mem = crate::runtime::memory::Tier2Memory::new(gw_dir, &manifest.agent.id)?;
        let memory = mem.share_with(&args.id, args.with_agents.clone())?;

        serde_json::to_string(&serde_json::json!({
            "ok": true,
            "memory_id": memory.memory_id,
            "visibility": "shared",
            "allowed_agents": memory.allowed_agents,
        }))
        .map_err(Into::into)
    }
}

// ---------------------------------------------------------------------------
// Skill Draft Tool
// ---------------------------------------------------------------------------

pub struct SkillDraftTool;

impl NativeTool for SkillDraftTool {
    fn name(&self) -> &'static str {
        "skill.draft"
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        // Relies on MemoryWrite capability as well
        manifest
            .capabilities
            .iter()
            .any(|cap| matches!(cap, Capability::MemoryWrite { .. }))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Draft a new skill by proposing its SKILL.md content. Drafting a skill requires human approval before it is loaded. The path must be in the skills/ directory (e.g., skills/my_skill.md).".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" },
                    "evidence_ref": { "type": "string" }
                },
                "required": ["path", "content"],
                "additionalProperties": false
            }),
        }
    }

    fn execute(
        &self,
        _manifest: &AgentManifest,
        policy: &PolicyEngine,
        agent_dir: &Path,
        _gateway_dir: Option<&Path>,
        arguments_json: &str,
        _session_id: Option<&str>,
        _turn_id: Option<&str>,
    ) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            path: String,
            content: String,
            #[serde(default)]
            evidence_ref: Option<String>,
        }
        let args: Args = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;

        anyhow::ensure!(!args.path.trim().is_empty(), "path must not be empty");
        anyhow::ensure!(
            args.path.starts_with("skills/"),
            "skill path must begin with skills/"
        );
        anyhow::ensure!(
            policy.can_write_path(&args.path),
            "skill draft write denied by policy"
        );

        persist_reevaluation_state(agent_dir, |state| {
            state.pending_scheduled_action = Some(ScheduledAction::WriteFile {
                path: args.path.clone(),
                content: args.content.clone(),
                requires_approval: true,
                evidence_ref: args.evidence_ref,
            });
        })?;

        serde_json::to_string(&serde_json::json!({
            "ok": true,
            "status": "Skill drafted and queued for approval",
            "path": args.path,
            "bytes_proposed": args.content.len(),
        }))
        .map_err(Into::into)
    }

    fn extract_metadata(&self, arguments_json: &str) -> ToolMetadata {
        extract_path_from_args(arguments_json)
    }
}

// ---------------------------------------------------------------------------
// Agent Install Tool
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct InstallAgentFile {
    path: String,
    content: String,
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

#[derive(Debug, Deserialize)]
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
    #[serde(default = "default_true")]
    arm_immediately: bool,
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
        })),
        (None, None) => Ok(None),
    }
}

pub struct AgentInstallTool;

impl NativeTool for AgentInstallTool {
    fn name(&self) -> &'static str {
        "agent.install"
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
            description: "Install a specialized child agent by writing its SKILL.md, runtime.lock, files, and optional background schedule. Use this when the agent should create a durable worker or specialist that continues operating after the current chat turn.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "instructions": { "type": "string" },
                    "llm_config": { "type": "object" },
                    "capabilities": {
                        "type": "array",
                        "items": { "type": "object" }
                    },
                    "background": { "type": "object" },
                    "scheduled_action": { "type": "object" },
                    "files": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "path": { "type": "string" },
                                "content": { "type": "string" }
                            },
                            "required": ["path", "content"],
                            "additionalProperties": false
                        }
                    },
                    "runtime_lock_dependencies": {
                        "type": "array",
                        "items": { "type": "object" }
                    },
                    "arm_immediately": { "type": "boolean" }
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
        _session_id: Option<&str>,
        _turn_id: Option<&str>,
    ) -> anyhow::Result<String> {
        let args: InstallAgentArgs = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;
        let scheduled_action = parse_install_scheduled_action(args.scheduled_action.clone())?;
        let background =
            normalize_install_background(args.background.clone(), &args.scheduled_action)?;

        validate_agent_id(&args.agent_id)?;
        anyhow::ensure!(
            !args.instructions.trim().is_empty(),
            "instructions must not be empty"
        );

        let agents_dir = agent_dir
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Agent directory is missing its agents root parent"))?;
        let child_dir = agents_dir.join(&args.agent_id);
        anyhow::ensure!(
            !child_dir.exists(),
            "child agent '{}' already exists",
            args.agent_id
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
                        matches!(cap, Capability::ShellExec { patterns } if patterns.iter().any(|pattern| pattern == command))
                    }) {
                        capabilities.push(Capability::ShellExec {
                            patterns: vec![command.clone()],
                        });
                    }
                }
                ScheduledAction::WriteFile { path, .. } => {
                    if !capabilities.iter().any(|cap| {
                        matches!(cap, Capability::MemoryWrite { scopes } if scopes.iter().any(|scope| path.starts_with(scope.trim_end_matches('*'))))
                    }) {
                        capabilities.push(Capability::MemoryWrite {
                            scopes: vec![path.clone()],
                        });
                    }
                }
            }
        }

        let llm_config = args
            .llm_config
            .clone()
            .or_else(|| manifest.llm_config.clone());
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
        };

        std::fs::create_dir_all(child_dir.join("state"))?;
        std::fs::create_dir_all(child_dir.join("history"))?;
        std::fs::create_dir_all(child_dir.join("skills"))?;
        std::fs::create_dir_all(child_dir.join("scripts"))?;

        let instruction_body = ensure_output_contract_section(
            &args.instructions,
            &args.files,
            scheduled_action.is_some(),
        );
        let skill_yaml = render_skill_frontmatter(&child_manifest)?;
        let skill_body = format!("---\n{}---\n{}\n", skill_yaml, instruction_body);
        std::fs::write(child_dir.join("SKILL.md"), skill_body)?;

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
            child_dir.join(&child_manifest.runtime.runtime_lock),
            serde_yaml::to_string(&runtime_lock)?,
        )?;

        for file in &args.files {
            validate_relative_agent_path(&file.path)?;
            anyhow::ensure!(
                file.path != "SKILL.md" && file.path != child_manifest.runtime.runtime_lock,
                "files may not overwrite generated SKILL.md or runtime.lock"
            );
            let target = child_dir.join(&file.path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(target, &file.content)?;
        }

        if let Some(action) = scheduled_action.clone() {
            persist_reevaluation_state(&child_dir, |state| {
                state.pending_scheduled_action = Some(action);
                state.last_outcome = Some("installed".to_string());
                state.retry_not_before = None;
            })?;
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

/// Builds the default registry with the core native tools.

#[derive(Debug, Deserialize)]
pub(crate) struct SandboxExecArgs {
    command: String,
    #[serde(default)]
    dependencies: Option<SandboxExecDependencies>,
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
    anyhow::ensure!(
        !packages.is_empty(),
        "dependency packages must not be empty"
    );
    Ok(DependencyPlan { runtime, packages })
}

pub fn default_registry() -> NativeToolRegistry {
    let mut registry = NativeToolRegistry::new();
    registry.register(Box::new(SandboxExecTool));
    registry.register(Box::new(MemoryReadTool));
    registry.register(Box::new(MemoryWriteTool));
    registry.register(Box::new(MemoryRememberTool));
    registry.register(Box::new(MemoryRecallTool));
    registry.register(Box::new(MemorySearchTool));
    registry.register(Box::new(MemoryShareTool));
    registry.register(Box::new(SkillDraftTool));
    registry.register(Box::new(AgentInstallTool));
    registry
}

#[cfg(test)]
mod tests {
    use super::*;
    use autonoetic_types::agent::{AgentIdentity, RuntimeDeclaration};
    use autonoetic_types::capability::Capability;
    use tempfile::tempdir;

    fn test_manifest(capabilities: Vec<Capability>) -> AgentManifest {
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
                id: "test-agent".to_string(),
                name: "test-agent".to_string(),
                description: "test".to_string(),
            },
            capabilities,
            llm_config: None,
            limits: None,
            background: None,
            disclosure: None,
        }
    }

    #[test]
    fn test_native_tool_registry_availability() {
        let registry = default_registry();
        let manifest_none = test_manifest(vec![]);
        assert_eq!(registry.available_definitions(&manifest_none).len(), 0);

        let manifest_shell = test_manifest(vec![Capability::ShellExec {
            patterns: vec!["*".into()],
        }]);
        let defs = registry.available_definitions(&manifest_shell);
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "sandbox.exec");

        let manifest_all = test_manifest(vec![
            Capability::ShellExec { patterns: vec![] },
            Capability::MemoryRead { scopes: vec![] },
            Capability::MemoryWrite { scopes: vec![] },
        ]);
        let defs_all = registry.available_definitions(&manifest_all);
        // sandbox.exec, memory.read, memory.write, memory.remember, memory.recall, skill.draft = 6
        assert_eq!(defs_all.len(), 6);

        let manifest_spawn = test_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
        let defs_spawn = registry.available_definitions(&manifest_spawn);
        assert_eq!(defs_spawn.len(), 1);
        assert_eq!(defs_spawn[0].name, "agent.install");
    }

    #[test]
    fn test_agent_install_tool_creates_background_child_agent() {
        let manifest = test_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
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
            "files": [
                {
                    "path": "scripts/fibonacci_worker.py",
                    "content": "import json\nfrom pathlib import Path\nstate_path = Path('state/fib.json')\nstate = json.loads(state_path.read_text())\nstate['previous'], state['current'] = state['current'], state['previous'] + state['current']\nstate['index'] += 1\nstate_path.write_text(json.dumps(state))\n"
                },
                {
                    "path": "state/fib.json",
                    "content": "{\"previous\": 0, \"current\": 1, \"index\": 1}"
                }
            ]
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
        assert!(skill.contains("type: ShellExec"));
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
    fn test_agent_install_tool_accepts_scheduled_action_shorthand() {
        let manifest = test_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
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
        let manifest = test_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
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
        let manifest = test_manifest(vec![Capability::AgentSpawn { max_children: 4 }]);
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
        let manifest = test_manifest(vec![Capability::ShellExec {
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
            )
            .expect_err("policy should deny command");
        assert!(err
            .to_string()
            .contains("sandbox command denied by ShellExec policy"));
    }
}
