use crate::llm::ToolDefinition;
use crate::policy::PolicyEngine;
use crate::runtime::reevaluation_state::persist_reevaluation_state;
use crate::sandbox::{DependencyPlan, DependencyRuntime, SandboxDriverKind, SandboxRunner};
use autonoetic_types::agent::AgentManifest;
use autonoetic_types::background::ScheduledAction;
use autonoetic_types::capability::Capability;
use serde::Deserialize;
use std::path::Path;

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
        arguments_json: &str,
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
        arguments_json: &str,
    ) -> anyhow::Result<String> {
        let tool = self
            .tools
            .iter()
            .find(|t| t.name() == name)
            .ok_or_else(|| anyhow::anyhow!("Unknown native tool '{}'", name))?;

        if !tool.is_available(manifest) {
            anyhow::bail!("Native tool '{}' is not available or permitted", name);
        }

        tool.execute(manifest, policy, agent_dir, arguments_json)
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
        arguments_json: &str,
    ) -> anyhow::Result<String> {
        // We delegate to the logic remaining in lifecycle.rs, or move it here.
        // For simplicity, let's keep the core execution in lifecycle.rs and just call it.
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
        arguments_json: &str,
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
        arguments_json: &str,
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
        arguments_json: &str,
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
    registry.register(Box::new(SkillDraftTool));
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
        assert_eq!(defs_all.len(), 4);
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
                &serde_json::to_string(&args).expect("json should encode"),
            )
            .expect_err("policy should deny command");
        assert!(err
            .to_string()
            .contains("sandbox command denied by ShellExec policy"));
    }
}
