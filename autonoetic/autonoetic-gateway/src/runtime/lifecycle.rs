//! Agent Execution Lifecycle.
//!
//! Manages Wake -> Context Assembly -> Reasoning -> Act -> Hibernate.

use crate::causal_chain::CausalLogger;
use crate::llm::{CompletionRequest, LlmDriver, Message, StopReason, ToolDefinition};
use crate::log_redaction::redact_text_for_logs;
use crate::policy::PolicyEngine;
use crate::runtime::disclosure::DisclosureState;
use crate::runtime::guard::LoopGuard;
use crate::runtime::mcp::McpToolRuntime;
use crate::runtime::store::SecretStoreRuntime;
use autonoetic_types::agent::AgentManifest;
use autonoetic_types::background::{ReevaluationState, ScheduledAction};
use autonoetic_types::causal_chain::EntryStatus;
use autonoetic_types::disclosure::DisclosurePolicy;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const EVIDENCE_MODE_ENV: &str = "AUTONOETIC_EVIDENCE_MODE";

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
    pub registry: crate::runtime::tools::NativeToolRegistry,
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
        registry: crate::runtime::tools::NativeToolRegistry,
    ) -> Self {
        Self {
            manifest,
            instructions,
            llm,
            agent_dir,
            registry,
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
        persist_reevaluation_state(&self.agent_dir, |state| {
            state.last_outcome = Some(reason.to_string());
        })?;
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
            if let Some(evidence_ref) =
                evidence_store.capture_json(None, "session", "start", &session_evidence)?
            {
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
                "history_messages": history.len(),
                "evidence_mode": evidence_store.mode.as_str(),
            })),
            &session_id,
            Some(&turn_id),
            event_seq,
        );

        let mut mcp_runtime = McpToolRuntime::from_env().await?;
        let mut secret_store = SecretStoreRuntime::from_instructions(&self.instructions)?;
        let policy = PolicyEngine::new(self.manifest.clone());
        let mut disclosure_state = DisclosureState::new(
            self.manifest
                .disclosure
                .clone()
                .unwrap_or_else(DisclosurePolicy::default),
        );

        let model = self
            .manifest
            .llm_config
            .as_ref()
            .map(|c| c.model.clone())
            .unwrap_or_else(|| "gpt-4o".to_string());
        let temperature = self
            .manifest
            .llm_config
            .as_ref()
            .map(|c| c.temperature as f32);
        let mut latest_assistant_text: Option<String> = None;
        let agent_id = self.manifest.agent.id.clone();

        loop {
            self.guard.check_loop()?;

            let mut tools: Vec<ToolDefinition> = mcp_runtime.tool_definitions()?;
            tools.extend(self.registry.available_definitions(&self.manifest));

            let req = CompletionRequest {
                model: model.clone(),
                messages: history.clone(),
                tools,
                max_tokens: None,
                temperature,
            };

            tracing::debug!("Calling LLM");
            let response = self.llm.complete(&req).await?;

            let mut llm_payload = serde_json::json!({
                "model": model.clone(),
                "stop_reason": format!("{:?}", response.stop_reason),
                "text": redact_text_for_logs(&truncate_for_log(&response.text, 256)),
                "text_sha256": sha256_hex(&response.text),
                "tool_calls": response.tool_calls.len(),
                "usage": {
                    "input_tokens": response.usage.input_tokens,
                    "output_tokens": response.usage.output_tokens
                }
            });
            let llm_evidence = serde_json::json!({
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
            if let Some(evidence_ref) =
                evidence_store.capture_json(Some(&turn_id), "llm", "completion", &llm_evidence)?
            {
                llm_payload["evidence_ref"] = serde_json::json!(evidence_ref);
            }
            let event_seq = self.next_event_seq();
            log_causal_event(
                &causal_logger,
                &agent_id,
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
                    let mut assistant_msg = Message::assistant(response.text.clone());
                    assistant_msg.tool_calls = response.tool_calls.clone();
                    history.push(assistant_msg);

                    for tc in &response.tool_calls {
                        let redacted_args = redact_text_for_logs(&tc.arguments);
                        let mut requested_payload = serde_json::json!({
                            "tool_name": tc.name,
                            "arguments": redacted_args,
                            "arguments_sha256": sha256_hex(&tc.arguments)
                        });
                        let requested_evidence = serde_json::json!({
                            "tool_name": tc.name,
                            "arguments": redact_text_for_logs(&tc.arguments)
                        });
                        if let Some(evidence_ref) = evidence_store.capture_json(
                            Some(&turn_id),
                            "tool_invoke",
                            "requested",
                            &requested_evidence,
                        )? {
                            requested_payload["evidence_ref"] = serde_json::json!(evidence_ref);
                        }
                        let event_seq = self.next_event_seq();
                        log_causal_event(
                            &causal_logger,
                            &agent_id,
                            "tool_invoke",
                            "requested",
                            EntryStatus::Success,
                            Some(requested_payload),
                            &session_id,
                            Some(&turn_id),
                            event_seq,
                        );

                        // Match logic: MCP first, then Native.
                        let mut result = if mcp_runtime.has_tool(&tc.name) {
                            mcp_runtime.call_tool(&tc.name, &tc.arguments).await?
                        } else if self.registry.has_tool(&tc.name) {
                            self.registry.execute(
                                &tc.name,
                                &self.manifest,
                                &policy,
                                &self.agent_dir,
                                &tc.arguments,
                            )?
                        } else {
                            anyhow::bail!("Unknown tool '{}'", tc.name)
                        };

                        let tc_meta = self.registry.extract_metadata(&tc.name, &tc.arguments);
                        disclosure_state.register_result(
                            &tc.name,
                            tc_meta.path.as_deref(),
                            &result,
                        );

                        if let Some(ref mut store) = secret_store {
                            let (new_result, extracted_secrets) =
                                store.apply_and_redact(&result)?;
                            result = new_result;
                            for s in extracted_secrets {
                                disclosure_state.register_explicit_taint(
                                    &s,
                                    autonoetic_types::disclosure::DisclosureClass::Secret,
                                );
                            }
                        }

                        let mut completed_payload = serde_json::json!({
                            "tool_name": tc.name,
                            "result_len": result.len(),
                            "result_sha256": sha256_hex(&result),
                            "result_preview": redact_text_for_logs(&truncate_for_log(&result, 256))
                        });
                        let completed_evidence = serde_json::json!({
                            "tool_name": tc.name,
                            "result": redact_text_for_logs(&result)
                        });
                        if let Some(evidence_ref) = evidence_store.capture_json(
                            Some(&turn_id),
                            "tool_invoke",
                            "completed",
                            &completed_evidence,
                        )? {
                            completed_payload["evidence_ref"] = serde_json::json!(evidence_ref);
                        }
                        let event_seq = self.next_event_seq();
                        log_causal_event(
                            &causal_logger,
                            &agent_id,
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
                    self.guard.register_progress();
                }
                StopReason::EndTurn | StopReason::StopSequence => {
                    if !response.text.trim().is_empty() {
                        history.push(Message::assistant(response.text.clone()));
                    }
                    let event_seq = self.next_event_seq();
                    log_causal_event(
                        &causal_logger,
                        &self.manifest.agent.id,
                        "lifecycle",
                        "hibernate",
                        EntryStatus::Success,
                        Some(
                            serde_json::json!({ "stop_reason": format!("{:?}", response.stop_reason) }),
                        ),
                        &session_id,
                        Some(&turn_id),
                        event_seq,
                    );
                    break;
                }
                StopReason::MaxTokens | StopReason::Other(_) => {
                    if !response.text.trim().is_empty() {
                        history.push(Message::assistant(response.text.clone()));
                    }
                    let event_seq = self.next_event_seq();
                    log_causal_event(
                        &causal_logger,
                        &self.manifest.agent.id,
                        "lifecycle",
                        "stopped",
                        EntryStatus::Error,
                        Some(
                            serde_json::json!({ "stop_reason": format!("{:?}", response.stop_reason) }),
                        ),
                        &session_id,
                        Some(&turn_id),
                        event_seq,
                    );
                    break;
                }
            }
        }
        Ok(latest_assistant_text.map(|t| disclosure_state.filter_reply(&t)))
    }
}

fn init_causal_logger(agent_dir: &Path) -> anyhow::Result<CausalLogger> {
    let history_dir = agent_dir.join("history");
    std::fs::create_dir_all(&history_dir)?;
    CausalLogger::new(history_dir.join("causal_chain.jsonl"))
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
    if let Err(e) = logger.log(
        actor_id, session_id, turn_id, event_seq, category, action, status, payload,
    ) {
        tracing::warn!(error = %e, category, action, "Failed to append causal log entry");
    }
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

pub fn reevaluation_state_path(agent_dir: &Path) -> PathBuf {
    agent_dir.join("state").join("reevaluation.json")
}

pub fn load_reevaluation_state(agent_dir: &Path) -> anyhow::Result<ReevaluationState> {
    let path = reevaluation_state_path(agent_dir);
    if !path.exists() {
        return Ok(ReevaluationState::default());
    }
    let body = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&body)?)
}

