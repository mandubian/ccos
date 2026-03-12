//! Shared gateway execution service for ingress and scheduler-driven runs.

use crate::agent::AgentRepository;
use crate::causal_chain::CausalLogger;
use crate::llm::{build_driver, Message};
use crate::runtime::lifecycle::{compose_system_instructions, AgentExecutor};
use crate::runtime::reevaluation_state::execute_scheduled_action;
use crate::runtime::session_context::SessionContext;
use autonoetic_types::agent::AgentManifest;
use autonoetic_types::background::ScheduledAction;
use autonoetic_types::causal_chain::{CausalChainEntry, EntryStatus};
use autonoetic_types::config::GatewayConfig;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};

#[derive(Debug)]
pub struct SpawnResult {
    pub agent_id: String,
    pub session_id: String,
    pub assistant_reply: Option<String>,
    pub should_signal_background: bool,
}

#[derive(Clone)]
pub struct GatewayExecutionService {
    config: Arc<GatewayConfig>,
    http_client: reqwest::Client,
    execution_semaphore: Arc<Semaphore>,
    agent_admission: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
    agent_execution_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl GatewayExecutionService {
    pub fn new(config: GatewayConfig) -> Self {
        Self {
            execution_semaphore: Arc::new(Semaphore::new(config.max_concurrent_spawns.max(1))),
            agent_admission: Arc::new(Mutex::new(HashMap::new())),
            agent_execution_locks: Arc::new(Mutex::new(HashMap::new())),
            config: Arc::new(config),
            http_client: reqwest::Client::new(),
        }
    }

    pub fn config(&self) -> Arc<GatewayConfig> {
        self.config.clone()
    }

    pub async fn spawn_agent_once(
        &self,
        agent_id: &str,
        message: &str,
        session_id: &str,
        source_agent_id: Option<&str>,
        is_message: bool,
        ingest_event_type: Option<&str>,
        metadata: Option<&serde_json::Value>,
    ) -> anyhow::Result<SpawnResult> {
        let span = tracing::info_span!(
            "spawn_agent_once",
            agent_id = agent_id,
            session_id = session_id
        );
        let _enter = span.enter();

        tracing::info!("Spawning agent {} (session: {})", agent_id, session_id);

        anyhow::ensure!(!agent_id.trim().is_empty(), "agent_id must not be empty");
        anyhow::ensure!(!message.trim().is_empty(), "message must not be empty");

        let result = self
            .execute_with_reliability_controls(agent_id, || async move {
                let repo = AgentRepository::from_config(&self.config);

            if let Some(source_id) = source_agent_id {
                if source_id != agent_id {
                    let source_loaded = repo.get_sync(source_id)?;
                    let source_policy = crate::policy::PolicyEngine::new(source_loaded.manifest);

                    if is_message {
                        anyhow::ensure!(
                            source_policy.can_message_agent(agent_id),
                            "Permission Denied: Source agent '{}' lacks 'AgentMessage' capability to message '{}'",
                            source_id,
                            agent_id
                        );
                    } else {
                        let spawn_limit = source_policy.spawn_agent_limit().ok_or_else(|| {
                            anyhow::anyhow!(
                                "Permission Denied: Source agent '{}' lacks 'AgentSpawn' capability",
                                source_id
                            )
                        })?;
                        anyhow::ensure!(
                            spawn_limit > 0,
                            "Permission Denied: Source agent '{}' exceeded AgentSpawn limit (0) for session '{}'",
                            source_id,
                            session_id
                        );
                        let prior_child_spawns = count_spawned_children_for_source_session(
                            self.config.as_ref(),
                            source_id,
                            session_id,
                        )?;
                        anyhow::ensure!(
                            prior_child_spawns < spawn_limit as usize,
                            "Permission Denied: Source agent '{}' exceeded AgentSpawn limit ({}) for session '{}'",
                            source_id,
                            spawn_limit,
                            session_id
                        );
                    }
                }
            }

            let selected_adaptation_ids = extract_selected_adaptation_ids(metadata);
            let loaded = repo.get_sync_with_adaptations(
                agent_id,
                selected_adaptation_ids.as_deref(),
            )?;

            // Validate spawn input against target agent's accepts schema (informational only)
            if let Some(ref io_schema) = loaded.manifest.io {
                if let Some(ref accepts) = io_schema.accepts {
                    let validation = validate_against_schema(message, accepts);
                    tracing::info!(
                        agent_id = agent_id,
                        valid = validation.valid,
                        issues = ?validation.issues,
                        "Input schema validation"
                    );
                    if let Err(error) = log_input_schema_validation_to_gateway(
                        self.config.as_ref(),
                        session_id,
                        source_agent_id,
                        agent_id,
                        message,
                        &validation,
                    ) {
                        tracing::warn!(
                            error = %error,
                            agent_id = agent_id,
                            session_id = session_id,
                            "Failed to append input schema validation to gateway causal chain"
                        );
                    }
                }
            }
            // Determine if background signaling is needed
            let should_signal_background = ingest_event_type.is_some()
                && loaded
                    .manifest
                    .background
                    .as_ref()
                    .map(|bg| bg.enabled && bg.wake_predicates.new_messages)
                    .unwrap_or(false);
            // Signal inbox for background scheduler if this is an event.ingest call
            if should_signal_background {
                let event_type = ingest_event_type.unwrap();
                let _ = crate::scheduler::append_inbox_event(
                    &self.config,
                    agent_id,
                    crate::router::ingress_wake_signal_internal(event_type, session_id),
                    Some(session_id),
                );
            }
            let llm_config = loaded
                .manifest
                .llm_config
                .clone()
                .ok_or_else(|| anyhow::anyhow!("Agent '{}' is missing llm_config", agent_id))?;
            let driver = build_driver(llm_config, self.http_client.clone())?;

            let mut runtime = AgentExecutor::new(
                loaded.manifest,
                loaded.instructions,
                driver,
                loaded.dir,
                crate::runtime::tools::default_registry(),
            )
            .with_gateway_dir(self.config.agents_dir.join(".gateway"))
            .with_config(self.config.clone())
            .with_adaptation_hooks(loaded.adaptation_hooks)
            .with_adaptation_assets(loaded.adaptation_assets)
            .with_initial_user_message(message.to_string())
            .with_session_id(session_id.to_string());
            let mut history = build_initial_history(
                &runtime.agent_dir,
                &runtime.instructions,
                &runtime.initial_user_message,
                session_id,
            );
            let assistant_reply = runtime.execute_with_history(&mut history).await?;
            let resolved_session_id = runtime
                .session_id
                .clone()
                .ok_or_else(|| anyhow::anyhow!("runtime session_id missing after execution"))?;
            persist_session_context_turn(
                &runtime.agent_dir,
                &resolved_session_id,
                &runtime.initial_user_message,
                assistant_reply.as_deref(),
            );
            let close_reason = if assistant_reply.is_some() {
                "jsonrpc_spawn_complete"
            } else {
                "jsonrpc_spawn_complete_empty"
            };
            runtime.close_session(close_reason)?;

            Ok(SpawnResult {
                agent_id: agent_id.to_string(),
                session_id: resolved_session_id,
                assistant_reply,
                should_signal_background,
            })
        })
        .await?;
        if source_agent_id.is_some() {
            log_nested_spawn_to_gateway(
                self.config.as_ref(),
                session_id,
                source_agent_id,
                agent_id,
                message,
                &result,
            );
        }
        Ok(result)
    }

    pub async fn execute_background_action(
        &self,
        agent_id: &str,
        _session_id: &str,
        action: &ScheduledAction,
    ) -> anyhow::Result<String> {
        self.execute_with_reliability_controls(agent_id, || async move {
            let (manifest, agent_dir) = self.load_agent_manifest(agent_id)?;
            execute_scheduled_action(
                &manifest,
                &agent_dir,
                action,
                &crate::runtime::tools::default_registry(),
                Some(self.config.as_ref()),
            )
        })
        .await
    }

    pub fn load_agent_manifest(
        &self,
        agent_id: &str,
    ) -> anyhow::Result<(AgentManifest, std::path::PathBuf)> {
        let repo = AgentRepository::from_config(&self.config);
        let loaded = repo.get_sync(agent_id)?;
        Ok((loaded.manifest, loaded.dir))
    }

    pub async fn execute_with_reliability_controls<F, Fut, T>(
        &self,
        agent_id: &str,
        operation: F,
    ) -> anyhow::Result<T>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = anyhow::Result<T>>,
    {
        let agent_admission = self.agent_admission_semaphore(agent_id).await;
        let _admission_permit = agent_admission.try_acquire_owned().map_err(|_| {
            anyhow::anyhow!(
                "Backpressure: pending execution queue is full for agent '{}'",
                agent_id
            )
        })?;

        let agent_lock = self.agent_execution_lock(agent_id).await;
        let _agent_guard = agent_lock.lock().await;

        let _execution_permit = self
            .execution_semaphore
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                anyhow::anyhow!(
                    "Backpressure: max concurrent executions reached ({})",
                    self.config.max_concurrent_spawns.max(1)
                )
            })?;

