//! Agent Execution Lifecycle.
//!
//! Manages Wake -> Context Assembly -> Reasoning -> Act -> Hibernate.

use crate::causal_chain::CausalLogger;
use crate::llm::{CompletionRequest, LlmDriver, Message, StopReason};
use crate::log_redaction::redact_text_for_logs;
use crate::policy::PolicyEngine;
use crate::runtime::guard::LoopGuard;
use crate::runtime::mcp::McpToolRuntime;
use crate::runtime::store::SecretStoreRuntime;
use crate::sandbox::{DependencyPlan, DependencyRuntime, SandboxDriverKind, SandboxRunner};
use autonoetic_types::agent::AgentManifest;
use autonoetic_types::capability::Capability;
use autonoetic_types::causal_chain::EntryStatus;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const SANDBOX_EXEC_TOOL_NAME: &str = "sandbox.exec";
const EVIDENCE_MODE_ENV: &str = "AUTONOETIC_EVIDENCE_MODE";

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EvidenceMode {
    Off,
    Full,
}

impl EvidenceMode {
    fn parse(value: &str) -> anyhow::Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "" | "off" => Ok(Self::Off),
            "full" => Ok(Self::Full),
            other => anyhow::bail!(
                "Invalid {}='{}'. Expected one of: off, full",
                EVIDENCE_MODE_ENV,
                other
            ),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            EvidenceMode::Off => "off",
            EvidenceMode::Full => "full",
        }
    }
}

#[derive(Debug)]
struct EvidenceStore {
    mode: EvidenceMode,
    agent_dir: PathBuf,
    base_dir: Option<PathBuf>,
}

impl EvidenceStore {
    fn from_env(agent_dir: &Path, session_id: &str) -> anyhow::Result<Self> {
        let raw = std::env::var(EVIDENCE_MODE_ENV).unwrap_or_else(|_| "off".to_string());
        let mode = EvidenceMode::parse(&raw)?;
        let base_dir = if mode == EvidenceMode::Full {
            let dir = agent_dir.join("history").join("evidence").join(session_id);
            std::fs::create_dir_all(&dir)?;
            Some(dir)
        } else {
            None
        };
        Ok(Self {
            mode,
            agent_dir: agent_dir.to_path_buf(),
            base_dir,
        })
    }

    fn capture_json(
        &self,
        turn_id: Option<&str>,
        category: &str,
        action: &str,
        payload: &serde_json::Value,
    ) -> anyhow::Result<Option<String>> {
        if self.mode != EvidenceMode::Full {
            return Ok(None);
        }
        let base = self
            .base_dir
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Evidence base directory is not initialized"))?;
        let file_name = format!(
            "{}-{}-{}-{}-{}.json",
            chrono::Utc::now().format("%Y%m%dT%H%M%S%.6fZ"),
            sanitize_token(turn_id.unwrap_or("session")),
            sanitize_token(category),
            sanitize_token(action),
            uuid::Uuid::new_v4()
        );
        let path = base.join(file_name);
        std::fs::write(&path, serde_json::to_string_pretty(payload)?)?;
        let rel = path.strip_prefix(&self.agent_dir).unwrap_or(&path);
        Ok(Some(rel.display().to_string()))
    }
}

