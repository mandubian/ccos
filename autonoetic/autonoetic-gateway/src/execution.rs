//! Shared gateway execution service for ingress and scheduler-driven runs.

use crate::causal_chain::CausalLogger;
use crate::llm::{build_driver, Message};
use crate::runtime::lifecycle::{execute_scheduled_action, AgentExecutor};
use crate::runtime::session_context::SessionContext;
use crate::agent::AgentRepository;
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

        self.execute_with_reliability_controls(agent_id, || async move {
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

            let loaded = repo.get_sync(agent_id)?;
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
            })
        })
        .await
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
    let mut history = vec![Message::system(instructions.to_string())];
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
}
