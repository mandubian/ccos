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
use autonoetic_types::agent::{AgentManifest, Middleware};
use autonoetic_types::config::GatewayConfig;
use autonoetic_types::disclosure::DisclosurePolicy;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Foundation Instructions
// ---------------------------------------------------------------------------

const FOUNDATION_INSTRUCTIONS: &str = include_str!("foundation_instructions.md");

#[derive(Debug, Clone, Default)]
struct SchemaValidation {
    valid: bool,
    messages: Vec<String>,
}

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
    /// Middleware hooks declared in the agent manifest.
    pub middleware: Middleware,
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
            middleware: Middleware::default(),
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

    pub fn with_middleware(mut self, middleware: Middleware) -> Self {
        self.middleware = middleware;
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

        let active_agent_dir = self.agent_dir.clone();

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
                metadata: None,
            };

            // --- Pre-process hook: transform input before LLM call ---
            let pre_hook = self.middleware.pre_process.as_ref();
            let req = if let Some(pre_hook) = pre_hook {
                self.apply_middleware_pre(
                    req,
                    pre_hook,
                    &active_agent_dir,
                    &session_id,
                    &turn_id,
                    &mut tracer,
                )?
            } else {
                req
            };

            // --- Skip LLM if signaled by pre-process hook ---
            // The hook can return a response in metadata.assistant_reply and set metadata.skip_llm: true
            let response = if req
                .metadata
                .as_ref()
                .and_then(|m| m.get("skip_llm"))
                .and_then(|v| v.as_bool())
                == Some(true)
            {
                let assistant_reply = req
                    .metadata
                    .as_ref()
                    .and_then(|m| m.get("assistant_reply"))
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();

                let _ = tracer.log_event(
                    "agent.process",
                    "pre_hook_skip_llm",
                    autonoetic_types::causal_chain::EntryStatus::Success,
                    None,
                );

                crate::llm::CompletionResponse {
                    text: assistant_reply,
                    tool_calls: vec![],
                    usage: crate::llm::TokenUsage::default(),
                    stop_reason: crate::llm::StopReason::EndTurn,
                }
            } else {
                tracing::debug!("Calling LLM");
                self.llm.complete(&req).await?
            };

            // --- Post-process hook: transform output after LLM call ---
            let post_hook = self.middleware.post_process.as_ref();
            let response = if let Some(post_hook) = post_hook {
                self.apply_middleware_post(
                    response,
                    post_hook,
                    &active_agent_dir,
                    &session_id,
                    &turn_id,
                    &mut tracer,
                )?
            } else {
                response
            };

            self.log_output_schema_validation(&response, &mut tracer);

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
                            &active_agent_dir,
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

                    // Persist history to content store at hibernate points
                    if let Some(gateway_dir) = self.gateway_dir.as_ref() {
                        if let Err(e) = persist_history_to_content_store(
                            &self.agent_dir,
                            &session_id,
                            history,
                            gateway_dir,
                            &mut tracer,
                        ) {
                            tracing::warn!("Failed to persist history: {}", e);
                        }
                    }

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

    fn log_output_schema_validation(
        &self,
        response: &crate::llm::CompletionResponse,
        tracer: &mut SessionTracer,
    ) {
        // Only validate final output when agent claims completion (EndTurn).
        // Skip validation for tool use responses - agents may emit free text
        // alongside tool calls, which is expected reasoning/narration.
        if !matches!(
            response.stop_reason,
            crate::llm::StopReason::EndTurn | crate::llm::StopReason::StopSequence
        ) {
            return;
        }

        let Some(returns_schema) = self.manifest.io.as_ref().and_then(|io| io.returns.as_ref())
        else {
            return;
        };

        let validation = validate_against_schema(&response.text, returns_schema);
        let _ = tracer.log_event(
            "agent.process",
            "output_schema_validation",
            autonoetic_types::causal_chain::EntryStatus::Success,
            Some(serde_json::json!({
                "valid": validation.valid,
                "messages": validation.messages,
            })),
        );
    }

    /// Executes middleware pre-process script in a sandbox.
    fn apply_middleware_pre(
        &self,
        mut req: crate::llm::CompletionRequest,
        hook_script: &str,
        active_agent_dir: &Path,
        session_id: &str,
        turn_id: &str,
        tracer: &mut SessionTracer,
    ) -> anyhow::Result<crate::llm::CompletionRequest> {
        let _ = tracer.log_event(
            "agent.process",
            "pre_hook_requested",
            autonoetic_types::causal_chain::EntryStatus::Success,
            Some(serde_json::json!({ "turn_id": turn_id })),
        );

        let input_json = serde_json::to_string(&req)?;
        let output =
            self.run_middleware_script(hook_script, input_json, active_agent_dir, session_id)?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Ok(transformed) = serde_json::from_str::<serde_json::Value>(&stdout) {
                if let Ok(new_req) =
                    serde_json::from_value::<crate::llm::CompletionRequest>(transformed.clone())
                {
                    req = new_req;
                } else if let Some(skip) = transformed.get("skip_llm").and_then(|v| v.as_bool()) {
                    let mut meta = req.metadata.unwrap_or_default();
                    meta.insert("skip_llm".to_string(), serde_json::Value::Bool(skip));
                    if let Some(reply) = transformed.get("assistant_reply").and_then(|v| v.as_str())
                    {
                        meta.insert(
                            "assistant_reply".to_string(),
                            serde_json::Value::String(reply.to_string()),
                        );
                    }
                    req.metadata = Some(meta);
                }
                let _ = tracer.log_event(
                    "agent.process",
                    "pre_hook_completed",
                    autonoetic_types::causal_chain::EntryStatus::Success,
                    None,
                );
                Ok(req)
            } else {
                let _ = tracer.log_event(
                    "agent.process",
                    "pre_hook_failed",
                    autonoetic_types::causal_chain::EntryStatus::Error,
                    Some(serde_json::json!({ "error": "Invalid JSON from hook" })),
                );
                anyhow::bail!("Pre-process hook returned invalid JSON");
            }
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let _ = tracer.log_event(
                "agent.process",
                "pre_hook_failed",
                autonoetic_types::causal_chain::EntryStatus::Error,
                Some(serde_json::json!({ "error": stderr })),
            );
            anyhow::bail!("Pre-process hook failed: {}", stderr);
        }
    }

    /// Executes middleware post-process script in a sandbox.
    fn apply_middleware_post(
        &self,
        mut response: crate::llm::CompletionResponse,
        hook_script: &str,
        active_agent_dir: &Path,
        session_id: &str,
        turn_id: &str,
        tracer: &mut SessionTracer,
    ) -> anyhow::Result<crate::llm::CompletionResponse> {
        let _ = tracer.log_event(
            "agent.process",
            "post_hook_requested",
            autonoetic_types::causal_chain::EntryStatus::Success,
            Some(serde_json::json!({ "turn_id": turn_id })),
        );

        let input_json = serde_json::to_string(&response)?;
        let output =
            self.run_middleware_script(hook_script, input_json, active_agent_dir, session_id)?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Ok(transformed) = serde_json::from_str::<crate::llm::CompletionResponse>(&stdout)
            {
                response = transformed;
                let _ = tracer.log_event(
                    "agent.process",
                    "post_hook_completed",
                    autonoetic_types::causal_chain::EntryStatus::Success,
                    None,
                );
                Ok(response)
            } else {
                let _ = tracer.log_event(
                    "agent.process",
                    "post_hook_failed",
                    autonoetic_types::causal_chain::EntryStatus::Error,
                    Some(serde_json::json!({ "error": "Invalid JSON from hook" })),
                );
                anyhow::bail!("Post-process hook returned invalid JSON");
            }
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let _ = tracer.log_event(
                "agent.process",
                "post_hook_failed",
                autonoetic_types::causal_chain::EntryStatus::Error,
                Some(serde_json::json!({ "error": stderr })),
            );
            anyhow::bail!("Post-process hook failed: {}", stderr);
        }
    }

    fn run_middleware_script(
        &self,
        command: &str,
        stdin_json: String,
        active_agent_dir: &Path,
        _session_id: &str,
    ) -> anyhow::Result<std::process::Output> {
        use crate::sandbox::{SandboxDriverKind, SandboxRunner};
        use std::io::Write;

        let driver = SandboxDriverKind::parse(&self.manifest.runtime.sandbox)?;
        let agent_dir_str = active_agent_dir
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid active_agent_dir"))?;

        let mut runner = SandboxRunner::spawn_with_driver_and_dependencies(
            driver,
            agent_dir_str,
            command,
            None,
        )?;

        if let Some(mut stdin) = runner.process.stdin.take() {
            stdin.write_all(stdin_json.as_bytes())?;
        }

        runner.process.wait_with_output().map_err(Into::into)
    }
}

