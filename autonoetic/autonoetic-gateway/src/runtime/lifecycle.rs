//! Agent Execution Lifecycle.
//!
//! Manages Wake -> Context Assembly -> Reasoning -> Act -> Hibernate.

use crate::llm::{CompletionRequest, LlmDriver, Message, StopReason};
use crate::log_redaction::redact_text_for_logs;
use crate::policy::PolicyEngine;
use crate::runtime::guard::LoopGuard;
use crate::runtime::mcp::McpToolRuntime;
use crate::runtime::store::SecretStoreRuntime;
use crate::sandbox::{DependencyPlan, DependencyRuntime, SandboxDriverKind, SandboxRunner};
use autonoetic_types::agent::AgentManifest;
use autonoetic_types::capability::Capability;
use serde::Deserialize;
use std::path::{Path, PathBuf};

const SANDBOX_EXEC_TOOL_NAME: &str = "sandbox.exec";

#[derive(Debug, Deserialize)]
struct SandboxExecArgs {
    command: String,
    #[serde(default)]
    dependencies: Option<SandboxExecDependencies>,
}

#[derive(Debug, Deserialize)]
struct SandboxExecDependencies {
    runtime: String,
    packages: Vec<String>,
}

pub struct AgentExecutor {
    pub manifest: AgentManifest,
    pub instructions: String,
    pub llm: std::sync::Arc<dyn LlmDriver>,
    pub agent_dir: PathBuf,
    pub initial_user_message: String,
    pub guard: LoopGuard,
}

impl AgentExecutor {
    pub fn new(
        manifest: AgentManifest,
        instructions: String,
        llm: std::sync::Arc<dyn LlmDriver>,
        agent_dir: PathBuf,
    ) -> Self {
        Self {
            manifest,
            instructions,
            llm,
            agent_dir,
            initial_user_message: "What is your next action?".to_string(),
            guard: LoopGuard::new(5), // bail after 5 non-progressing cycles
        }
    }

    /// Override the default kickoff user message used for the first turn.
    pub fn with_initial_user_message(mut self, message: impl Into<String>) -> Self {
        self.initial_user_message = message.into();
        self
    }

    /// Run the agent loop until completion or guard trip.
    pub async fn execute_loop(&mut self) -> anyhow::Result<()> {
        let mut history: Vec<Message> = vec![
            Message::system(self.instructions.clone()),
            Message::user(self.initial_user_message.clone()),
        ];
        let _ = self.execute_with_history(&mut history).await?;
        Ok(())
    }

    /// Continue execution from an existing conversation history.
    ///
    /// Returns the latest assistant text produced during this execution cycle.
    pub async fn execute_with_history(
        &mut self,
        history: &mut Vec<Message>,
    ) -> anyhow::Result<Option<String>> {
        tracing::info!("Agent {} waking up...", self.manifest.agent.id);
        self.guard = LoopGuard::new(5);

        let mut mcp_runtime = McpToolRuntime::from_env().await?;
        if !mcp_runtime.is_empty() {
            tracing::info!("Loaded MCP tool runtime for agent {}", self.manifest.agent.id);
        }
        let mut secret_store = SecretStoreRuntime::from_instructions(&self.instructions)?;
        if secret_store.is_some() {
            tracing::info!(
                "Loaded secret store directives for agent {}",
                self.manifest.agent.id
            );
        }
        let policy = PolicyEngine::new(self.manifest.clone());

        let model = self.manifest
            .llm_config.as_ref()
            .map(|c| c.model.clone())
            .unwrap_or_else(|| "gpt-4o".to_string());

        let temperature = self.manifest.llm_config.as_ref().map(|c| c.temperature as f32);
        let mut latest_assistant_text: Option<String> = None;

        loop {
            // --- Loop Guard ---
            self.guard.check_loop()?;

            // --- Context Assembly ---
            tracing::debug!("Assembling context ({} messages)", history.len());

            let mut tools = mcp_runtime.tool_definitions()?;
            if supports_sandbox_exec_tool(&self.manifest) {
                tools.push(sandbox_exec_tool_definition());
            }

            let req = CompletionRequest {
                model: model.clone(),
                messages: history.clone(),
                tools,
                max_tokens: None,
                temperature,
            };

            // --- Reasoning (LLM call) ---
            tracing::debug!("Calling LLM");
            let response = self.llm.complete(&req).await?;

            tracing::debug!(
                stop_reason = ?response.stop_reason,
                text_len = response.text.len(),
                tool_calls = response.tool_calls.len(),
                "LLM response received"
            );
            if !response.text.trim().is_empty() {
                latest_assistant_text = Some(response.text.clone());
            }

            match response.stop_reason {
                StopReason::ToolUse => {
                    // Push the assistant's tool-call turn into history
                    let mut assistant_msg = Message::assistant(response.text.clone());
                    assistant_msg.tool_calls = response.tool_calls.clone();
                    history.push(assistant_msg);

                    // Execute each tool call (stubbed for now)
                    for tc in &response.tool_calls {
                        let redacted_args = redact_text_for_logs(&tc.arguments);
                        tracing::info!(tool = tc.name, args = redacted_args, "Agent requested tool call");
                        let mut result = if mcp_runtime.has_tool(&tc.name) {
                            tracing::debug!(tool = tc.name, "Dispatching tool call to MCP runtime");
                            mcp_runtime.call_tool(&tc.name, &tc.arguments).await?
                        } else if tc.name == SANDBOX_EXEC_TOOL_NAME {
                            tracing::debug!(tool = tc.name, "Dispatching tool call to sandbox runtime");
                            execute_sandbox_tool_call(
                                &self.manifest,
                                &policy,
                                &self.agent_dir,
                                &tc.arguments,
                            )?
                        } else {
                            anyhow::bail!("Unknown tool '{}'", tc.name)
                        };
                        if let Some(ref mut store_runtime) = secret_store {
                            result = store_runtime.apply_and_redact(&result)?;
                        }
                        history.push(Message::tool_result(tc.id.clone(), tc.name.clone(), result));
                    }

                    // Meaningful action taken — reset the guard
                    self.guard.register_progress();
                }
                StopReason::EndTurn | StopReason::StopSequence => {
                    if !response.text.trim().is_empty() {
                        history.push(Message::assistant(response.text.clone()));
                    }
                    tracing::info!("Agent {} task complete, hibernating", self.manifest.agent.id);
                    break;
                }
                StopReason::MaxTokens => {
                    if !response.text.trim().is_empty() {
                        history.push(Message::assistant(response.text.clone()));
                    }
                    tracing::warn!("Agent {} exceeded max tokens", self.manifest.agent.id);
                    break;
                }
                StopReason::Other(ref reason) => {
                    if !response.text.trim().is_empty() {
                        history.push(Message::assistant(response.text.clone()));
                    }
                    tracing::warn!("Agent {} stopped: {}", self.manifest.agent.id, reason);
                    break;
                }
            }
        }

        Ok(latest_assistant_text)
    }
}

