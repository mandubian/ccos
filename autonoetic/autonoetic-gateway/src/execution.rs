//! Shared gateway execution service for ingress and scheduler-driven runs.

use crate::agent::AgentRepository;
use crate::causal_chain::CausalLogger;
use crate::llm::{build_driver, Message};
use crate::runtime::lifecycle::{compose_system_instructions, AgentExecutor};
use crate::runtime::reevaluation_state::execute_scheduled_action;
use crate::runtime::openrouter_catalog::OpenRouterCatalog;
use crate::runtime::session_budget::SessionBudgetRegistry;
use crate::runtime::session_context::SessionContext;
use crate::runtime::session_timeline::{base_session_id, SessionTimelineWriter};
use autonoetic_types::agent::{AgentManifest, ExecutionMode, LlmExchangeUsage};
use autonoetic_types::background::ScheduledAction;
use autonoetic_types::causal_chain::{CausalChainEntry, EntryStatus};
use autonoetic_types::config::GatewayConfig;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactMetadata {
    pub id: String,           // content handle (sha256:...)
    pub name: String,         // agent name from SKILL.md frontmatter
    pub description: String,
    pub files: Vec<String>,   // list of file names in the artifact
    pub entry_point: Option<String>,
    pub io: Option<serde_json::Value>,
}

/// A single named content item written by a child agent during a spawn.
///
/// Included in `SpawnResult.files` so the caller (parent agent / planner) gets
/// a structured manifest of everything the child produced — no need to mine
/// handles from the free-text reply.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentFile {
    /// The name the child registered the content under (e.g. "weather_fetcher.py").
    pub name: String,
    /// Full SHA-256 content handle (e.g. "sha256:838ddf76...").
    pub handle: String,
    /// Short 8-hex-char alias for LLM-friendly lookup (e.g. "838ddf76").
    pub alias: String,
}

/// Knowledge shared during execution that the caller can access.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedKnowledge {
    pub id: String,           // memory_id
    pub scope: String,
    pub content_preview: String, // first 100 chars
    pub writer_agent_id: String,
    pub created_at: String,
}

