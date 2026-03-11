//! Agent Execution Lifecycle.
//!
//! Manages Wake -> Context Assembly -> Reasoning -> Act -> Hibernate.

use crate::llm::{CompletionRequest, LlmDriver, Message, StopReason, ToolDefinition};
use crate::policy::PolicyEngine;
use crate::runtime::artifact::extract_artifacts_from_text;
use crate::runtime::disclosure::DisclosureState;
use crate::runtime::guard::LoopGuard;
use crate::runtime::mcp::McpToolRuntime;
use crate::runtime::reevaluation_state::persist_reevaluation_state;
use crate::runtime::session_tracer::{EvidenceMode, SessionTracer};
use crate::runtime::store::SecretStoreRuntime;
use crate::runtime::tool_call_processor::ToolCallProcessor;
use autonoetic_types::agent::AgentManifest;
use autonoetic_types::config::GatewayConfig;
use autonoetic_types::disclosure::DisclosurePolicy;
use std::path::PathBuf;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Foundation Instructions
// ---------------------------------------------------------------------------

const FOUNDATION_INSTRUCTIONS: &str = include_str!("foundation_instructions.md");

pub(crate) fn compose_system_instructions(agent_instructions: &str) -> String {
    let trimmed = agent_instructions.trim();
    if trimmed.is_empty() {
        FOUNDATION_INSTRUCTIONS.trim().to_string()
    } else {
        format!(
            "{}\n\n---\n\nAgent-Specific Instructions\n\n{}",
            FOUNDATION_INSTRUCTIONS.trim(),
            trimmed
        )
    }
}