pub struct AgentExecutor {
    pub manifest: AgentManifest,
    pub instructions: String,
    pub llm: std::sync::Arc<dyn LlmDriver>,
    pub agent_dir: PathBuf,
    pub initial_user_message: String,
    pub guard: LoopGuard,
    pub session_id: Option<String>,
    pub session_started: bool,
    pub turn_counter: u64,
    pub event_counter: u64,
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
            session_id: None,
            session_started: false,
            turn_counter: 0,
            event_counter: 0,
        }
    }

    /// Override the default kickoff user message used for the first turn.
    pub fn with_initial_user_message(mut self, message: impl Into<String>) -> Self {
        self.initial_user_message = message.into();
        self
    }

    /// Optionally pin a caller-defined session ID for trace correlation.
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    fn ensure_session_id(&mut self) -> String {
        if let Some(id) = &self.session_id {
            return id.clone();
        }
        let id = uuid::Uuid::new_v4().to_string();
        self.session_id = Some(id.clone());
        id
    }

    fn next_event_seq(&mut self) -> u64 {
        self.event_counter += 1;
        self.event_counter
    }

    fn next_turn_id(&mut self) -> String {
        self.turn_counter += 1;
        format!("turn-{:06}", self.turn_counter)
    }

    /// Close the current session and append a final session/end event.
    pub fn close_session(&mut self, reason: &str) -> anyhow::Result<()> {
        if !self.session_started {
            return Ok(());
        }
        let session_id = self.ensure_session_id();
        let causal_logger = init_causal_logger(&self.agent_dir)?;
        let event_seq = self.next_event_seq();
        log_causal_event(
            &causal_logger,
            &self.manifest.agent.id,
            "session",
            "end",
            EntryStatus::Success,
            Some(serde_json::json!({ "reason": reason })),
            &session_id,
            None,
            event_seq,
        );
        self.session_started = false;
        self.session_id = None;
        self.turn_counter = 0;
        self.event_counter = 0;
        Ok(())
    }

    /// Run the agent loop until completion or guard trip.
    pub async fn execute_loop(&mut self) -> anyhow::Result<()> {
        let mut history: Vec<Message> = vec![
            Message::system(self.instructions.clone()),
            Message::user(self.initial_user_message.clone()),
        ];
        match self.execute_with_history(&mut history).await {
            Ok(_) => {
                let _ = self.close_session("execute_loop_complete");
                Ok(())
            }
            Err(e) => {
                let _ = self.close_session("execute_loop_error");
                Err(e)
            }
        }
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
        let session_id = self.ensure_session_id();
        let turn_id = self.next_turn_id();
        let causal_logger = init_causal_logger(&self.agent_dir)?;
        let evidence_store = EvidenceStore::from_env(&self.agent_dir, &session_id)?;
        if !self.session_started {
            let trigger = history
                .iter()
                .rev()
                .find(|m| matches!(m.role, crate::llm::Role::User))
                .map(|m| m.content.clone())
                .unwrap_or_default();
            let mut session_payload = serde_json::json!({
                "trigger_type": "user_input",
                "trigger_len": trigger.len(),
                "trigger_sha256": sha256_hex(&trigger),
                "trigger_preview": redact_text_for_logs(&truncate_for_log(&trigger, 256)),
                "evidence_mode": evidence_store.mode.as_str(),
            });
            let session_evidence = serde_json::json!({
                "trigger": redact_text_for_logs(&trigger)
            });
            if let Some(evidence_ref) = evidence_store.capture_json(None, "session", "start", &session_evidence)? {
                session_payload["evidence_ref"] = serde_json::json!(evidence_ref);
            }
            let event_seq = self.next_event_seq();
            log_causal_event(
                &causal_logger,
                &self.manifest.agent.id,
                "session",
                "start",
                EntryStatus::Success,
                Some(session_payload),
                &session_id,
                None,
                event_seq,
            );
            self.session_started = true;
        }
        let event_seq = self.next_event_seq();
        log_causal_event(
            &causal_logger,
            &self.manifest.agent.id,
            "lifecycle",
            "wake",
            EntryStatus::Success,
            Some(serde_json::json!({
                "turn_id": turn_id.clone(),
                "history_messages": history.len(),
                "evidence_mode": evidence_store.mode.as_str(),
            })),
            &session_id,
            Some(&turn_id),
            event_seq,
        );

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
            let mut llm_payload = serde_json::json!({
                "session_id": session_id.clone(),
                "turn_id": turn_id.clone(),
                "model": model.clone(),
                "stop_reason": format!("{:?}", response.stop_reason),
                "text_len": response.text.len(),
                "text_sha256": sha256_hex(&response.text),
                "text_preview": redact_text_for_logs(&truncate_for_log(&response.text, 256)),
                "tool_calls": response.tool_calls.len(),
                "usage": {
                    "input_tokens": response.usage.input_tokens,
                    "output_tokens": response.usage.output_tokens
                }
            });
            let llm_evidence = serde_json::json!({
                "session_id": session_id.clone(),
                "turn_id": turn_id.clone(),
                "model": model.clone(),
                "stop_reason": format!("{:?}", response.stop_reason),
                "text": redact_text_for_logs(&response.text),
                "tool_calls": response.tool_calls.iter().map(|tc| serde_json::json!({
                    "id": tc.id,
                    "name": tc.name,
                    "arguments": redact_text_for_logs(&tc.arguments)
                })).collect::<Vec<_>>(),
                "usage": {
                    "input_tokens": response.usage.input_tokens,
                    "output_tokens": response.usage.output_tokens
                }
            });
            if let Some(evidence_ref) = evidence_store.capture_json(Some(&turn_id), "llm", "completion", &llm_evidence)? {
                llm_payload["evidence_ref"] = serde_json::json!(evidence_ref);
            }
            let event_seq = self.next_event_seq();
            log_causal_event(
                &causal_logger,
                &self.manifest.agent.id,
                "llm",
                "completion",
                EntryStatus::Success,
                Some(llm_payload),
                &session_id,
                Some(&turn_id),
                event_seq,
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
                        let mut requested_payload = serde_json::json!({
                            "session_id": session_id.clone(),
                            "turn_id": turn_id.clone(),
                            "tool_name": tc.name,
                            "arguments": redacted_args,
                            "arguments_sha256": sha256_hex(&tc.arguments)
                        });
                        let requested_evidence = serde_json::json!({
                            "session_id": session_id.clone(),
                            "turn_id": turn_id.clone(),
                            "tool_name": tc.name,
                            "arguments": redact_text_for_logs(&tc.arguments)
                        });
                        if let Some(evidence_ref) = evidence_store.capture_json(Some(&turn_id), "tool_invoke", "requested", &requested_evidence)? {
                            requested_payload["evidence_ref"] = serde_json::json!(evidence_ref);
                        }
                        let event_seq = self.next_event_seq();
                        log_causal_event(
                            &causal_logger,
                            &self.manifest.agent.id,
                            "tool_invoke",
                            "requested",
                            EntryStatus::Success,
                            Some(requested_payload),
                            &session_id,
                            Some(&turn_id),
                            event_seq,
                        );
                        let result = if mcp_runtime.has_tool(&tc.name) {
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
                        let mut result = result;
                        if let Some(ref mut store_runtime) = secret_store {
                            result = store_runtime.apply_and_redact(&result)?;
                        }
                        let mut completed_payload = serde_json::json!({
                            "session_id": session_id.clone(),
                            "turn_id": turn_id.clone(),
                            "tool_name": tc.name,
                            "result_len": result.len(),
                            "result_sha256": sha256_hex(&result),
                            "result_preview": redact_text_for_logs(&truncate_for_log(&result, 256))
                        });
                        let completed_evidence = serde_json::json!({
                            "session_id": session_id.clone(),
                            "turn_id": turn_id.clone(),
                            "tool_name": tc.name,
                            "result": redact_text_for_logs(&result)
                        });
                        if let Some(evidence_ref) = evidence_store.capture_json(Some(&turn_id), "tool_invoke", "completed", &completed_evidence)? {
                            completed_payload["evidence_ref"] = serde_json::json!(evidence_ref);
                        }
                        let event_seq = self.next_event_seq();
                        log_causal_event(
                            &causal_logger,
                            &self.manifest.agent.id,
                            "tool_invoke",
                            "completed",
                            EntryStatus::Success,
                            Some(completed_payload),
                            &session_id,
                            Some(&turn_id),
                            event_seq,
                        );
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
                    let event_seq = self.next_event_seq();
                    log_causal_event(
                        &causal_logger,
                        &self.manifest.agent.id,
                        "lifecycle",
                        "hibernate",
                        EntryStatus::Success,
                        Some(serde_json::json!({
                            "turn_id": turn_id.clone(),
                            "stop_reason": format!("{:?}", response.stop_reason)
                        })),
                        &session_id,
                        Some(&turn_id),
                        event_seq,
                    );
                    break;
                }
                StopReason::MaxTokens => {
                    if !response.text.trim().is_empty() {
                        history.push(Message::assistant(response.text.clone()));
                    }
                    tracing::warn!("Agent {} exceeded max tokens", self.manifest.agent.id);
                    let event_seq = self.next_event_seq();
                    log_causal_event(
                        &causal_logger,
                        &self.manifest.agent.id,
                        "lifecycle",
                        "stopped",
                        EntryStatus::Error,
                        Some(serde_json::json!({
                            "turn_id": turn_id.clone(),
                            "stop_reason": "MaxTokens"
                        })),
                        &session_id,
                        Some(&turn_id),
                        event_seq,
                    );
                    break;
                }
                StopReason::Other(ref reason) => {
                    if !response.text.trim().is_empty() {
                        history.push(Message::assistant(response.text.clone()));
                    }
                    tracing::warn!("Agent {} stopped: {}", self.manifest.agent.id, reason);
                    let event_seq = self.next_event_seq();
                    log_causal_event(
                        &causal_logger,
                        &self.manifest.agent.id,
                        "lifecycle",
                        "stopped",
                        EntryStatus::Error,
                        Some(serde_json::json!({
                            "turn_id": turn_id.clone(),
                            "stop_reason": reason
                        })),
                        &session_id,
                        Some(&turn_id),
                        event_seq,
                    );
                    break;
                }
            }
        }

        Ok(latest_assistant_text)
    }
}