pub fn persist_reevaluation_state<F>(
    agent_dir: &Path,
    mutate: F,
) -> anyhow::Result<ReevaluationState>
where
    F: FnOnce(&mut ReevaluationState),
{
    let mut state = load_reevaluation_state(agent_dir)?;
    mutate(&mut state);
    let path = reevaluation_state_path(agent_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&state)?)?;
    Ok(state)
}

pub fn execute_scheduled_action(
    manifest: &AgentManifest,
    agent_dir: &Path,
    action: &ScheduledAction,
    registry: &crate::runtime::tools::NativeToolRegistry,
) -> anyhow::Result<String> {
    let policy = PolicyEngine::new(manifest.clone());
    match action {
        ScheduledAction::WriteFile { path, content, .. } => {
            anyhow::ensure!(
                !path.trim().is_empty(),
                "scheduled file path must not be empty"
            );
            anyhow::ensure!(
                !path.starts_with('/') && !path.split('/').any(|part| part == ".."),
                "scheduled file path must stay within the agent directory"
            );
            anyhow::ensure!(
                policy.can_write_path(path),
                "scheduled file write denied by MemoryWrite policy"
            );
            let target = agent_dir.join(path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&target, content)?;
            serde_json::to_string(
                &serde_json::json!({ "ok": true, "path": path, "bytes_written": content.len() }),
            )
            .map_err(Into::into)
        }
        ScheduledAction::SandboxExec {
            command,
            dependencies,
            ..
        } => {
            let args = serde_json::to_string(&serde_json::json!({
                "command": command,
                "dependencies": dependencies.as_ref().map(|deps| serde_json::json!({ "runtime": deps.runtime, "packages": deps.packages }))
            }))?;
            registry.execute("sandbox.exec", manifest, &policy, agent_dir, &args)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{
        CompletionRequest, CompletionResponse, LlmDriver, StopReason, TokenUsage, ToolCall,
    };
    use autonoetic_types::agent::{AgentIdentity, RuntimeDeclaration};
    use autonoetic_types::capability::Capability;
    use autonoetic_types::disclosure::{DisclosureClass, DisclosurePolicy};
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
    fn test_execute_scheduled_write_file_action() {
        let manifest = manifest_with_capabilities(vec![Capability::MemoryWrite {
            scopes: vec!["skills/*".to_string()],
        }]);
        let temp = tempdir().expect("tempdir should create");
        let result = execute_scheduled_action(
            &manifest,
            temp.path(),
            &ScheduledAction::WriteFile {
                path: "skills/generated.md".to_string(),
                content: "generated".to_string(),
                requires_approval: false,
                evidence_ref: None,
            },
            &crate::runtime::tools::default_registry(),
        )
        .expect("scheduled write should succeed");
        assert!(result.contains("\"ok\":true"));
    }

    struct FixedTextDriver;
    #[async_trait::async_trait]
    impl LlmDriver for FixedTextDriver {
        async fn complete(
            &self,
            _request: &CompletionRequest,
        ) -> anyhow::Result<CompletionResponse> {
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
            crate::runtime::tools::default_registry(),
        );
        let mut history = vec![Message::system("System prompt"), Message::user("Hello")];
        let reply = runtime
            .execute_with_history(&mut history)
            .await
            .expect("execution should succeed");
        assert_eq!(reply.as_deref(), Some("assistant reply"));
    }

    #[test]
    fn test_native_disclosure_path_extraction() {
        let registry = crate::runtime::tools::default_registry();
        let meta = registry.extract_metadata("memory.read", "{\"path\": \"secrets.txt\"}");
        assert_eq!(meta.path.as_deref(), Some("secrets.txt"));
    }

    #[tokio::test]
    async fn test_unknown_tool_fails_cleanly() {
        let manifest = manifest_with_capabilities(vec![]);
        let temp = tempdir().expect("tempdir should create");
        struct ToolDriver;
        #[async_trait::async_trait]
        impl LlmDriver for ToolDriver {
            async fn complete(
                &self,
                _req: &CompletionRequest,
            ) -> anyhow::Result<CompletionResponse> {
                Ok(CompletionResponse {
                    text: "".to_string(),
                    tool_calls: vec![ToolCall {
                        id: "c1".to_string(),
                        name: "unknown.tool".to_string(),
                        arguments: "{}".to_string(),
                    }],
                    stop_reason: StopReason::ToolUse,
                    usage: TokenUsage::default(),
                })
            }
        }
        let mut runtime = AgentExecutor::new(
            manifest,
            "p".to_string(),
            Arc::new(ToolDriver),
            temp.path().to_path_buf(),
            crate::runtime::tools::default_registry(),
        );
        let mut history = vec![Message::user("go")];
        let res = runtime.execute_with_history(&mut history).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("Unknown tool"));
    }

    #[tokio::test]
    async fn test_disclosure_enforcement_in_executor_loop() {
        let mut manifest = manifest_with_capabilities(vec![Capability::MemoryRead {
            scopes: vec!["*".to_string()],
        }]);
        manifest.disclosure = Some(DisclosurePolicy {
            default_class: DisclosureClass::Public,
            rules: vec![autonoetic_types::disclosure::DisclosureRule {
                source: "memory.read".to_string(),
                path_pattern: Some("secrets.txt".to_string()),
                class: autonoetic_types::disclosure::DisclosureClass::Secret,
            }],
        });

        let temp = tempdir().expect("tempdir should create");
        let state_dir = temp.path().join("state");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(state_dir.join("secrets.txt"), "TOP_SECRET_GOLD").unwrap();

        struct DisclosureDriver;
        #[async_trait::async_trait]
        impl LlmDriver for DisclosureDriver {
            async fn complete(
                &self,
                req: &CompletionRequest,
            ) -> anyhow::Result<CompletionResponse> {
                if req
                    .messages
                    .iter()
                    .any(|m| m.role == crate::llm::Role::Tool)
                {
                    Ok(CompletionResponse {
                        text: "The secret is TOP_SECRET_GOLD".to_string(),
                        tool_calls: vec![],
                        stop_reason: StopReason::EndTurn,
                        usage: TokenUsage::default(),
                    })
                } else {
                    Ok(CompletionResponse {
                        text: "".to_string(),
                        tool_calls: vec![ToolCall {
                            id: "c1".to_string(),
                            name: "memory.read".to_string(),
                            arguments: "{\"path\": \"secrets.txt\"}".to_string(),
                        }],
                        stop_reason: StopReason::ToolUse,
                        usage: TokenUsage::default(),
                    })
                }
            }
        }

        let mut runtime = AgentExecutor::new(
            manifest,
            "p".to_string(),
            Arc::new(DisclosureDriver),
            temp.path().to_path_buf(),
            crate::runtime::tools::default_registry(),
        );
        let mut history = vec![Message::user("tell me the secret")];
        let reply = runtime
            .execute_with_history(&mut history)
            .await
            .expect("exec success");

        assert!(reply.is_some());
        let r = reply.unwrap();
        assert!(
            !r.contains("TOP_SECRET_GOLD"),
            "Secret should have been redacted"
        );
        assert!(r.contains("REDACTED"), "Redaction marker missing");
    }
}