        operation().await
    }

    pub async fn agent_admission_semaphore(&self, agent_id: &str) -> Arc<Semaphore> {
        let mut guards = self.agent_admission.lock().await;
        guards
            .entry(agent_id.to_string())
            .or_insert_with(|| {
                Arc::new(Semaphore::new(
                    self.config.max_pending_spawns_per_agent.max(1),
                ))
            })
            .clone()
    }

    pub fn execution_semaphore(&self) -> Arc<Semaphore> {
        self.execution_semaphore.clone()
    }

    async fn agent_execution_lock(&self, agent_id: &str) -> Arc<Mutex<()>> {
        let mut guards = self.agent_execution_locks.lock().await;
        guards
            .entry(agent_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}

fn extract_selected_adaptation_ids(metadata: Option<&serde_json::Value>) -> Option<Vec<String>> {
    let value = metadata?;
    let arr = value.get("selected_adaptation_ids")?.as_array()?;
    let ids: Vec<String> = arr
        .iter()
        .filter_map(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    Some(ids)
}

/// Logs agent.spawn.requested and agent.spawn.completed to the gateway causal chain for nested
/// delegations (when source_agent_id is set), so the gateway log shows the full delegation tree.
fn log_nested_spawn_to_gateway(
    config: &GatewayConfig,
    session_id: &str,
    source_agent_id: Option<&str>,
    agent_id: &str,
    message: &str,
    result: &SpawnResult,
) {
    let logger = match init_gateway_causal_logger(config) {
        Ok(l) => l,
        Err(_) => return,
    };
    let path = logger.path().to_path_buf();
    let entries = match CausalLogger::read_entries(&path) {
        Ok(e) => e,
        Err(err) => {
            if path.exists() {
                tracing::warn!(
                    error = %err,
                    "Failed to read existing gateway causal entries before input schema log"
                );
                return;
            }
            Vec::new()
        }
    };
    let mut seq = entries.last().map(|e| e.event_seq + 1).unwrap_or(1);
    let requested_data = serde_json::json!({
        "agent_id": agent_id,
        "source_agent_id": source_agent_id,
        "session_id": session_id,
        "message_len": message.len(),
        "message_sha256": sha256_hex(message),
    });
    log_gateway_causal_event(
        &logger,
        &gateway_actor_id(),
        session_id,
        seq,
        "agent.spawn.requested",
        EntryStatus::Success,
        Some(requested_data),
    );
    seq += 1;
    let completed_data = serde_json::json!({
        "agent_id": result.agent_id,
        "source_agent_id": source_agent_id,
        "session_id": result.session_id,
        "assistant_reply_len": result.assistant_reply.as_ref().map(|s| s.len()).unwrap_or(0),
        "assistant_reply_sha256": result.assistant_reply.as_ref().map(|s| sha256_hex(s)),
    });
    log_gateway_causal_event(
        &logger,
        &gateway_actor_id(),
        session_id,
        seq,
        "agent.spawn.completed",
        EntryStatus::Success,
        Some(completed_data),
    );
}

fn log_input_schema_validation_to_gateway(
    config: &GatewayConfig,
    session_id: &str,
    source_agent_id: Option<&str>,
    agent_id: &str,
    message: &str,
    validation: &SchemaValidation,
) -> anyhow::Result<()> {
    let logger = init_gateway_causal_logger(config)?;
    let path = logger.path().to_path_buf();
    let entries = match CausalLogger::read_entries(&path) {
        Ok(e) => e,
        Err(err) => {
            if path.exists() {
                return Err(err);
            }
            Vec::new()
        }
    };
    let seq = entries.last().map(|e| e.event_seq + 1).unwrap_or(1);
    let payload = serde_json::json!({
        "agent_id": agent_id,
        "source_agent_id": source_agent_id,
        "session_id": session_id,
        "valid": validation.valid,
        "issues": validation.issues,
        "issue_count": validation.issues.len(),
        "message_len": message.len(),
        "message_sha256": sha256_hex(message),
    });
    logger.log(
        &gateway_actor_id(),
        session_id,
        None,
        seq,
        "gateway",
        "agent.spawn.input_schema_validation",
        EntryStatus::Success,
        Some(payload),
    )?;
    Ok(())
}

pub fn gateway_actor_id() -> String {
    std::env::var("AUTONOETIC_NODE_ID").unwrap_or_else(|_| "gateway".to_string())
}

pub fn gateway_root_dir(config: &GatewayConfig) -> std::path::PathBuf {
    config.agents_dir.join(".gateway")
}

pub fn gateway_causal_path(config: &GatewayConfig) -> std::path::PathBuf {
    gateway_root_dir(config)
        .join("history")
        .join("causal_chain.jsonl")
}

pub fn init_gateway_causal_logger(config: &GatewayConfig) -> anyhow::Result<CausalLogger> {
    let path = gateway_causal_path(config);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    CausalLogger::new(path)
}

pub fn next_event_seq(counter: &mut u64) -> u64 {
    *counter += 1;
    *counter
}

pub fn log_gateway_causal_event(
    logger: &CausalLogger,
    actor_id: &str,
    session_id: &str,
    event_seq: u64,
    action: &str,
    status: EntryStatus,
    payload: Option<serde_json::Value>,
) {
    if let Err(e) = logger.log(
        actor_id, session_id, None, event_seq, "gateway", action, status, payload,
    ) {
        tracing::warn!(error = %e, action, "Failed to append gateway causal log entry");
    }
}

pub fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn build_initial_history(
    agent_dir: &std::path::Path,
    instructions: &str,
    user_message: &str,
    session_id: &str,
) -> Vec<Message> {
    let mut history = vec![Message::system(compose_system_instructions(instructions))];
    match SessionContext::load(agent_dir, session_id).and_then(|context| {
        Ok(context
            .render_prompt()
            .map(Message::system)
            .into_iter()
            .collect::<Vec<_>>())
    }) {
        Ok(mut injected) => history.append(&mut injected),
        Err(error) => tracing::warn!(
            error = %error,
            session_id,
            "Failed to load session context; continuing without injected continuity"
        ),
    }
    history.push(Message::user(user_message.to_string()));
    history
}

fn persist_session_context_turn(
    agent_dir: &std::path::Path,
    session_id: &str,
    user_message: &str,
    assistant_reply: Option<&str>,
) {
    let result = (|| -> anyhow::Result<()> {
        let mut context = SessionContext::load(agent_dir, session_id)?;
        context.record_turn(user_message, assistant_reply);
        context.save(agent_dir)?;
        Ok(())
    })();
    if let Err(error) = result {
        tracing::warn!(
            error = %error,
            session_id,
            "Failed to persist session context after execution"
        );
    }
}

fn count_spawned_children_for_source_session(
    config: &GatewayConfig,
    source_agent_id: &str,
    session_id: &str,
) -> anyhow::Result<usize> {
    let path = gateway_causal_path(config);
    if !path.exists() {
        return Ok(0);
    }

    let content = std::fs::read_to_string(path)?;
    let mut count = 0usize;
    for line in content.lines().filter(|line| !line.trim().is_empty()) {
        let entry: CausalChainEntry = serde_json::from_str(line)?;
        if entry.session_id != session_id {
            continue;
        }
        if entry.action != "agent.spawn.completed" && entry.action != "event.ingest.completed" {
            continue;
        }
        let Some(payload) = entry.payload.as_ref() else {
            continue;
        };
        let matches_source = payload
            .get("source_agent_id")
            .and_then(|value| value.as_str())
            .map(|value| value == source_agent_id)
            .unwrap_or(false);
        if matches_source {
            count += 1;
        }
    }

    Ok(count)
}

struct SchemaValidation {
    valid: bool,
    issues: Vec<String>,
}

/// Lightweight schema validation: checks required fields and basic type hints.
/// Logs results but does NOT hard-fail — the LLM can handle minor mismatches.
fn validate_against_schema(input: &str, schema: &serde_json::Value) -> SchemaValidation {
    let mut issues = Vec::new();

    // Try to parse input as JSON; if it's plain text, check if schema expects an object
    let input_value: serde_json::Value = match serde_json::from_str(input) {
        Ok(v) => v,
        Err(_) => {
            // Plain text input — if schema expects an object with required fields, note the mismatch
            if schema.get("type").and_then(|t| t.as_str()) == Some("object") {
                if let Some(required) = schema.get("required").and_then(|r| r.as_array()) {
                    if !required.is_empty() {
                        issues.push(format!(
                            "Input is plain text but schema expects object with required fields: {:?}",
                            required.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>()
                        ));
                    }
                }
            }
            return SchemaValidation {
                valid: issues.is_empty(),
                issues,
            };
        }
    };

    // Check type
    if let Some(expected_type) = schema.get("type").and_then(|t| t.as_str()) {
        let actual_type = match &input_value {
            serde_json::Value::Object(_) => "object",
            serde_json::Value::Array(_) => "array",
            serde_json::Value::String(_) => "string",
            serde_json::Value::Number(_) => "number",
            serde_json::Value::Bool(_) => "boolean",
            serde_json::Value::Null => "null",
        };
        if actual_type != expected_type {
            issues.push(format!(
                "Type mismatch: expected '{}', got '{}'",
                expected_type, actual_type
            ));
        }
    }

    // Check required fields for objects
    if let Some(required) = schema.get("required").and_then(|r| r.as_array()) {
        if let Some(obj) = input_value.as_object() {
            for field in required {
                if let Some(field_name) = field.as_str() {
                    if !obj.contains_key(field_name) {
                        issues.push(format!("Missing required field: '{}'", field_name));
                    }
                }
            }
        }
    }

    SchemaValidation {
        valid: issues.is_empty(),
        issues,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::session_context::session_context_path;

    #[test]
    fn test_build_initial_history_injects_session_context_before_user_message() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let mut context = SessionContext::empty("session-1");
        context.record_turn("remember Atlas", Some("Stored that."));
        context
            .save(temp.path())
            .expect("session context should save");

        let history = build_initial_history(
            temp.path(),
            "System prompt",
            "What did I ask you to remember?",
            "session-1",
        );

        assert_eq!(history.len(), 3);
        assert_eq!(history[0].role.as_str(), "system");
        assert_eq!(history[2].role.as_str(), "user");
        assert!(history[0]
            .content
            .contains("Autonoetic Gateway Foundation Rules"));
        assert!(history[0].content.contains("System prompt"));
        assert!(history[1]
            .content
            .contains("Last user message: remember Atlas"));
        assert!(history[1]
            .content
            .contains("Last assistant reply: Stored that."));
    }

    #[test]
    fn test_persist_session_context_turn_writes_current_exchange() {
        let temp = tempfile::tempdir().expect("tempdir should create");

        persist_session_context_turn(
            temp.path(),
            "session-2",
            "hello there",
            Some("general kenobi"),
        );

        let path = session_context_path(temp.path(), "session-2");
        let body = std::fs::read_to_string(path).expect("session context file should exist");
        assert!(body.contains("\"last_user_message\": \"hello there\""));
        assert!(body.contains("\"last_assistant_reply\": \"general kenobi\""));
    }

    #[test]
    fn test_validate_valid_json_input() {
        let schema = serde_json::json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "query": { "type": "string" }
            }
        });
        let input = r#"{"query": "test search"}"#;
        let result = validate_against_schema(input, &schema);
        assert!(result.valid, "Expected valid, got issues: {:?}", result.issues);
    }

    #[test]
    fn test_validate_missing_required_field() {
        let schema = serde_json::json!({
            "type": "object",
            "required": ["query", "domain"],
            "properties": {
                "query": { "type": "string" },
                "domain": { "type": "string" }
            }
        });
        let input = r#"{"query": "test"}"#;
        let result = validate_against_schema(input, &schema);
        assert!(!result.valid);
        assert!(result.issues.iter().any(|i| i.contains("domain")));
    }

    #[test]
    fn test_validate_type_mismatch() {
        let schema = serde_json::json!({
            "type": "object",
            "required": ["count"],
            "properties": {
                "count": { "type": "number" }
            }
        });
        let input = r#"["not", "an", "object"]"#;
        let result = validate_against_schema(input, &schema);
        assert!(!result.valid);
        assert!(result.issues.iter().any(|i| i.contains("Type mismatch")));
    }

    #[test]
    fn test_validate_plain_text_input() {
        let schema = serde_json::json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "query": { "type": "string" }
            }
        });
        let input = "just a plain text query";
        let result = validate_against_schema(input, &schema);
        assert!(!result.valid);
        assert!(result.issues.iter().any(|i| i.contains("plain text")));
    }

    #[test]
    fn test_log_input_schema_validation_to_gateway_writes_event() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let mut config = GatewayConfig::default();
        config.agents_dir = temp.path().join("agents");

        let validation = SchemaValidation {
            valid: false,
            issues: vec!["Missing required field: 'query'".to_string()],
        };
        log_input_schema_validation_to_gateway(
            &config,
            "session-3",
            Some("planner.default"),
            "researcher.default",
            "plain text query",
            &validation,
        )
        .expect("schema validation event should log");

        let entries = CausalLogger::read_entries(&gateway_causal_path(&config))
            .expect("causal entries should be readable");
        let last = entries.last().expect("expected at least one causal entry");
        assert_eq!(last.action, "agent.spawn.input_schema_validation");
        assert_eq!(last.session_id, "session-3");
        assert!(matches!(last.status, EntryStatus::Success));
        let payload = last.payload.as_ref().expect("payload should be present");
        assert_eq!(payload["valid"], serde_json::Value::Bool(false));
        assert_eq!(payload["agent_id"], "researcher.default");
    }
}