fn init_causal_logger(agent_dir: &Path) -> anyhow::Result<CausalLogger> {
    let history_dir = agent_dir.join("history");
    std::fs::create_dir_all(&history_dir)?;
    Ok(CausalLogger::new(history_dir.join("causal_chain.jsonl")))
}

fn log_causal_event(
    logger: &CausalLogger,
    actor_id: &str,
    category: &str,
    action: &str,
    status: EntryStatus,
    payload: Option<serde_json::Value>,
    session_id: &str,
    turn_id: Option<&str>,
    event_seq: u64,
) {
    let payload = enrich_payload(payload, session_id, turn_id, event_seq);
    if let Err(e) = logger.log(actor_id, category, action, status, payload) {
        tracing::warn!(error = %e, category, action, "Failed to append causal log entry");
    }
}

fn enrich_payload(
    payload: Option<serde_json::Value>,
    session_id: &str,
    turn_id: Option<&str>,
    event_seq: u64,
) -> Option<serde_json::Value> {
    let mut obj = match payload.unwrap_or_else(|| serde_json::json!({})) {
        serde_json::Value::Object(map) => map,
        other => {
            let mut map = serde_json::Map::new();
            map.insert("data".to_string(), other);
            map
        }
    };
    obj.insert(
        "session_id".to_string(),
        serde_json::Value::String(session_id.to_string()),
    );
    if let Some(turn_id) = turn_id {
        obj.insert(
            "turn_id".to_string(),
            serde_json::Value::String(turn_id.to_string()),
        );
    }
    obj.insert(
        "event_seq".to_string(),
        serde_json::Value::Number(serde_json::Number::from(event_seq)),
    );
    Some(serde_json::Value::Object(obj))
}