fn supports_sandbox_exec_tool(manifest: &AgentManifest) -> bool {
    manifest
        .capabilities
        .iter()
        .any(|cap| matches!(cap, Capability::ShellExec { .. }))
}

fn sandbox_exec_tool_definition() -> crate::llm::ToolDefinition {
    crate::llm::ToolDefinition {
        name: SANDBOX_EXEC_TOOL_NAME.to_string(),
        description: "Execute an approved shell command in the configured sandbox driver".to_string(),
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
    anyhow::ensure!(!packages.is_empty(), "dependency packages must not be empty");
    Ok(DependencyPlan { runtime, packages })
}

fn execute_sandbox_tool_call(
    manifest: &AgentManifest,
    policy: &PolicyEngine,
    agent_dir: &Path,
    arguments_json: &str,
) -> anyhow::Result<String> {
    let args: SandboxExecArgs = serde_json::from_str(arguments_json)
        .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", SANDBOX_EXEC_TOOL_NAME, e))?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{CompletionRequest, CompletionResponse, LlmDriver, StopReason, TokenUsage};
    use autonoetic_types::agent::{AgentIdentity, RuntimeDeclaration};
    use std::sync::Arc;
    use tempfile::tempdir;

    fn manifest_with_capabilities(capabilities: Vec<Capability>) -> AgentManifest {
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
                id: "agent".to_string(),
                name: "Agent".to_string(),
                description: "desc".to_string(),
            },
            capabilities,
            llm_config: None,
            limits: None,
        }
    }

    #[test]
    fn test_supports_sandbox_exec_tool_with_shell_capability() {
        let manifest = manifest_with_capabilities(vec![Capability::ShellExec {
            patterns: vec!["python3 scripts/*".to_string()],
        }]);
        assert!(supports_sandbox_exec_tool(&manifest));
    }

    #[test]
    fn test_supports_sandbox_exec_tool_without_shell_capability() {
        let manifest = manifest_with_capabilities(vec![Capability::ToolInvoke {
            allowed: vec!["a".to_string()],
        }]);
        assert!(!supports_sandbox_exec_tool(&manifest));
    }

    #[test]
    fn test_dependency_plan_from_args_python() {
        let manifest = manifest_with_capabilities(vec![]);
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
        let manifest = manifest_with_capabilities(vec![]);
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
        let manifest = manifest_with_capabilities(vec![]);
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
        let manifest = manifest_with_capabilities(vec![]);
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
        let manifest = manifest_with_capabilities(vec![Capability::ShellExec {
            patterns: vec!["python3 scripts/*".to_string()],
        }]);
        let policy = PolicyEngine::new(manifest.clone());
        let temp = tempdir().expect("tempdir should create");
        let args = serde_json::json!({
            "command": "echo should_fail"
        });
        let err = execute_sandbox_tool_call(
            &manifest,
            &policy,
            temp.path(),
            &serde_json::to_string(&args).expect("json should encode"),
        )
        .expect_err("policy should deny command");
        assert!(err.to_string().contains("sandbox command denied by ShellExec policy"));
    }

    struct FixedTextDriver;

    #[async_trait::async_trait]
    impl LlmDriver for FixedTextDriver {
        async fn complete(&self, _request: &CompletionRequest) -> anyhow::Result<CompletionResponse> {
            Ok(CompletionResponse {
                text: "assistant reply".to_string(),
                tool_calls: vec![],
                stop_reason: StopReason::EndTurn,
                usage: TokenUsage::default(),
            })
        }
    }

    #[tokio::test]
    async fn test_execute_with_history_appends_assistant_text() {
        let manifest = manifest_with_capabilities(vec![]);
        let temp = tempdir().expect("tempdir should create");
        let mut runtime = AgentExecutor::new(
            manifest,
            "System prompt".to_string(),
            Arc::new(FixedTextDriver),
            temp.path().to_path_buf(),
        );
        let mut history = vec![
            Message::system("System prompt"),
            Message::user("Hello"),
        ];
        let reply = runtime
            .execute_with_history(&mut history)
            .await
            .expect("execution should succeed");
        assert_eq!(reply.as_deref(), Some("assistant reply"));
        assert_eq!(
            history.last().map(|m| m.content.as_str()),
            Some("assistant reply")
        );
    }
}