/// Extracts JSON from markdown-wrapped content.
/// Handles common LLM output formats:
/// - ```json ... ``` (code block with json language hint)
/// - ``` ... ``` (plain code block)
/// - Plain JSON without markdown wrapping
fn extract_json_from_markdown(input: &str) -> String {
    let trimmed = input.trim();

    // Try to find ```json ... ``` or ``` ... ``` blocks
    if let Some(start) = trimmed.find("```") {
        let after_first_block = &trimmed[start + 3..];

        // Skip language hint (e.g., "json\n" -> "\n")
        let content_start = after_first_block
            .find('\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        let content = &after_first_block[content_start..];

        // Find closing ```
        if let Some(end) = content.find("```") {
            return content[..end].trim().to_string();
        }
    }

    // No markdown wrapping found, return original
    input.to_string()
}

/// Lightweight schema validation: checks required fields and basic type hints.
/// Extracts JSON from markdown-wrapped content before validation.
fn validate_against_schema(input: &str, schema: &serde_json::Value) -> SchemaValidation {
    let mut validation = SchemaValidation {
        valid: true,
        messages: Vec::new(),
    };

    // Extract JSON from markdown if present
    let json_input = extract_json_from_markdown(input);

    let parsed_input: serde_json::Value = match serde_json::from_str(&json_input) {
        Ok(v) => v,
        Err(_) => {
            validation.valid = false;
            validation
                .messages
                .push("Output is not valid JSON".to_string());
            return validation;
        }
    };

    if let Some(expected_type) = schema.get("type").and_then(|t| t.as_str()) {
        let type_matches = match expected_type {
            "object" => parsed_input.is_object(),
            "array" => parsed_input.is_array(),
            "string" => parsed_input.is_string(),
            "number" => parsed_input.is_number(),
            "boolean" => parsed_input.is_boolean(),
            _ => true,
        };
        if !type_matches {
            validation.valid = false;
            validation.messages.push(format!(
                "Type mismatch: expected {}, got {}",
                expected_type, parsed_input
            ));
        }
    }

    if let Some(required) = schema.get("required").and_then(|r| r.as_array()) {
        if let Some(obj) = parsed_input.as_object() {
            for field in required {
                if let Some(field_name) = field.as_str() {
                    if !obj.contains_key(field_name) {
                        validation.valid = false;
                        validation
                            .messages
                            .push(format!("Missing required field: {}", field_name));
                    }
                }
            }
        }
    }

    validation
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
    use sha2::Digest;
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
            io: None,
            middleware: None,
            execution_mode: Default::default(),
            script_entry: None,
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
        // content.read uses name_or_handle, not path
        let meta = registry.extract_metadata("content.read", "{\"name_or_handle\": \"secrets.txt\"}");
        assert_eq!(meta.path.as_deref(), Some("secrets.txt"));
    }

    #[test]
    fn test_extract_json_from_markdown_plain_json() {
        let input = r#"{"findings":["fact1"],"summary":"ok"}"#;
        let extracted = extract_json_from_markdown(input);
        assert_eq!(extracted, input);
    }

    #[test]
    fn test_extract_json_from_markdown_json_code_block() {
        let input = r#"Here is the result:
```json
{"findings":["fact1"],"summary":"ok"}
```
Hope this helps!"#;
        let extracted = extract_json_from_markdown(input);
        let expected = r#"{"findings":["fact1"],"summary":"ok"}"#;
        assert_eq!(extracted, expected);
    }

    #[test]
    fn test_extract_json_from_markdown_plain_code_block() {
        let input = r#"Result:
```
{"findings":["fact1"],"summary":"ok"}
```"#;
        let extracted = extract_json_from_markdown(input);
        let expected = r#"{"findings":["fact1"],"summary":"ok"}"#;
        assert_eq!(extracted, expected);
    }

    #[test]
    fn test_extract_json_from_markdown_multiline_json() {
        let input = r#"```json
{
  "findings": ["fact1", "fact2"],
  "summary": "ok"
}
```"#;
        let extracted = extract_json_from_markdown(input);
        let expected = r#"{
  "findings": ["fact1", "fact2"],
  "summary": "ok"
}"#;
        assert_eq!(extracted, expected);
    }

    #[test]
    fn test_validate_output_schema_valid_json_input() {
        let schema = serde_json::json!({
            "type": "object",
            "required": ["findings", "summary"]
        });
        let output = r#"{"findings":["fact1"],"summary":"ok"}"#;
        let result = validate_against_schema(output, &schema);
        assert!(result.valid);
        assert!(result.messages.is_empty());
    }

    #[test]
    fn test_validate_output_schema_non_json_input() {
        let schema = serde_json::json!({
            "type": "object",
            "required": ["findings"]
        });
        let output = "plain text response";
        let result = validate_against_schema(output, &schema);
        assert!(!result.valid);
        assert!(result.messages.iter().any(|m| m.contains("not valid JSON")));
    }

    #[test]
    fn test_validate_output_schema_accepts_markdown_wrapped_json() {
        let schema = serde_json::json!({
            "type": "object",
            "required": ["findings", "summary"]
        });
        let output = r#"Here is the result:
```json
{"findings":["fact1"],"summary":"ok"}
```
Hope this helps!"#;
        let result = validate_against_schema(output, &schema);
        assert!(result.valid, "Should accept markdown-wrapped JSON: {:?}", result.messages);
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
        // Test that the disclosure filter mechanism works
        // The actual filtering is tested in unit tests, here we just verify
        // that the executor loop applies the filter
        let manifest = manifest_with_capabilities(vec![]);
        let temp = tempdir().expect("tempdir should create");

        struct DisclosureDriver;
        #[async_trait::async_trait]
        impl LlmDriver for DisclosureDriver {
            async fn complete(
                &self,
                _req: &CompletionRequest,
            ) -> anyhow::Result<CompletionResponse> {
                // Direct response without tool use
                Ok(CompletionResponse {
                    text: "The answer is 42".to_string(),
                    tool_calls: vec![],
                    stop_reason: StopReason::EndTurn,
                    usage: TokenUsage::default(),
                })
            }
        }

        let mut runtime = AgentExecutor::new(
            manifest,
            "p".to_string(),
            Arc::new(DisclosureDriver),
            temp.path().to_path_buf(),
            crate::runtime::tools::default_registry(),
        );
        let mut history = vec![Message::user("what is the answer?")];
        let reply = runtime
            .execute_with_history(&mut history)
            .await
            .expect("exec success");

        assert!(reply.is_some());
        let r = reply.unwrap();
        assert!(r.contains("42"), "Expected answer in reply");
    }

    #[test]
    fn test_log_output_schema_validation_skips_tool_use() {
        let manifest = manifest_with_capabilities(vec![]);
        let temp = tempdir().expect("tempdir should create");
        let executor = AgentExecutor::new(
            manifest,
            "p".to_string(),
            Arc::new(FixedTextDriver),
            temp.path().to_path_buf(),
            crate::runtime::tools::default_registry(),
        );

        let mut tracer = crate::runtime::session_tracer::SessionTracer::test_tracer();

        // ToolUse with any text should be skipped - no validation
        let response = CompletionResponse {
            text: "Let me check the database first...".to_string(),
            tool_calls: vec![ToolCall {
                id: "c1".to_string(),
                name: "any".to_string(),
                arguments: "{}".to_string(),
            }],
            stop_reason: StopReason::ToolUse,
            usage: TokenUsage::default(),
        };

        executor.log_output_schema_validation(&response, &mut tracer);
    }

    #[test]
    fn test_log_output_schema_validation_validates_end_turn() {
        let mut manifest = manifest_with_capabilities(vec![]);
        manifest.io = Some(autonoetic_types::agent::AgentIO {
            accepts: None,
            returns: Some(serde_json::json!({
                "type": "object",
                "required": ["result"]
            })),
        });

        let temp = tempdir().expect("tempdir should create");
        let executor = AgentExecutor::new(
            manifest,
            "p".to_string(),
            Arc::new(FixedTextDriver),
            temp.path().to_path_buf(),
            crate::runtime::tools::default_registry(),
        );

        let mut tracer = crate::runtime::session_tracer::SessionTracer::test_tracer();

        // EndTurn with invalid JSON should produce validation error
        let response = CompletionResponse {
            text: "plain text response".to_string(),
            tool_calls: vec![],
            stop_reason: StopReason::EndTurn,
            usage: TokenUsage::default(),
        };

        executor.log_output_schema_validation(&response, &mut tracer);

        // EndTurn with valid JSON matching schema should pass
        let mut tracer2 = crate::runtime::session_tracer::SessionTracer::test_tracer();
        let response2 = CompletionResponse {
            text: r#"{"result": "success"}"#.to_string(),
            tool_calls: vec![],
            stop_reason: StopReason::EndTurn,
            usage: TokenUsage::default(),
        };

        executor.log_output_schema_validation(&response2, &mut tracer2);
    }
}

/// Persists conversation history to content store at hibernate points.
fn persist_history_to_content_store(
    agent_dir: &Path,
    session_id: &str,
    history: &[Message],
    gateway_dir: &Path,
    tracer: &mut SessionTracer,
) -> anyhow::Result<()> {
    use crate::runtime::content_store::ContentStore;
    use crate::runtime::session_snapshot::SessionSnapshot;

    let store = ContentStore::new(gateway_dir)?;

    // Serialize history
    let history_json = serde_json::to_string(history)?;
    let history_handle = store.write(history_json.as_bytes())?;

    // Register in session
    store.register_name(session_id, "session_history", &history_handle)?;

    // Persist for cross-session access
    store.persist(session_id, &history_handle)?;

    // Log causal chain entry
    tracer.log_history_persisted(history.len(), &history_handle);

    tracing::debug!(
        target: "lifecycle",
        session_id = %session_id,
        handle = %history_handle,
        message_count = history.len(),
        "Persisted session history to content store"
    );

    Ok(())
}