fn truncate_for_log(value: &str, max_len: usize) -> String {
    if value.chars().count() <= max_len {
        return value.to_string();
    }
    let truncated: String = value.chars().take(max_len).collect();
    format!("{}...", truncated)
}

fn sanitize_token(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn sha256_hex(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    let digest = hasher.finalize();
    format!("{:x}", digest)
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
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

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

    #[tokio::test]
    async fn test_execute_with_history_writes_causal_chain_file() {
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
        runtime
            .execute_with_history(&mut history)
            .await
            .expect("execution should succeed");
        let causal_path = temp.path().join("history").join("causal_chain.jsonl");
        let body = std::fs::read_to_string(causal_path).expect("causal chain should be written");
        assert!(body.contains("\"category\":\"lifecycle\""));
        assert!(body.contains("\"action\":\"wake\""));
        assert!(body.contains("\"action\":\"hibernate\""));
    }

    #[tokio::test]
    async fn test_execute_with_history_writes_evidence_when_enabled() {
        let _guard = ENV_LOCK.lock().expect("env lock should acquire");
        let old = std::env::var(EVIDENCE_MODE_ENV).ok();
        std::env::set_var(EVIDENCE_MODE_ENV, "full");

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
        runtime
            .execute_with_history(&mut history)
            .await
            .expect("execution should succeed");

        if let Some(v) = old {
            std::env::set_var(EVIDENCE_MODE_ENV, v);
        } else {
            std::env::remove_var(EVIDENCE_MODE_ENV);
        }

        let causal_path = temp.path().join("history").join("causal_chain.jsonl");
        let body = std::fs::read_to_string(causal_path).expect("causal chain should be written");
        assert!(
            body.contains("\"evidence_ref\":\"history/evidence/"),
            "expected evidence_ref pointer in causal entries"
        );

        let evidence_root = temp.path().join("history").join("evidence");
        let mut run_dirs = std::fs::read_dir(&evidence_root)
            .expect("evidence dir should exist")
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        assert_eq!(run_dirs.len(), 1, "expected one evidence run directory");
        let files = std::fs::read_dir(run_dirs.pop().expect("run dir should exist").path())
            .expect("run evidence dir should be readable")
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        assert!(!files.is_empty(), "evidence files should be written");
    }
}