pub struct AgentExecutor {
    pub manifest: AgentManifest,
    pub instructions: String,
    pub llm: std::sync::Arc<dyn LlmDriver>,
    pub agent_dir: PathBuf,
    pub gateway_dir: Option<PathBuf>,
    pub registry: crate::runtime::tools::NativeToolRegistry,
    pub initial_user_message: String,
    pub guard: LoopGuard,
    pub session_id: Option<String>,
    pub session_started: bool,
    pub turn_counter: u64,
    /// When set, passed to tool execution (e.g. agent.install approval policy).
    pub config: Option<Arc<GatewayConfig>>,
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
            gateway_dir: None,
            registry,
            initial_user_message: "What is your next action?".to_string(),
            guard: LoopGuard::new(5),
            session_id: None,
            session_started: false,
            turn_counter: 0,
            config: None,
        }
    }

    pub fn with_gateway_dir(mut self, gateway_dir: PathBuf) -> Self {
        self.gateway_dir = Some(gateway_dir);
        self
    }

    pub fn with_config(mut self, config: Arc<GatewayConfig>) -> Self {
        self.config = Some(config);
        self
    }

    pub fn with_initial_user_message(mut self, message: impl Into<String>) -> Self {
        self.initial_user_message = message.into();
        self
    }

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

    fn next_turn_id(&mut self) -> String {
        self.turn_counter += 1;
        format!("turn-{:06}", self.turn_counter)
    }

    pub fn close_session(&mut self, reason: &str) -> anyhow::Result<()> {
        if !self.session_started {
            return Ok(());
        }
        let session_id = self.ensure_session_id();
        persist_reevaluation_state(&self.agent_dir, |state| {
            state.last_outcome = Some(reason.to_string());
        })?;
        let mut tracer = SessionTracer::new(&self.agent_dir, &self.manifest.agent.id, &session_id)?;
        tracer.log_session_end(reason);
        self.session_started = false;
        self.session_id = None;
        self.turn_counter = 0;
        Ok(())
    }

    /// Run the agent loop until completion or guard trip.
    pub async fn execute_loop(&mut self) -> anyhow::Result<()> {
        let system_instructions = compose_system_instructions(&self.instructions);
        let mut history: Vec<Message> = vec![
            Message::system(system_instructions),
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

        let evidence_mode = EvidenceMode::parse(
            &std::env::var("AUTONOETIC_EVIDENCE_MODE").unwrap_or_else(|_| "off".to_string()),
        )?;

        let mut tracer = SessionTracer::new(&self.agent_dir, &self.manifest.agent.id, &session_id)?
            .with_turn_id(&turn_id);

        if !self.session_started {
            let trigger = history
                .iter()
                .rev()
                .find(|m| matches!(m.role, crate::llm::Role::User))
                .map(|m| m.content.clone())
                .unwrap_or_default();
            tracer.log_session_start("user_input", &trigger, evidence_mode)?;
            self.session_started = true;
        }

        tracer.log_wake(history.len(), evidence_mode);

        let mut mcp_runtime = McpToolRuntime::from_env().await?;
        let mut secret_store: Option<SecretStoreRuntime> =
            SecretStoreRuntime::from_instructions(&self.instructions)?;
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
        let policy = PolicyEngine::new(self.manifest.clone());

        loop {
            self.guard.check_loop()?;

            // Update system message
            let system_instructions = compose_system_instructions(&self.instructions);

            if let Some(first) = history.get_mut(0) {
                if matches!(first.role, crate::llm::Role::System) {
                    first.content = system_instructions;
                } else {
                    history.insert(0, Message::system(system_instructions));
                }
            } else {
                history.push(Message::system(system_instructions));
            }

            // MCP tool exposure ... (skipped for brevity, but I must match exactly)

            // MCP tool exposure is capability-gated by ToolInvoke allow-lists.
            let mut tools: Vec<ToolDefinition> = mcp_runtime
                .tool_definitions()?
                .into_iter()
                .filter(|def| policy.can_invoke_tool(&def.name))
                .collect();
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

            // Extract new artifacts from response for logging
            let new_artifacts = extract_artifacts_from_text(&response.text);
            for artifact in &new_artifacts {
                tracer.log_artifact_detected(artifact)?;
            }

            let tool_call_details: Vec<serde_json::Value> = response
                .tool_calls
                .iter()
                .map(|tc| {
                    serde_json::json!({
                        "id": tc.id,
                        "name": tc.name,
                        "arguments": crate::log_redaction::redact_text_for_logs(&tc.arguments)
                    })
                })
                .collect();

            tracer.log_llm_completion(
                &model,
                &format!("{:?}", response.stop_reason),
                &response.text,
                response.tool_calls.len(),
                response.usage.input_tokens,
                response.usage.output_tokens,
                &tool_call_details,
            )?;

            if !response.text.trim().is_empty() {
                latest_assistant_text = Some(response.text.clone());
            }

            match response.stop_reason {
                StopReason::ToolUse => {
                    let mut assistant_msg = Message::assistant(response.text.clone());
                    assistant_msg.tool_calls = response.tool_calls.clone();
                    history.push(assistant_msg);

                    let mut processor = ToolCallProcessor::new(
                        &mut mcp_runtime,
                        &self.registry,
                        &self.manifest,
                        &mut disclosure_state,
                        secret_store.as_mut(),
                        self.config.as_deref(),
                    )
                    .with_session_context(self.session_id.clone(), Some(turn_id.clone()));

                    let (had_any_success, results) = processor
                        .process_tool_calls(
                            &response.tool_calls,
                            &self.agent_dir,
                            self.gateway_dir.as_deref(),
                            &mut tracer,
                        )
                        .await?;

                    for (id, name, result) in results {
                        history.push(Message::tool_result(id.clone(), name, result.clone()));

                        // Check if result is an error to register in guard
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&result) {
                            if parsed.get("ok") == Some(&serde_json::Value::Bool(false)) {
                                // Find matching tool call for arguments
                                if let Some(tc) = response.tool_calls.iter().find(|tc| tc.id == id)
                                {
                                    self.guard.register_failure(&tc.name, &tc.arguments);
                                }
                            }
                        }
                    }

                    if had_any_success {
                        self.guard.register_progress();
                    }
                }
                StopReason::EndTurn | StopReason::StopSequence => {
                    if !response.text.trim().is_empty() {
                        history.push(Message::assistant(response.text.clone()));
                    }
                    tracer.log_hibernate(&format!("{:?}", response.stop_reason));
                    break;
                }
                StopReason::MaxTokens | StopReason::Other(_) => {
                    if !response.text.trim().is_empty() {
                        history.push(Message::assistant(response.text.clone()));
                    }
                    tracer.log_stopped(&format!("{:?}", response.stop_reason));
                    break;
                }
            }
        }

        Ok(latest_assistant_text.map(|t| disclosure_state.filter_reply(&t)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{
        CompletionRequest, CompletionResponse, LlmDriver, StopReason, TokenUsage, ToolCall,
    };
    use crate::runtime::reevaluation_state::execute_scheduled_action;
    use autonoetic_types::agent::{AgentIdentity, RuntimeDeclaration};
    use autonoetic_types::background::ScheduledAction;
    use autonoetic_types::capability::Capability;
    use autonoetic_types::disclosure::{DisclosureClass, DisclosurePolicy};
    use std::sync::Arc;
    use std::sync::Mutex;
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
            None,
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

    struct CaptureSystemDriver {
        system_message: Arc<Mutex<Option<String>>>,
    }

    #[async_trait::async_trait]
    impl LlmDriver for CaptureSystemDriver {
        async fn complete(
            &self,
            request: &CompletionRequest,
        ) -> anyhow::Result<CompletionResponse> {
            let system = request
                .messages
                .iter()
                .find(|m| m.role == crate::llm::Role::System)
                .map(|m| m.content.clone());
            *self.system_message.lock().expect("mutex should lock") = system;
            Ok(CompletionResponse {
                text: "ok".to_string(),
                tool_calls: vec![],
                stop_reason: StopReason::EndTurn,
                usage: TokenUsage::default(),
            })
        }
    }

    #[tokio::test]
    async fn test_execute_loop_includes_foundation_in_system_prompt() {
        let manifest = manifest_with_capabilities(vec![]);
        let temp = tempdir().expect("tempdir should create");
        let captured = Arc::new(Mutex::new(None));
        let driver = CaptureSystemDriver {
            system_message: Arc::clone(&captured),
        };
        let mut runtime = AgentExecutor::new(
            manifest,
            "Agent local rules".to_string(),
            Arc::new(driver),
            temp.path().to_path_buf(),
            crate::runtime::tools::default_registry(),
        );

        runtime
            .execute_loop()
            .await
            .expect("execution should succeed");

        let system = captured
            .lock()
            .expect("mutex should lock")
            .clone()
            .expect("system message should be captured");
        assert!(system.contains("Autonoetic Gateway Foundation Rules"));
        assert!(system.contains("Python sandbox code can import `autonoetic_sdk`"));
        assert!(system.contains("Agent local rules"));
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
        let err = res.unwrap_err().to_string();
        assert!(
            err.contains("LoopGuard tripped"),
            "expected loop-guard failure for repeated unknown tool calls, got: {}",
            err
        );
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