/// Extracts structured-agent artifacts from the content store by looking for SKILL.md files.
///
/// This function:
/// 1. Lists all content names in the session
/// 2. Finds any SKILL.md files
/// 3. Parses YAML frontmatter to extract metadata
/// 4. Creates ArtifactMetadata for each SKILL.md found
pub fn extract_artifacts_from_content_store(
    gateway_dir: &std::path::Path,
    session_id: &str,
) -> anyhow::Result<Vec<ArtifactMetadata>> {
    let store = crate::runtime::content_store::ContentStore::new(gateway_dir)?;
    let names = store.list_names(session_id)?;
    
    let mut artifacts = Vec::new();
    
    for name in &names {
        // Look for SKILL.md files
        if name.ends_with("SKILL.md") || name == "SKILL.md" {
            match store.read_by_name(session_id, name) {
                Ok(content_bytes) => {
                    if let Ok(content) = String::from_utf8(content_bytes) {
                        if let Some(metadata) = parse_skill_md_artifact(&store, session_id, name, &content) {
                            artifacts.push(metadata);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        target: "artifacts",
                        name = %name,
                        error = %e,
                        "Failed to read SKILL.md from content store"
                    );
                }
            }
        }
    }
    
    Ok(artifacts)
}

/// Collects all named content written by an agent during a spawn session.
///
/// Returns one `ContentFile` per named entry in the session manifest.
/// Namespaced names (containing `/` with the shape of a session path) are excluded
/// because those are parent-propagation copies, not original child outputs.
///
/// This gives the calling agent (planner) a structured manifest of everything the
/// child produced — with names, handles, and short aliases — without having to parse
/// the child's free-text reply.
pub fn collect_named_content(
    gateway_dir: &std::path::Path,
    session_id: &str,
) -> Vec<ContentFile> {
    let Ok(store) = crate::runtime::content_store::ContentStore::new(gateway_dir) else {
        return Vec::new();
    };
    let Ok(entries) = store.list_names_with_handles(session_id) else {
        return Vec::new();
    };

    entries
        .into_iter()
        .filter_map(|(name, handle)| {
            // Exclude internal session-snapshot names and namespaced propagation copies.
            // A namespaced copy looks like "some-session-id/filename" where the prefix
            // contains a UUID fragment (hex chars and hyphens). We skip any name whose
            // first path component looks like a session path segment.
            if name.starts_with("snapshot:") {
                return None;
            }
            // If the name contains a '/' and the part before the first '/' looks like a
            // session ID fragment (contains '-' or is long), treat it as a namespaced
            // propagation copy and skip it — the flat version is also registered.
            if let Some(slash_pos) = name.find('/') {
                let prefix = &name[..slash_pos];
                // Session ID segments contain hyphens (e.g. "demo-session", "coder-abc123")
                if prefix.contains('-') || prefix.len() > 12 {
                    return None;
                }
            }
            let alias = crate::runtime::content_store::ContentStore::get_short_alias(&handle);
            Some(ContentFile { name, handle, alias })
        })
        .collect()
}

/// Collects knowledge that was shared with a specific agent during execution.
///
/// Queries the Tier 2 memory for records that:
/// 1. Have visibility "shared" or "global"
/// 2. Include the target_agent_id in allowed_agents
/// 3. Were created or updated recently (within this session)
pub fn collect_shared_knowledge(
    gateway_dir: &std::path::Path,
    target_agent_id: &str,
    writer_agent_id: &str,
) -> Vec<SharedKnowledge> {
    let Ok(mem) = crate::runtime::memory::Tier2Memory::new(gateway_dir, writer_agent_id) else {
        return Vec::new();
    };

    // Get all memories owned by the writer agent
    let Ok(all_memories) = mem.list_memories() else {
        return Vec::new();
    };

    // Filter to those shared with the target agent
    all_memories
        .into_iter()
        .filter(|m| {
            match &m.visibility {
                autonoetic_types::memory::MemoryVisibility::Global => true,
                autonoetic_types::memory::MemoryVisibility::Shared => {
                    m.allowed_agents.contains(&target_agent_id.to_string())
                }
                autonoetic_types::memory::MemoryVisibility::Private => false,
            }
        })
        .map(|m| {
            let preview = if m.content.len() > 100 {
                format!("{}...", &m.content[..100])
            } else {
                m.content.clone()
            };
            SharedKnowledge {
                id: m.memory_id,
                scope: m.scope,
                content_preview: preview,
                writer_agent_id: m.writer_agent_id,
                created_at: m.created_at,
            }
        })
        .collect()
}

/// Parses SKILL.md content and creates ArtifactMetadata.
///
/// Uses loose/soft validation:
/// - Missing or invalid frontmatter → still creates artifact with defaults
/// - Missing fields → sensible defaults (name from dir, empty description)
/// - This matches the "soft validation" approach for LLM-generated content
fn parse_skill_md_artifact(
    store: &crate::runtime::content_store::ContentStore,
    session_id: &str,
    skill_md_name: &str,
    content: &str,
) -> Option<ArtifactMetadata> {
    // Get all files in the session (needed regardless of parsing)
    let files = store.list_names(session_id).unwrap_or_default();
    
    // Use the directory of SKILL.md as the artifact ID prefix
    let artifact_dir = if skill_md_name.contains('/') {
        skill_md_name.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("")
    } else {
        ""
    };
    
    // Derive default name from directory
    let default_name = artifact_dir
        .split('/')
        .last()
        .unwrap_or("unknown")
        .to_string();
    
    // Try to parse frontmatter, but use defaults if it fails
    #[derive(Deserialize)]
    struct SkillFrontmatter {
        name: Option<String>,
        description: Option<String>,
        script_entry: Option<String>,
        io: Option<serde_json::Value>,
    }
    
    let (name, description, script_entry, io) = match content.split("---").collect::<Vec<&str>>().get(1) {
        Some(frontmatter) => {
            // Attempt to parse YAML - if it fails, use defaults
            match serde_yaml::from_str::<SkillFrontmatter>(frontmatter) {
                Ok(fm) => (
                    fm.name.unwrap_or(default_name),
                    fm.description.unwrap_or_default(),
                    fm.script_entry,
                    fm.io,
                ),
                Err(e) => {
                    tracing::debug!(
                        target: "artifacts",
                        skill_md = %skill_md_name,
                        error = %e,
                        "Could not parse SKILL.md frontmatter, using defaults"
                    );
                    (default_name, String::new(), None, None)
                }
            }
        }
        None => {
            // No frontmatter markers - still create artifact with defaults
            tracing::debug!(
                target: "artifacts",
                skill_md = %skill_md_name,
                "SKILL.md has no frontmatter, using defaults"
            );
            (default_name, String::new(), None, None)
        }
    };
    
    // Filter files that are in the same directory as SKILL.md
    let artifact_files: Vec<String> = files
        .iter()
        .filter(|f| {
            if artifact_dir.is_empty() {
                !f.contains('/')
            } else {
                f.starts_with(artifact_dir)
            }
        })
        .cloned()
        .collect();
    
    // Compute a combined handle for the artifact (hash of all file handles)
    let mut combined_hash = Sha256::new();
    for file in &artifact_files {
        if let Ok(handle) = store.resolve_name(session_id, file) {
            combined_hash.update(handle.as_bytes());
        }
    }
    let artifact_id = format!("sha256:{:x}", combined_hash.finalize());
    
    // Always return an artifact if we found the SKILL.md file
    Some(ArtifactMetadata {
        id: artifact_id,
        name,
        description,
        files: artifact_files,
        entry_point: script_entry,
        io,
    })
}

#[derive(Debug)]
pub struct SpawnResult {
    pub agent_id: String,
    pub session_id: String,
    pub assistant_reply: Option<String>,
    pub should_signal_background: bool,
    pub artifacts: Vec<ArtifactMetadata>,
    /// All named content written by the child agent during this spawn.
    /// The calling agent (e.g. planner) can use `name`, `handle`, or `alias`
    /// to read any of these files via `content.read` without parsing reply text.
    pub files: Vec<ContentFile>,
    pub shared_knowledge: Vec<SharedKnowledge>,
    /// Per–LLM-round token usage for this run (JSON-RPC / CLI can surface this).
    pub llm_usage: Vec<LlmExchangeUsage>,
}

#[derive(Clone)]
pub struct GatewayExecutionService {
    config: Arc<GatewayConfig>,
    http_client: reqwest::Client,
    execution_semaphore: Arc<Semaphore>,
    agent_admission: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
    agent_execution_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    /// Shared per-session budget counters for all spawns using this gateway process.
    session_budget: Arc<SessionBudgetRegistry>,
}

impl GatewayExecutionService {
    pub fn new(config: GatewayConfig) -> Self {
        let session_budget = Arc::new(SessionBudgetRegistry::new(config.session_budget.clone()));
        Self {
            execution_semaphore: Arc::new(Semaphore::new(config.max_concurrent_spawns.max(1))),
            agent_admission: Arc::new(Mutex::new(HashMap::new())),
            agent_execution_locks: Arc::new(Mutex::new(HashMap::new())),
            config: Arc::new(config),
            http_client: reqwest::Client::new(),
            session_budget,
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
        _metadata: Option<&serde_json::Value>,
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

            let loaded = repo.get_sync(agent_id)?;

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

            // --- Fast path for script-only agents ---
            if matches!(loaded.manifest.execution_mode, ExecutionMode::Script) {
                let script_entry = loaded.manifest.script_entry.as_ref().ok_or_else(|| {
                    anyhow::anyhow!(
                        "Agent '{}' has execution_mode=script but is missing script_entry",
                        agent_id
                    )
                })?;
                let script_path = loaded.dir.join(script_entry);
                if !script_path.exists() {
                    anyhow::bail!(
                        "Script entry point not found: {}",
                        script_path.display()
                    );
                }

                // Log script start to gateway causal chain
                let gateway_logger = init_gateway_causal_logger(self.config.as_ref())?;
                log_gateway_causal_event(
                    &gateway_logger,
                    agent_id,
                    session_id,
                    1,
                    "script.started",
                    EntryStatus::Success,
                    Some(serde_json::json!({
                        "script_entry": script_entry,
                        "sandbox": loaded.manifest.runtime.sandbox
                    })),
                );

                // Execute script directly in sandbox
                let script_result = execute_script_in_sandbox(
                    &loaded.dir,
                    &script_path,
                    message,
                    &loaded.manifest.runtime.sandbox,
                    self.config.as_ref(),
                )
                .await;

                // Log script completion/failure
                match &script_result {
                    Ok(result) => {
                        log_gateway_causal_event(
                            &gateway_logger,
                            agent_id,
                            session_id,
                            2,
                            "script.completed",
                            EntryStatus::Success,
                            Some(serde_json::json!({
                                "result_len": result.len()
                            })),
                        );
                    }
                    Err(e) => {
                        log_gateway_causal_event(
                            &gateway_logger,
                            agent_id,
                            session_id,
                            2,
                            "script.failed",
                            EntryStatus::Error,
                            Some(serde_json::json!({
                                "error": e.to_string()
                            })),
                        );
                    }
                }

                // Return result (or error)
                let script_result = script_result?;

                // Extract artifacts from content store
                let artifacts = extract_artifacts_from_content_store(
                    &self.config.agents_dir.join(".gateway"),
                    session_id,
                ).unwrap_or_default();

                // Collect all named content written by the child agent
                let files = collect_named_content(
                    &self.config.agents_dir.join(".gateway"),
                    session_id,
                );

                // Collect shared knowledge (for script mode, typically empty)
                let shared_knowledge = collect_shared_knowledge(
                    &self.config.agents_dir.join(".gateway"),
                    source_agent_id.unwrap_or(agent_id),
                    agent_id,
                );

                return Ok(SpawnResult {
                    agent_id: agent_id.to_string(),
                    session_id: session_id.to_string(),
                    assistant_reply: Some(script_result),
                    should_signal_background,
                    artifacts,
                    files,
                    shared_knowledge,
                    llm_usage: Vec::new(),
                });
            }

            let llm_config = loaded
                .manifest
                .llm_config
                .clone()
                .ok_or_else(|| anyhow::anyhow!("Agent '{}' is missing llm_config", agent_id))?;
            let driver = build_driver(llm_config, self.http_client.clone())?;

            let openrouter_catalog =
                Arc::new(OpenRouterCatalog::new(self.http_client.clone()));
            let middleware = loaded.manifest.middleware.clone().unwrap_or_default();
            let mut runtime = AgentExecutor::new(
                loaded.manifest,
                loaded.instructions,
                driver,
                loaded.dir,
                crate::runtime::tools::default_registry(),
            )
            .with_gateway_dir(self.config.agents_dir.join(".gateway"))
            .with_config(self.config.clone())
            .with_session_budget(Some(self.session_budget.clone()))
            .with_openrouter_catalog(Some(openrouter_catalog))
            .with_middleware(middleware)
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
            let llm_usage = runtime.take_llm_usage_last_run();

            // Extract artifacts from content store
            let artifacts = extract_artifacts_from_content_store(
                &self.config.agents_dir.join(".gateway"),
                &resolved_session_id,
            ).unwrap_or_default();

            // Collect all named content written by the child agent
            let files = collect_named_content(
                &self.config.agents_dir.join(".gateway"),
                &resolved_session_id,
            );

            // Collect knowledge shared with the caller
            let shared_knowledge = collect_shared_knowledge(
                &self.config.agents_dir.join(".gateway"),
                source_agent_id.unwrap_or(agent_id),
                agent_id,
            );

            Ok(SpawnResult {
                agent_id: agent_id.to_string(),
                session_id: resolved_session_id,
                assistant_reply,
                should_signal_background,
                artifacts,
                files,
                shared_knowledge,
                llm_usage,
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
        "llm_usage": result.llm_usage,
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
    let status_clone = status.clone();
    if let Err(e) = logger.log(
        actor_id, session_id, None, event_seq, "gateway", action, status, payload.clone(),
    ) {
        tracing::warn!(error = %e, action, "Failed to append gateway causal log entry");
    }

    if let Err(e) = update_session_index(logger, actor_id, session_id, event_seq, action, &status_clone, payload.as_ref()) {
        tracing::warn!(error = %e, action, "Failed to update session index");
    }

    // Mirror orchestration into the same Markdown timeline as agent rows (`timeline.md`).
    if action.starts_with("workflow.") {
        append_workflow_gateway_timeline_best_effort(
            logger,
            actor_id,
            session_id,
            action,
            &status_clone,
            payload.as_ref(),
        );
    }
}

/// Best-effort: append one row to `.gateway/sessions/{base}/timeline.md` for workflow mirror events.
fn append_workflow_gateway_timeline_best_effort(
    logger: &CausalLogger,
    actor_id: &str,
    session_id: &str,
    action: &str,
    status: &EntryStatus,
    payload: Option<&serde_json::Value>,
) {
    let Some(gateway_dir) = logger.path().parent().and_then(|p| p.parent()) else {
        tracing::warn!(path = ?logger.path(), "workflow timeline: cannot resolve gateway dir");
        return;
    };
    let base = base_session_id(session_id).to_string();
    let mut writer = match SessionTimelineWriter::open(gateway_dir, &base) {
        Ok(w) => w,
        Err(e) => {
            tracing::warn!(
                target: "session_timeline",
                error = %e,
                base = %base,
                "workflow timeline: open failed"
            );
            return;
        }
    };
    let ts = chrono::Utc::now().to_rfc3339();
    if let Err(e) = writer.append(
        actor_id,
        session_id,
        &ts,
        "gateway",
        action,
        status,
        payload,
    ) {
        tracing::warn!(
            target: "session_timeline",
            error = %e,
            action = %action,
            "workflow timeline: append failed"
        );
    }
}

fn update_session_index(
    logger: &CausalLogger,
    actor_id: &str,
    session_id: &str,
    event_seq: u64,
    action: &str,
    status: &EntryStatus,
    payload: Option<&serde_json::Value>,
) -> anyhow::Result<()> {
    let index_path = logger.path()
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Logger path has no parent"))?
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Logger path has no grandparent"))?
        .join("sessions")
        .join(session_id)
        .join("index.json");

    let mut index = if index_path.exists() {
        serde_json::from_str::<SessionIndex>(&std::fs::read_to_string(&index_path)?)?
    } else {
        SessionIndex {
            session_id: session_id.to_string(),
            first_timestamp: None,
            last_timestamp: None,
            events: vec![],
        }
    };

    let timestamp = chrono::Utc::now().to_rfc3339();
    if index.first_timestamp.is_none() {
        index.first_timestamp = Some(timestamp.clone());
    }
    index.last_timestamp = Some(timestamp.clone());

    let log_id = format!("{}:{}:{}", actor_id, session_id, event_seq);
    
    let event_ref = SessionEventRef {
        log_id: log_id.clone(),
        agent_id: actor_id.to_string(),
        timestamp: timestamp.clone(),
        category: "gateway".to_string(),
        action: action.to_string(),
        status: status.clone(),
        causal_hash: payload.and_then(|p| p.get("causal_hash").and_then(|h| h.as_str())).map(String::from),
    };
    index.events.push(event_ref);

    if let Some(parent) = index_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&index_path, serde_json::to_string_pretty(&index)?)?;
    
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionIndex {
    session_id: String,
    first_timestamp: Option<String>,
    last_timestamp: Option<String>,
    events: Vec<SessionEventRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionEventRef {
    log_id: String,
    agent_id: String,
    timestamp: String,
    category: String,
    action: String,
    status: EntryStatus,
    causal_hash: Option<String>,
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

/// Execute a script agent directly in sandbox, bypassing the LLM.
async fn execute_script_in_sandbox(
    agent_dir: &PathBuf,
    script_path: &PathBuf,
    input_payload: &str,
    sandbox_type: &str,
    _config: &GatewayConfig,
) -> anyhow::Result<String> {
    use std::process::Stdio;
    use tokio::process::Command;

    tracing::info!(
        agent_dir = %agent_dir.display(),
        script = %script_path.display(),
        sandbox = %sandbox_type,
        "Executing script agent"
    );

    // Build the command based on sandbox type
    let (program, args) = match sandbox_type {
        "bubblewrap" | "bwrap" => {
            bubblewrap_command(agent_dir, script_path)?
        }
        "docker" => {
            docker_command(agent_dir, script_path)?
        }
        "microvm" => {
            microvm_command(script_path)?
        }
        _ => {
            bubblewrap_command(agent_dir, script_path)?
        }
    };

    // Create command with environment variables and pass input via stdin
    tracing::info!(
        program = %program,
        args = ?&args,
        input_len = input_payload.len(),
        input_preview = %input_payload.chars().take(100).collect::<String>(),
        "Spawning script process"
    );
    
    let mut cmd = Command::new(&program);
    cmd.args(&args)
        .env_clear()
        .env("SCRIPT_INPUT", input_payload)
        .env("AGENT_DIR", agent_dir.to_string_lossy().as_ref())
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("PYTHONUNBUFFERED", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Start the process and write input to stdin
    let mut child = cmd.spawn().map_err(|e| {
        anyhow::anyhow!("Failed to spawn script: {}", e)
    })?;

    // Write input to stdin and close it
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin.write_all(input_payload.as_bytes()).await.map_err(|e| {
            anyhow::anyhow!("Failed to write to script stdin: {}", e)
        })?;
        // stdin is dropped here, closing the pipe
    }

    // Wait for output
    let output = child.wait_with_output().await.map_err(|e| {
        anyhow::anyhow!("Failed to execute script: {}", e)
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        tracing::error!(stderr = %stderr, stdout = %stdout, status = ?output.status.code(), "Script execution failed");
        anyhow::bail!("Script execution failed with code {:?}: stdout={}, stderr={}", output.status.code(), stdout, stderr);
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    tracing::info!(stdout_len = stdout.len(), "Script execution completed");

    Ok(stdout)
}

/// Build bubblewrap command for executing a script.
/// 
/// NOTE: Full sandbox isolation requires bind-mounting Python's standard library.
/// For now, we use a simplified sandbox that shares the host filesystem.
/// TODO: Implement proper sandbox with minimal bind mounts.
fn bubblewrap_command(_agent_dir: &PathBuf, script_path: &PathBuf) -> anyhow::Result<(String, Vec<String>)> {
    // Determine interpreter based on script extension
    let interpreter = match script_path.extension().and_then(|e| e.to_str()) {
        Some("py") => "python3",
        Some("js") | Some("mjs") => "node",
        Some("rb") => "ruby",
        Some("sh") => "sh",
        Some("bash") => "bash",
        _ => "python3", // Default to python3
    };
    
    // For now, just run the script directly without sandbox isolation
    // This allows the demo to work while we implement proper sandboxing
    let args = vec![
        script_path.to_string_lossy().to_string(),
    ];
    
    Ok((interpreter.to_string(), args))
}

/// Build docker command for executing a script.
fn docker_command(agent_dir: &PathBuf, script_path: &PathBuf) -> anyhow::Result<(String, Vec<String>)> {
    let image = std::env::var("AUTONOETIC_DOCKER_IMAGE").unwrap_or_else(|_| "python:3.11".to_string());
    Ok((
        "docker".to_string(),
        vec![
            "run".to_string(),
            "--rm".to_string(),
            "-i".to_string(),
            "--network".to_string(), "none".to_string(),
            "-v".to_string(), format!("{}:/workspace", agent_dir.to_string_lossy()),
            image,
            script_path.to_string_lossy().to_string(),
        ],
    ))
}

/// Build microvm command for executing a script.
fn microvm_command(_script_path: &PathBuf) -> anyhow::Result<(String, Vec<String>)> {
    let config = std::env::var("AUTONOETIC_FIRECRACKER_CONFIG")
        .map_err(|_| anyhow::anyhow!("MicroVM requires AUTONOETIC_FIRECRACKER_CONFIG to be set"))?;
    Ok((
        "firecracker".to_string(),
        vec![
            "--config-file".to_string(), config,
        ],
    ))
}
