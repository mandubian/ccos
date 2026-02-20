use log;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Json, Path, Query, State, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use chrono::{DateTime, Utc};
use rtfs::ast::MapKey;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};

use crate::approval::{
    storage_file::FileApprovalStorage, ApprovalCategory, ApprovalConsumer, ApprovalRequest,
    UnifiedApprovalQueue,
};
use crate::capabilities::registry::CapabilityRegistry;
use crate::capability_marketplace::types::CapabilityDiscovery;
use crate::capability_marketplace::CapabilityMarketplace;
use crate::causal_chain::{CausalChain, CausalQuery};
use crate::chat::run::{BudgetContext, Run, RunState, RunStore, SharedRunStore};
use crate::chat::{
    attach_label, record_chat_audit_event, register_chat_capabilities, AgentMonitor, AgentSpawner,
    ChatDataLabel, ChatMessage, Checkpoint, CheckpointStore, FileQuarantineStore,
    InMemoryCheckpointStore, MessageDirection, MessageEnvelope, QuarantineKey, QuarantineStore,
    RealTimeTrackingSink, Scheduler, SessionRegistry, SpawnConfig, SpawnerFactory,
};
use crate::chat::{ResourceStore, SharedResourceStore};
use crate::config::types::AgentConfig;
use crate::utils::log_redaction::{redact_json_for_logs, redact_token_for_logs};
use crate::utils::value_conversion::{json_to_rtfs_value, rtfs_value_to_json};

use super::connector::{
    ChatConnector, ConnectionHandle, LoopbackConnectorConfig, LoopbackWebhookConnector,
    OutboundRequest,
};
use crate::chat::agent_log::{AgentLogRequest, AgentLogResponse};

#[derive(Debug, Clone)]
pub struct ChatGatewayConfig {
    pub bind_addr: String,
    pub approvals_dir: PathBuf,
    pub quarantine_dir: PathBuf,
    pub quarantine_default_ttl_seconds: i64,
    pub quarantine_key_env: String,
    pub policy_pack_version: String,
    pub connector: LoopbackConnectorConfig,
    /// Optional outbound HTTP host allowlist for governed egress (ccos.network.http-fetch).
    ///
    /// If empty, the gateway defaults to allowing only localhost/127.0.0.1 for safety.
    pub http_allow_hosts: Vec<String>,
    /// Optional outbound HTTP port allowlist for governed egress (ccos.network.http-fetch).
    ///
    /// If empty, all ports are allowed (subject to host allowlist).
    pub http_allow_ports: Vec<u16>,
    /// Admin tokens for privileged operations (listing sessions, WebSocket upgrade, etc.)
    ///
    /// If empty, no admin access is allowed via tokens. Can be set via CCOS_ADMIN_TOKENS env var
    /// (comma-separated) or in config. Defaults to empty for production safety.
    pub admin_tokens: Vec<String>,
    /// Base URL for the Approval UI, used in nudge messages to direct users.
    ///
    /// Defaults to "http://localhost:3000" for development.
    pub approval_ui_url: String,
}

#[derive(Clone)]
pub struct ChatGateway {
    state: Arc<GatewayState>,
}

#[allow(dead_code)]
pub(crate) struct GatewayState {
    pub(crate) marketplace: Arc<CapabilityMarketplace>,
    pub(crate) quarantine: Arc<dyn QuarantineStore>,
    pub(crate) chain: Arc<Mutex<CausalChain>>,
    pub(crate) _approvals: Option<UnifiedApprovalQueue<FileApprovalStorage>>,
    pub(crate) connector: Arc<dyn ChatConnector>,
    pub(crate) connector_handle: ConnectionHandle,
    pub(crate) inbox: Mutex<VecDeque<MessageEnvelope>>,
    pub(crate) policy_pack_version: String,
    /// Session registry for managing agent sessions
    pub(crate) session_registry: SessionRegistry,
    /// Agent spawner for launching agent runtimes
    pub(crate) spawner: Arc<dyn AgentSpawner>,
    /// Run store for managing autonomous runs
    pub(crate) run_store: SharedRunStore,
    /// Instruction resource store (URLs/text/docs) for restart-safe autonomy
    pub(crate) resource_store: SharedResourceStore,
    /// Scheduler for delayed/recurring runs
    pub(crate) scheduler: Arc<Scheduler>,
    /// Checkpoint store for persistence
    pub(crate) checkpoint_store: Arc<dyn CheckpointStore>,
    /// Internal secret for native capabilities (like scheduler)
    pub(crate) internal_api_secret: String,
    /// Real-time event tracking sink for WebSocket streaming
    pub(crate) realtime_sink: Arc<RealTimeTrackingSink>,
    /// Admin tokens for privileged operations
    pub(crate) admin_tokens: Vec<String>,
    /// Base URL for the Approval UI
    pub(crate) approval_ui_url: String,
    /// Optional LLM profile override applied to newly spawned agents.
    pub(crate) spawn_llm_profile: Arc<RwLock<Option<String>>>,
    /// Optional session-specific LLM profile overrides.
    pub(crate) session_llm_profiles: Arc<RwLock<HashMap<String, String>>>,
    /// Shared WorkingMemory instance used to inject prior-run context into scheduled agent spawns.
    pub(crate) working_memory: Arc<Mutex<crate::working_memory::WorkingMemory>>,
}

#[async_trait]
impl ApprovalConsumer for GatewayState {
    async fn on_approval_requested(&self, request: &ApprovalRequest) {
        log::debug!("[Gateway] Approval requested: {}", request.id);
        let session_id = request.metadata.get("session_id");
        if let Some(session_id) = session_id {
            self.nudge_user(session_id, &request.id, &request.category)
                .await;
        }
    }

    async fn on_approval_resolved(&self, request: &ApprovalRequest) {
        log::info!(
            "[Gateway] Approval resolved: {} (status: {:?})",
            request.id,
            request.status
        );

        let session_id = request.metadata.get("session_id");
        if let Some(session_id) = session_id {
            let category_label = match &request.category {
                crate::approval::types::ApprovalCategory::HttpHostApproval { .. } => {
                    "HTTP host approval"
                }
                crate::approval::types::ApprovalCategory::SecretWrite { .. } => {
                    "secret write approval"
                }
                crate::approval::types::ApprovalCategory::HumanActionRequest { .. } => {
                    "human action approval"
                }
                crate::approval::types::ApprovalCategory::EffectApproval { .. } => {
                    "effect approval"
                }
                _ => "approval",
            };
            // Notify chat about resolution
            let status_msg = match &request.status {
                crate::approval::types::ApprovalStatus::Approved { .. } => {
                    format!("✅ Approved ({}).", category_label)
                }
                crate::approval::types::ApprovalStatus::Rejected { reason, .. } => {
                    format!("❌ Rejected ({}): {}", category_label, reason)
                }
                _ => return,
            };
            self.send_chat_message(
                session_id,
                format!("Approval `{}`: {}", request.id, status_msg),
            )
            .await;

            if request.status.is_approved() {
                // First, try to find and resume a paused run
                let paused_run_id = {
                    let store = self.run_store.lock().unwrap();
                    store.get_latest_paused_approval_run_id_for_session(session_id)
                };

                if let Some(run_id) = paused_run_id {
                    // Resume the run (we need lock again)
                    {
                        let mut store = self.run_store.lock().unwrap();
                        store.update_run_state(&run_id, crate::chat::run::RunState::Active);
                    }

                    SheriffLogger::log_run(
                        "AUTO-RESUME",
                        &run_id,
                        session_id,
                        &format!("Resuming run after approval {}", request.id),
                    );

                    // Record event (no lock needed for record_run_event)
                    let _ = record_run_event(
                        self,
                        session_id,
                        &run_id,
                        &format!("auto-resume-{}", request.id),
                        "run.auto_resume",
                        HashMap::new(),
                    )
                    .await;
                } else {
                    // No paused run - this might be an HTTP host approval that blocked
                    // a capability call. Push a retry hint to the agent's inbox.
                    if matches!(
                        &request.category,
                        crate::approval::types::ApprovalCategory::HttpHostApproval { .. }
                    ) {
                        log::info!(
                            "[Gateway] No paused run for HTTP host approval {}, pushing retry hint to agent",
                            request.id
                        );

                        // Push a retry hint to the agent's inbox
                        let parts: Vec<&str> = session_id.split(':').collect();
                        if parts.len() >= 2 {
                            let channel_id = parts[1];
                            let retry_hint =
                                "🔄 Host approved. You can now retry the previous operation.";
                            let _ = self
                                .session_registry
                                .push_message_to_session(
                                    session_id,
                                    channel_id.to_string(),
                                    retry_hint.to_string(),
                                    "system".to_string(),
                                    None,
                                )
                                .await;
                        }
                    }

                    // Handle PackageApproval - resume paused run and push retry hint
                    if let crate::approval::types::ApprovalCategory::PackageApproval {
                        ref package,
                        ..
                    } = request.category
                    {
                        log::info!(
                            "[Gateway] Package {} approved for session {}, checking for paused run",
                            package,
                            session_id
                        );

                        // First, try to find the specific run from the approval metadata
                        let paused_run_id = if let Some(run_id) = request.metadata.get("run_id") {
                            // Verify this run is still in PausedApproval state
                            let store = self.run_store.lock().unwrap();
                            if let Some(run) = store.get_run(run_id) {
                                if matches!(run.state, RunState::PausedApproval { .. }) {
                                    Some(run_id.clone())
                                } else {
                                    log::info!(
                                        "[Gateway] Run {} from approval metadata is not in PausedApproval state (state: {:?}), falling back to session search",
                                        run_id,
                                        run.state
                                    );
                                    None
                                }
                            } else {
                                None
                            }
                        } else {
                            // Fall back to finding any paused run for this session
                            let store = self.run_store.lock().unwrap();
                            store.get_latest_paused_approval_run_id_for_session(session_id)
                        };

                        if let Some(run_id) = paused_run_id {
                            // Resume the run
                            {
                                let mut store = self.run_store.lock().unwrap();
                                store.update_run_state(&run_id, crate::chat::run::RunState::Active);
                            }

                            SheriffLogger::log_run(
                                "AUTO-RESUME",
                                &run_id,
                                session_id,
                                &format!("Resuming run after package approval {}", request.id),
                            );

                            // Record event
                            let _ = record_run_event(
                                self,
                                session_id,
                                &run_id,
                                &format!("auto-resume-{}", request.id),
                                "run.auto_resume",
                                HashMap::new(),
                            )
                            .await;
                        }

                        // Push retry hint to agent
                        let parts: Vec<&str> = session_id.split(':').collect();
                        if parts.len() >= 2 {
                            let channel_id = parts[1];
                            let retry_hint = format!("📦 Package `{}` approved. You can now retry the previous operation.", package);
                            let _ = self
                                .session_registry
                                .push_message_to_session(
                                    session_id,
                                    channel_id.to_string(),
                                    retry_hint,
                                    "system".to_string(),
                                    None,
                                )
                                .await;
                        }
                    }

                    // Also handle EffectApproval - update capability status in marketplace
                    if let crate::approval::types::ApprovalCategory::EffectApproval {
                        ref capability_id,
                        ..
                    } = request.category
                    {
                        if request.status.is_approved() {
                            log::info!(
                                "[Gateway] Updating capability approval status for {}",
                                capability_id
                            );
                            let _ = self
                                .marketplace
                                .update_approval_status(
                                    capability_id.clone(),
                                    crate::capability_marketplace::types::ApprovalStatus::Approved,
                                )
                                .await;
                        }
                    }

                    // Handle SecretWrite - persist secret to SecretStore when approved
                    if let crate::approval::types::ApprovalCategory::SecretWrite {
                        ref key,
                        ref scope,
                        value,
                        ..
                    } = &request.category
                    {
                        if request.status.is_approved() {
                            if let Some(secret_value) = value {
                                log::info!(
                                    "[Gateway] Persisting approved secret '{}' to SecretStore",
                                    key
                                );
                                // Write to SecretStore
                                if let Err(e) =
                                    crate::secrets::SecretStore::set(key, secret_value, scope)
                                {
                                    log::error!(
                                        "[Gateway] Failed to persist secret '{}': {}",
                                        key,
                                        e
                                    );
                                } else {
                                    log::info!("[Gateway] Secret '{}' persisted successfully", key);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

impl GatewayState {
    async fn resolve_spawn_profile_for_session(&self, session_id: &str) -> Option<String> {
        if let Some(profile) = self
            .session_llm_profiles
            .read()
            .await
            .get(session_id)
            .cloned()
        {
            return Some(profile);
        }

        self.spawn_llm_profile.read().await.clone()
    }

    async fn send_chat_message(&self, session_id: &str, text: String) {
        let parts: Vec<&str> = session_id.split(':').collect();
        if parts.len() < 3 {
            return;
        }
        let channel_id = parts[1];

        let outbound = OutboundRequest {
            channel_id: channel_id.to_string(),
            content: text,
            reply_to: None,
            metadata: None,
        };

        if let Err(e) = self.connector.send(&self.connector_handle, outbound).await {
            log::error!(
                "[Gateway] Failed to send chat message to {}: {}",
                session_id,
                e
            );
        }
    }
    async fn nudge_user(&self, session_id: &str, approval_id: &str, category: &ApprovalCategory) {
        let approval_ui_url = &self.approval_ui_url;

        log::info!(
            "[Gateway] nudge_user: session_id={}, approval_id={}, category={:?}",
            session_id,
            approval_id,
            category
        );

        let message = match category {
            ApprovalCategory::SecretWrite {
                key, description, ..
            } => {
                format!("🔓 I need your approval to store a secret for **{}** ({}) .\n\nID: `{}`\n\nPlease visit the [Approval UI]({}/approvals) to complete this step.", key, description, approval_id, approval_ui_url)
            }
            ApprovalCategory::HumanActionRequest {
                title,
                skill_id,
                instructions,
                ..
            } => {
                let instructions_preview = if instructions.len() > 800 {
                    format!("{}...", &instructions[..800])
                } else {
                    instructions.clone()
                };
                format!(
                    "👋 Human intervention required for **{}** (Skill: {})\n\nID: `{}`\n\nInstructions:\n{}\n\nPlease visit the [Approval UI]({}/approvals) to proceed.",
                    title, skill_id, approval_id, instructions_preview, approval_ui_url
                )
            }
            ApprovalCategory::HttpHostApproval {
                host,
                port,
                requesting_url,
                ..
            } => {
                let port_str = port.map(|p| format!(":{}", p)).unwrap_or_default();
                format!(
                    "🌐 HTTP host approval required for **{}{}**\n\nURL: {}\n\nID: `{}`\n\nUse: `/approve {}`\n\nOr visit the [Approval UI]({}/approvals) to proceed.",
                    host, port_str, requesting_url, approval_id, approval_id, approval_ui_url
                )
            }
            ApprovalCategory::PackageApproval { package, runtime } => {
                format!(
                    "📦 Package approval required for **{}** ({})\n\nID: `{}`\n\nUse: `/approve {}`\n\nOr visit the [Approval UI]({}/approvals) to proceed.",
                    package, runtime, approval_id, approval_id, approval_ui_url
                )
            }
            _ => return,
        };

        // Resolve channel_id from session_id: "chat:channel_id:sender_id"
        let parts: Vec<&str> = session_id.split(':').collect();
        if parts.len() < 3 {
            return;
        }
        let channel_id = parts[1];

        let outbound = OutboundRequest {
            channel_id: channel_id.to_string(),
            content: message,
            reply_to: None,
            metadata: None,
        };

        if let Err(e) = self.connector.send(&self.connector_handle, outbound).await {
            log::error!(
                "[Gateway] Failed to send nudge to session {}: {}",
                session_id,
                e
            );
        }
    }

    pub(crate) async fn spawn_agent_for_run(
        &self,
        session_id: &str,
        run_id: &str,
        budget: Option<BudgetContext>,
    ) -> Result<(), StatusCode> {
        // Create SpawnConfig from budget
        let spawn_config = if let Some(ref b) = budget {
            let sc = SpawnConfig::new()
                .with_max_steps(b.max_steps)
                .with_max_duration_secs(b.max_duration_secs)
                .with_budget_policy("pause_approval");
            if let Some(mt) = b.max_tokens {
                sc.with_llm_max_tokens(mt as u32)
            } else {
                sc
            }
        } else {
            SpawnConfig::default()
        };

        let mut spawn_config = spawn_config.with_run_id(run_id);
        if let Some(profile) = self.resolve_spawn_profile_for_session(session_id).await {
            spawn_config = spawn_config.with_llm_profile(profile);
        }

        // C2: Query WorkingMemory for prior scheduled-run context to inject into agent spawn.
        // This gives the LLM agent awareness of the last run's goal and memory key so it uses
        // a consistent WorkingMemory key across recurring ticks.
        if let Ok(wm) = self.working_memory.lock() {
            use crate::working_memory::QueryParams;
            let params = QueryParams::with_tags([
                "scheduled-context".to_string(),
                format!("session:{}", session_id),
            ])
            .with_limit(Some(3));
            if let Ok(result) = wm.query(&params) {
                if !result.entries.is_empty() {
                    let ctx_lines: Vec<String> = result
                        .entries
                        .iter()
                        .map(|e| format!("[{}]: {}", e.title, e.content))
                        .collect();
                    // Extract all keys mentioned across the prior-run entries so we
                    // can give an explicit, unambiguous instruction to the next agent.
                    let known_keys: Vec<String> = result
                        .entries
                        .iter()
                        .flat_map(|e| {
                            // content: "run_id=X goal=Y memory_keys=[k1, k2]"
                            e.content
                                .split("memory_keys=[")
                                .nth(1)
                                .and_then(|s| s.split(']').next())
                                .map(|keys_str| {
                                    keys_str
                                        .split(',')
                                        .map(|k| k.trim().to_string())
                                        .filter(|k| !k.is_empty())
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default()
                        })
                        .collect();
                    let key_instruction = if known_keys.is_empty() {
                        "When using ccos.memory.get or ccos.memory.store (or ccos_sdk.memory), \
                        use the EXACT SAME key that was used in prior runs for this session."
                            .to_string()
                    } else {
                        // Deduplicate while preserving order
                        let mut seen = std::collections::HashSet::new();
                        let unique_keys: Vec<&String> = known_keys
                            .iter()
                            .filter(|k| seen.insert(k.as_str()))
                            .collect();
                        format!(
                            "CRITICAL: The WorkingMemory keys used in prior runs for this session \
                            are: [{}]. You MUST use these EXACT key names in \
                            ccos_sdk.memory.get / ccos_sdk.memory.store (or ccos.memory.get / \
                            ccos.memory.store). Do NOT invent new key names.",
                            unique_keys
                                .iter()
                                .map(|k| format!("\"{}\"", k))
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    };
                    let prior_context = format!(
                        "PRIOR RUN HISTORY (most recent scheduled runs for this session):\n{}\n{}",
                        ctx_lines.join("\n"),
                        key_instruction
                    );
                    spawn_config = spawn_config.with_prior_context(prior_context);
                }
            }
        }

        log::info!(
            "[Gateway] Spawning agent for run {} session {} (budget: steps={}, duration_secs={}, max_tokens={:?})",
            run_id,
            session_id,
            spawn_config.max_steps,
            spawn_config.max_duration_secs,
            spawn_config.llm_max_tokens
        );

        let token = self
            .session_registry
            .get_token(session_id)
            .await
            .ok_or(StatusCode::NOT_FOUND)?;

        SheriffLogger::log_agent(
            "SPAWN",
            session_id,
            &format!("Triggering agent spawn for run {}", run_id),
        );

        match self
            .spawner
            .spawn(session_id.to_string(), token, spawn_config)
            .await
        {
            Ok(spawn_result) => {
                if let Some(pid) = spawn_result.pid {
                    let _ = self.session_registry.set_agent_pid(session_id, pid).await;
                }

                let log_path = spawn_result
                    .log_path
                    .clone()
                    .unwrap_or_else(|| "none".to_string());

                SheriffLogger::log_spawn(run_id, session_id, spawn_result.pid, &log_path);

                SheriffLogger::log_agent(
                    "INFO",
                    session_id,
                    &format!("Agent for run {} started. {}", run_id, spawn_result.message),
                );
                Ok(())
            }
            Err(e) => {
                log::error!("[Gateway] Failed to spawn agent for run {}: {}", run_id, e);
                SheriffLogger::log_agent(
                    "SPAWN_FAILED",
                    session_id,
                    &format!("Failed to spawn agent for run {}: {}", run_id, e),
                );
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    }
}

/// Structured logger for the Gateway (Sheriff)
pub struct SheriffLogger;

impl SheriffLogger {
    fn truncate_for_log(value: &str, max_chars: usize) -> String {
        if value.chars().count() <= max_chars {
            return value.to_string();
        }
        let truncated: String = value.chars().take(max_chars.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }

    pub fn log_run(event: &str, run_id: &str, session_id: &str, details: &str) {
        println!(
            "{} {:<10} | {:<20} | run={:<12} session={:<32} | {}",
            Utc::now().format("%H:%M:%S"),
            "RUN",
            event,
            run_id.chars().take(8).collect::<String>(),
            session_id,
            details
        );
    }

    pub fn log_action(run_id: &str, capability_id: &str, inputs: &serde_json::Value) {
        let inputs_str = serde_json::to_string(inputs).unwrap_or_default();
        let truncated = Self::truncate_for_log(&inputs_str, 100);

        if capability_id == "ccos.run.create" {
            let summary = Self::summarize_run_create_program(inputs);
            println!(
                "{} {:<10} | {:<20} | run={:<12} | cap={:<24} {} inputs={}",
                Utc::now().format("%H:%M:%S"),
                "ACTION",
                "TRIGGER",
                run_id.chars().take(8).collect::<String>(),
                capability_id,
                summary,
                truncated
            );
            return;
        }

        println!(
            "{} {:<10} | {:<20} | run={:<12} | cap={:<24} inputs={}",
            Utc::now().format("%H:%M:%S"),
            "ACTION",
            "TRIGGER",
            run_id.chars().take(8).collect::<String>(),
            capability_id,
            truncated
        );
    }

    fn summarize_run_create_program(inputs: &serde_json::Value) -> String {
        let get_text = |key: &str, fallback: &str| {
            inputs
                .get(key)
                .and_then(|v| v.as_str())
                .unwrap_or(fallback)
                .to_string()
        };

        let goal = Self::truncate_for_log(&get_text("goal", "<missing>"), 80);
        let schedule = get_text("schedule", "none");
        let next_run_at = get_text("next_run_at", "none");
        let trigger_capability_id = get_text("trigger_capability_id", "none");
        let execution_mode = if trigger_capability_id != "none" {
            "capability_trigger"
        } else {
            "llm_agent"
        };
        let max_run = inputs
            .get("max_run")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "none".to_string());
        let budget = inputs
            .get("budget")
            .map(|v| Self::truncate_for_log(&v.to_string(), 80))
            .unwrap_or_else(|| "none".to_string());

        format!(
            "program=goal=\"{}\" schedule={} next_run_at={} mode={} trigger={} max_run={} budget={}",
            goal, schedule, next_run_at, execution_mode, trigger_capability_id, max_run, budget
        )
    }

    pub fn log_result(
        run_id: &str,
        capability_id: &str,
        success: bool,
        result: &serde_json::Value,
    ) {
        let res_str = serde_json::to_string(result).unwrap_or_default();
        let truncated = Self::truncate_for_log(&res_str, 512);
        let status = if success {
            "✅ SUCCESS"
        } else {
            "❌ FAILURE"
        };
        println!(
            "{} {:<10} | {:<20} | run={:<12} | cap={:<24} status={} result={}",
            Utc::now().format("%H:%M:%S"),
            "RESULT",
            "COMPLETE",
            run_id.chars().take(8).collect::<String>(),
            capability_id,
            status,
            truncated
        );
    }

    pub fn log_budget(run_id: &str, reason: &str, details: &str) {
        println!(
            "{} {:<10} | {:<20} | run={:<12} | reason={:<16} | {}",
            Utc::now().format("%H:%M:%S"),
            "BUDGET",
            "LIMIT",
            run_id.chars().take(8).collect::<String>(),
            reason,
            details
        );
    }

    pub fn log_agent(event: &str, session_id: &str, details: &str) {
        println!(
            "{} {:<10} | {:<20} | session={:<32} | {}",
            Utc::now().format("%H:%M:%S"),
            "AGENT",
            event,
            session_id,
            details
        );
    }

    pub fn log_spawn(run_id: &str, _session_id: &str, pid: Option<u32>, log_path: &str) {
        let pid_str = pid
            .map(|p| p.to_string())
            .unwrap_or_else(|| "none".to_string());
        println!(
            "{} {:<10} | {:<20} | run={:<12} | pid={:<8} logs={}",
            Utc::now().format("%H:%M:%S"),
            "AGENT",
            "SPAWNED",
            run_id.chars().take(8).collect::<String>(),
            pid_str,
            log_path
        );
    }
}

impl ChatGateway {
    pub async fn start(config: ChatGatewayConfig) -> RuntimeResult<()> {
        let key = QuarantineKey::from_env(&config.quarantine_key_env)?;
        let quarantine = Arc::new(FileQuarantineStore::new(
            config.quarantine_dir.clone(),
            key,
            chrono::Duration::seconds(config.quarantine_default_ttl_seconds),
        )?) as Arc<dyn QuarantineStore>;

        let chain = Arc::new(Mutex::new(CausalChain::new()?));
        let approvals = Some(UnifiedApprovalQueue::new(Arc::new(
            FileApprovalStorage::new(config.approvals_dir.clone()).map_err(|e| {
                RuntimeError::Generic(format!("Failed to init approval storage: {}", e))
            })?,
        )));

        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        {
            // Configure the governed HTTP egress boundary before any capability execution.
            let allow_hosts = if config.http_allow_hosts.is_empty() {
                vec!["localhost".to_string(), "127.0.0.1".to_string()]
            } else {
                config.http_allow_hosts.clone()
            };
            let mut guard = registry.write().await;
            log::debug!(
                "[Gateway] Setting HTTP allowlist: hosts={:?}, ports={:?}",
                allow_hosts,
                config.http_allow_ports
            );
            guard.set_http_allow_hosts(allow_hosts)?;
            guard.set_http_allow_ports(config.http_allow_ports.clone())?;
        }
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));

        // Wire a CatalogService into the Gateway's marketplace so that
        // ccos.skill.search can discover local skills.
        {
            let catalog_service = Arc::new(crate::catalog::CatalogService::new());
            marketplace
                .set_catalog_service(Arc::clone(&catalog_service))
                .await;

            // Bootstrap skill discovery immediately so the catalog is populated.
            let skills_path =
                std::env::var("CCOS_SKILLS_PATH").unwrap_or_else(|_| "skills".to_string());
            let skills_dir = std::path::PathBuf::from(&skills_path);
            if skills_dir.exists() {
                let skill_agent =
                    crate::capability_marketplace::discovery::LocalSkillDiscoveryAgent::new(
                        skills_dir,
                    );
                match skill_agent.discover(Some(Arc::clone(&marketplace))).await {
                    Ok(_) => log::info!("[Gateway] LocalSkillDiscoveryAgent discovery complete"),
                    Err(e) => log::warn!("[Gateway] LocalSkillDiscoveryAgent failed: {:?}", e),
                }
            } else {
                log::info!(
                    "[Gateway] CCOS_SKILLS_PATH={} does not exist, skipping skill discovery",
                    skills_path
                );
            }
        }

        // Configure approval store for capability approval tracking
        let approval_store_path = config.approvals_dir.join("capability_approvals.json");
        if let Err(e) = marketplace
            .configure_approval_store(&approval_store_path)
            .await
        {
            log::warn!(
                "[Gateway] Failed to configure approval store at {}: {}. \
                Capability approvals will not be persisted.",
                approval_store_path.display(),
                e
            );
        }

        let connector = Arc::new(LoopbackWebhookConnector::new(
            config.connector.clone(),
            quarantine.clone(),
        ));
        let handle = connector.connect().await?;

        // Determine persistence paths for runs and sessions (under .ccos/ in cwd)
        let ccos_state_dir = std::env::current_dir().unwrap_or_default().join(".ccos");
        if let Err(e) = std::fs::create_dir_all(&ccos_state_dir) {
            log::warn!("[Gateway] Failed to create .ccos state dir: {}", e);
        }
        let runs_persist_path = ccos_state_dir.join("runs.json");
        let sessions_persist_path = ccos_state_dir.join("sessions.json");
        log::info!(
            "[Gateway] Persistence paths: runs={:?} sessions={:?}",
            runs_persist_path,
            sessions_persist_path
        );

        // Rebuild stores from causal chain (restart-safe autonomy).
        // These stores are also shared with chat capabilities (resource ingestion).
        let run_store: SharedRunStore = Arc::new(Mutex::new(RunStore::new()));
        let resource_store: SharedResourceStore = Arc::new(Mutex::new(ResourceStore::new()));
        {
            let guard = chain.lock().unwrap();
            let actions = guard.get_all_actions();
            if let Ok(mut store) = run_store.lock() {
                // Load from JSON first (survives restarts); fall back to causal-chain replay
                if let Some(loaded) = RunStore::load_from_disk(&runs_persist_path) {
                    *store = loaded;
                } else {
                    *store = RunStore::rebuild_from_chain(&actions);
                    log::info!("[Gateway] No persisted run state found; rebuilt from causal chain");
                }
                store.set_persist_path(runs_persist_path.clone());
            }
            if let Ok(mut store) = resource_store.lock() {
                *store = ResourceStore::rebuild_from_chain(&actions);
            }
        }

        // Generate internal secret for native capabilities
        let internal_api_secret = uuid::Uuid::new_v4().to_string();

        // Register minimal non-chat primitives needed by autonomy flows.
        // We keep this intentionally small (no full marketplace bootstrap) to preserve
        // the chat security contract while still enabling governed resource ingestion.
        crate::capabilities::network::register_network_capabilities(
            &*marketplace,
            approvals.clone(),
            Some(internal_api_secret.clone()),
        )
        .await?;

        // Generate internal secret for native capabilities
        register_chat_capabilities(
            marketplace.clone(),
            quarantine.clone(),
            chain.clone(),
            approvals.clone(),
            resource_store.clone(),
            Some(connector.clone() as Arc<dyn ChatConnector>),
            Some(handle.clone()),
            Some(format!("http://{}", config.bind_addr)),
            Some(internal_api_secret.clone()),
            crate::config::types::SandboxConfig::default(),
            crate::config::types::CodingAgentsConfig::default(),
        )
        .await?;

        // Default in-memory WorkingMemory — overridden with persistent backend inside the
        // approvals block below when a persistent path is available.
        let mut working_memory_arc = Arc::new(Mutex::new(
            crate::working_memory::WorkingMemory::new(Box::new(
                crate::working_memory::InMemoryJsonlBackend::new(None, None, None),
            )),
        ));

        // Register onboarding capabilities (approval + secrets + onboarding memory)
        // so chat agents can explicitly request human approvals during execution.
        if let Some(ref approvals_queue) = approvals {
            let secret_store = match crate::secrets::SecretStore::new(std::env::current_dir().ok())
            {
                Ok(store) => store,
                Err(e) => {
                    log::warn!(
                        "[Gateway] Failed to initialize SecretStore from cwd: {}. Falling back to in-memory-only secret staging.",
                        e
                    );
                    crate::secrets::SecretStore::empty()
                }
            };
            let wm_jsonl_path = std::env::var("CCOS_WORKING_MEMORY_PATH")
                .ok()
                .filter(|p| !p.trim().is_empty())
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::current_dir()
                        .ok()
                        .map(|cwd| cwd.join(".ccos").join("working_memory.jsonl"))
                });
            let wm_backend_path = wm_jsonl_path.and_then(|path| {
                if let Some(parent) = path.parent() {
                    if let Err(e) = std::fs::create_dir_all(parent) {
                        log::warn!(
                            "[Gateway] Failed to create working memory directory {}: {}. Falling back to in-memory working memory.",
                            parent.display(),
                            e
                        );
                        return None;
                    }
                }
                Some(path)
            });

            let working_memory = crate::working_memory::WorkingMemory::new(Box::new(
                crate::working_memory::InMemoryJsonlBackend::new(
                    wm_backend_path.clone(),
                    None,
                    None,
                ),
            ));
            working_memory_arc = Arc::new(Mutex::new(working_memory));
            if let Some(path) = wm_backend_path {
                log::info!(
                    "[Gateway] Working memory persistence enabled at {}",
                    path.display()
                );
            }

            crate::skills::onboarding_capabilities::register_onboarding_capabilities(
                marketplace.clone(),
                Arc::new(Mutex::new(secret_store)),
                working_memory_arc.clone(),
                Arc::new(approvals_queue.clone()),
            )
            .await?;

            // Sanity check: request_human_action must be available to agents.
            if marketplace
                .get_manifest("ccos.approval.request_human_action")
                .await
                .is_none()
            {
                log::warn!(
                    "[Gateway] Capability ccos.approval.request_human_action is not registered"
                );
            }
        }

        // Initialize session management with persistence so sessions survive gateway restarts
        let session_registry = SessionRegistry::new_with_persistence(&sessions_persist_path);
        session_registry.load_from_disk().await;
        let spawner = SpawnerFactory::create();

        // Initialize real-time tracking sink
        let realtime_sink = Arc::new(RealTimeTrackingSink::new(100));

        // Register realtime sink with Causal Chain
        {
            let mut chain_guard = chain.lock().unwrap();
            chain_guard.register_event_sink(realtime_sink.clone());
        }

        let state = Arc::new(GatewayState {
            marketplace: marketplace.clone(),
            quarantine: quarantine.clone(),
            chain: chain.clone(),
            _approvals: approvals.clone(),
            connector: connector.clone(),
            connector_handle: handle.clone(),
            inbox: Mutex::new(VecDeque::new()),
            policy_pack_version: config.policy_pack_version.clone(),
            session_registry,
            spawner: spawner.into(),
            run_store: run_store.clone(),
            resource_store,
            scheduler: Arc::new(Scheduler::new(run_store.clone())),
            checkpoint_store: Arc::new(InMemoryCheckpointStore::new()),
            internal_api_secret,
            realtime_sink,
            admin_tokens: {
                let mut tokens = config.admin_tokens.clone();
                if !tokens.contains(&config.connector.shared_secret) {
                    tokens.push(config.connector.shared_secret.clone());
                }
                tokens
            },
            approval_ui_url: config.approval_ui_url.clone(),
            spawn_llm_profile: Arc::new(RwLock::new(resolve_default_spawn_llm_profile())),
            session_llm_profiles: Arc::new(RwLock::new(HashMap::new())),
            working_memory: working_memory_arc,
        });

        // Register gateway as approval consumer
        if let Some(ref approvals_queue) = approvals {
            approvals_queue.add_consumer(state.clone()).await;
        }

        let gateway = ChatGateway {
            state: state.clone(),
        };

        // Start background scheduler
        let scheduler_state = state.clone();
        let scheduler = state.scheduler.clone();
        tokio::spawn(async move {
            scheduler.start(scheduler_state).await;
        });

        // Start agent health monitor
        let monitor_chain = state.chain.clone();
        let monitor_registry = state.session_registry.clone();
        let monitor_sink = state.realtime_sink.clone();
        // Use default config values (could be loaded from config file)
        let health_check_interval = 5u64;
        let heartbeat_timeout = 3u64;
        tokio::spawn(async move {
            let monitor = AgentMonitor::new(
                health_check_interval,
                heartbeat_timeout,
                monitor_chain,
                monitor_registry,
                monitor_sink,
            );
            monitor.run().await;
        });

        connector
            .subscribe(
                &handle,
                Arc::new(move |envelope| {
                    let gateway = gateway.clone();
                    Box::pin(async move { gateway.handle_inbound(envelope).await })
                }),
            )
            .await?;

        let app_state = state.clone();
        let router = Router::new()
            .route("/chat/health", get(health_handler))
            .route("/chat/inbox", get(inbox_handler))
            .route("/chat/execute", post(execute_handler))
            .route("/chat/send", post(send_handler))
            .route("/chat/capabilities", get(list_capabilities_handler))
            .route("/chat/audit", get(audit_handler))
            .route("/chat/events/:session_id", get(events_handler))
            .route("/chat/stream/:session_id", get(event_stream_handler))
            .route("/chat/heartbeat/:session_id", post(heartbeat_handler))
            .route("/chat/session/:session_id", get(get_session_handler))
            .route("/chat/sessions", get(list_sessions_handler))
            .route(
                "/chat/admin/llm-profile",
                get(get_spawn_llm_profile_handler).post(set_spawn_llm_profile_handler),
            )
            .route("/chat/agent/log", post(agent_log_handler))
            .route("/chat/run", post(create_run_handler).get(list_runs_handler))
            .route("/chat/run/:run_id", get(get_run_handler))
            .route("/chat/run/:run_id/actions", get(list_run_actions_handler))
            .route("/chat/run/:run_id/cancel", post(cancel_run_handler))
            .route("/chat/run/:run_id/pause", post(pause_run_handler))
            .route("/chat/run/:run_id/transition", post(transition_run_handler))
            .route("/chat/run/:run_id/resume", post(resume_run_handler))
            .route("/chat/run/:run_id/checkpoint", post(checkpoint_run_handler))
            .with_state(app_state);

        let listener = TcpListener::bind(config.bind_addr.as_str())
            .await
            .map_err(|e| RuntimeError::Generic(format!("Gateway bind error: {}", e)))?;
        axum::serve(listener, router.into_make_service())
            .await
            .map_err(|e| RuntimeError::Generic(format!("Gateway server error: {}", e)))?;

        Ok(())
    }

    async fn handle_inbound(&self, mut envelope: MessageEnvelope) -> RuntimeResult<()> {
        let session_id = format!("chat:{}:{}", envelope.channel_id, envelope.sender_id);
        SheriffLogger::log_agent(
            "INBOUND",
            &session_id,
            &format!("Message {} from {}", envelope.id, envelope.sender_id),
        );
        // If there's an active run for this session, correlate inbound messages to it.
        // If a run is paused waiting for an external event, auto-resume it on inbound message.
        // Otherwise fall back to an ephemeral run id for audit correlation.
        let run_id = {
            let mut chosen: Option<String> = None;
            if let Ok(mut store) = self.state.run_store.lock() {
                if let Some(active) = store
                    .get_active_run_for_session(&session_id)
                    .map(|r| r.id.clone())
                {
                    chosen = Some(active);
                } else if let Some(paused_id) =
                    store.get_latest_paused_external_run_id_for_session(&session_id)
                {
                    // Resume automatically.
                    let _ = store.update_run_state(&paused_id, crate::chat::run::RunState::Active);
                    chosen = Some(paused_id);
                } else if let Some(checkpoint_id) =
                    store.get_latest_checkpoint_run_id_for_session(&session_id)
                {
                    // Resume automatically from checkpoint.
                    let _ =
                        store.update_run_state(&checkpoint_id, crate::chat::run::RunState::Active);
                    chosen = Some(checkpoint_id);
                } else if let Some(paused_approval_id) =
                    store.get_latest_paused_approval_run_id_for_session(&session_id)
                {
                    // Correlate without auto-resuming (requires explicit approval/resume).
                    chosen = Some(paused_approval_id);
                }
            }
            chosen.unwrap_or_else(|| {
                let id = format!("chat-run-{}", Utc::now().timestamp_millis());
                // Register ephemeral run in store so it's "gettable"
                if let Ok(mut store) = self.state.run_store.lock() {
                    let mut run = Run::new_ephemeral(
                        session_id.clone(),
                        "Chat interaction".to_string(),
                        None,
                    );
                    run.id = id.clone();
                    store.create_run(run);
                }
                id
            })
        };
        let trigger = envelope
            .activation
            .as_ref()
            .and_then(|a| a.trigger.clone())
            .unwrap_or_else(|| "none".to_string());
        log::debug!(
            "[Gateway] Inbound message {} correlated to run {} (session={}, trigger={})",
            envelope.id,
            run_id,
            session_id,
            trigger
        );
        let step_id = format!("message-ingest-{}", envelope.id);
        envelope.session_id = Some(session_id.clone());
        envelope.run_id = Some(run_id.clone());
        envelope.step_id = Some(step_id.clone());
        envelope.direction = MessageDirection::Inbound;

        let mut meta = HashMap::new();
        meta.insert("message_id".to_string(), Value::String(envelope.id.clone()));
        meta.insert(
            "channel_id".to_string(),
            Value::String(envelope.channel_id.clone()),
        );
        meta.insert(
            "sender_id".to_string(),
            Value::String(envelope.sender_id.clone()),
        );
        meta.insert(
            "message_timestamp".to_string(),
            Value::String(envelope.timestamp.clone()),
        );
        meta.insert(
            "policy_pack_version".to_string(),
            Value::String(self.state.policy_pack_version.clone()),
        );
        meta.insert(
            "rule_id".to_string(),
            Value::String("chat.message.ingest".to_string()),
        );
        if let Some(activation) = &envelope.activation {
            meta.insert(
                "activation_trigger".to_string(),
                Value::String(activation.trigger.clone().unwrap_or_default()),
            );
        }

        record_run_event(
            &self.state,
            &session_id,
            &run_id,
            &step_id,
            "message.ingest",
            meta,
        )
        .await?;
        log::debug!("[Gateway] Recorded message.ingest event for run {}", run_id);

        // Hybrid session handling: Get or create session with smart agent management
        let (session, is_new_session) = self
            .state
            .session_registry
            .get_or_create_session(&session_id)
            .await;
        let token = session.auth_token.clone();

        // Determine if we need to spawn an agent
        let needs_agent_spawn = if is_new_session {
            SheriffLogger::log_agent(
                "SESSION",
                &session_id,
                &format!(
                    "Created new session for channel {} sender {}",
                    envelope.channel_id, envelope.sender_id
                ),
            );
            true
        } else {
            // Existing session - check if agent is running
            let agent_running = session.is_agent_running();
            if !agent_running {
                SheriffLogger::log_agent(
                    "SESSION",
                    &session_id,
                    "Reconnected to session but agent not running - will respawn",
                );
                // Update status to indicate agent is not running
                self.state
                    .session_registry
                    .update_session_status(
                        &session_id,
                        crate::chat::session::SessionStatus::AgentNotRunning,
                    )
                    .await;
                true
            } else {
                SheriffLogger::log_agent(
                    "SESSION",
                    &session_id,
                    &format!(
                        "Reconnected to session with agent running (PID {:?})",
                        session.agent_pid
                    ),
                );
                false
            }
        };

        if needs_agent_spawn {
            SheriffLogger::log_agent(
                if is_new_session { "SPAWN" } else { "RESPAWN" },
                &session_id,
                if is_new_session {
                    "Triggering agent spawn for new session"
                } else {
                    "Triggering agent respawn for existing session"
                },
            );

            let mut spawn_config = SpawnConfig::default();
            if let Some(profile) = self
                .state
                .resolve_spawn_profile_for_session(&session_id)
                .await
            {
                spawn_config = spawn_config.with_llm_profile(profile);
            }

            match self
                .state
                .spawner
                .spawn(session.session_id.clone(), token.clone(), spawn_config)
                .await
            {
                Ok(spawn_result) => {
                    if let Some(pid) = spawn_result.pid {
                        let _ = self
                            .state
                            .session_registry
                            .set_agent_pid(&session.session_id, pid)
                            .await;
                    }

                    // Update status to Active since agent is now running
                    self.state
                        .session_registry
                        .update_session_status(
                            &session_id,
                            crate::chat::session::SessionStatus::Active,
                        )
                        .await;

                    let log_path = spawn_result
                        .log_path
                        .clone()
                        .unwrap_or_else(|| "none".to_string());

                    SheriffLogger::log_spawn(
                        &run_id,
                        &session.session_id,
                        spawn_result.pid,
                        &log_path,
                    );

                    SheriffLogger::log_agent(
                        "INFO",
                        &session.session_id,
                        &format!(
                            "{} agent started. {}",
                            if is_new_session { "New" } else { "Respawned" },
                            spawn_result.message
                        ),
                    );
                }
                Err(e) => {
                    SheriffLogger::log_agent(
                        "SPAWN_FAILED",
                        &session_id,
                        &format!("Failed to spawn legacy agent: {}", e),
                    );
                }
            }
        }

        // Resolve content from quarantine (transparent to agent)
        let content_to_push = match self.state.quarantine.get_bytes(&envelope.content_ref) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(s) => s,
                Err(_) => {
                    log::warn!(
                        "[Gateway] Content for {} is not valid UTF-8, sending ref instead",
                        envelope.id
                    );
                    envelope.content_ref.clone()
                }
            },
            Err(e) => {
                log::warn!(
                    "[Gateway] Failed to resolve content for {}: {}, sending ref instead",
                    envelope.id,
                    e
                );
                envelope.content_ref.clone()
            }
        };
        log::debug!(
            "[Gateway] Resolved inbound content for {} (len={}, ref_fallback={})",
            envelope.id,
            content_to_push.len(),
            content_to_push == envelope.content_ref
        );

        // Handle Slash Commands
        if content_to_push.starts_with('/') {
            if content_to_push.starts_with("/approve ") {
                let id = content_to_push["/approve ".len()..].trim();
                let result = if let Some(ref queue) = self.state._approvals {
                    queue
                        .approve(
                            id,
                            crate::approval::queue::ApprovalAuthority::User(
                                envelope.sender_id.clone(),
                            ),
                            Some("Approved via chat command".to_string()),
                        )
                        .await
                } else {
                    Err(RuntimeError::Generic(
                        "Approval system not initialized".to_string(),
                    ))
                };

                match result {
                    Ok(_) => {}
                    Err(e) => {
                        self.state
                            .send_chat_message(&session_id, format!("❌ Approval failed: {}", e))
                            .await;
                    }
                }
                return Ok(()); // Command handled, don't push to agent
            }
            if content_to_push.starts_with("/session") {
                let session_name = content_to_push
                    .strip_prefix("/session")
                    .unwrap_or("")
                    .trim();

                let new_channel_id = if session_name.is_empty() {
                    // Generate a new session id with current date
                    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
                } else {
                    session_name.to_string()
                };

                let target_session_id = format!("chat:{}:{}", new_channel_id, envelope.sender_id);

                // Create the new session if it doesn't exist yet
                if self
                    .state
                    .session_registry
                    .get_session(&target_session_id)
                    .await
                    .is_none()
                {
                    let _ = self
                        .state
                        .session_registry
                        .create_session(Some(target_session_id.clone()))
                        .await;
                }

                self.state
                    .send_chat_message(
                        &session_id,
                        format!("🔄 Run this command in a new terminal to join the session:\n\n    cargo run --bin ccos-chat -- --channel-id {} --user-id {}\n", new_channel_id, envelope.sender_id),
                    )
                    .await;

                return Ok(());
            }
            if content_to_push.starts_with("/model") {
                let profile = content_to_push
                    .strip_prefix("/model")
                    .map(|s| s.trim())
                    .unwrap_or("");

                if profile.is_empty() {
                    self.state
                        .send_chat_message(
                            &session_id,
                            "Usage: /model <profile-name> (example: /model openai:gpt-4.1)\nUse Tab for autocomplete."
                                .to_string(),
                        )
                        .await;
                    return Ok(());
                }

                {
                    let mut guard = self.state.session_llm_profiles.write().await;
                    guard.insert(session_id.clone(), profile.to_string());
                }

                let mut old_pid: Option<u32> = None;
                let mut token: Option<String> = None;
                if let Some(session) = self.state.session_registry.get_session(&session_id).await {
                    old_pid = session.agent_pid;
                    token = Some(session.auth_token.clone());

                    let mut updated = session;
                    updated.agent_pid = None;
                    updated.status = crate::chat::session::SessionStatus::AgentNotRunning;
                    self.state.session_registry.update_session(updated).await;
                }

                if let Some(pid) = old_pid {
                    let _ = std::process::Command::new("kill")
                        .arg("-TERM")
                        .arg(pid.to_string())
                        .status();
                }

                let restart_result = if let Some(auth_token) = token {
                    let spawn_config = SpawnConfig::default().with_llm_profile(profile.to_string());
                    self.state
                        .spawner
                        .spawn(session_id.clone(), auth_token, spawn_config)
                        .await
                } else {
                    Err(RuntimeError::Generic(
                        "Session not found while applying model switch".to_string(),
                    ))
                };

                match restart_result {
                    Ok(spawn_result) => {
                        if let Some(new_pid) = spawn_result.pid {
                            let _ = self
                                .state
                                .session_registry
                                .set_agent_pid(&session_id, new_pid)
                                .await;
                        }
                        self.state
                            .session_registry
                            .update_session_status(
                                &session_id,
                                crate::chat::session::SessionStatus::Active,
                            )
                            .await;

                        self.state
                            .send_chat_message(
                                &session_id,
                                format!(
                                    "✅ Model switched to '{}' and agent restarted (pid={:?})",
                                    profile, spawn_result.pid
                                ),
                            )
                            .await;
                    }
                    Err(e) => {
                        self.state
                            .send_chat_message(
                                &session_id,
                                format!(
                                    "⚠️ Model profile set to '{}' but agent restart failed: {}",
                                    profile, e
                                ),
                            )
                            .await;
                    }
                }

                return Ok(());
            }
            // Add more commands here (e.g., /reject, /status)
        }

        // Add message to session inbox
        if let Err(e) = self
            .state
            .session_registry
            .push_message_to_session(
                &session_id,
                envelope.channel_id.clone(),
                content_to_push,
                envelope.sender_id.clone(),
                envelope.run_id.clone(),
            )
            .await
        {
            log::error!(
                "[Gateway] Failed to push message to session {} for run {:?}: {}",
                session_id,
                envelope.run_id,
                e
            );
            return Err(RuntimeError::Generic(e));
        } else {
            log::debug!(
                "[Gateway] Pushed message {} to session inbox {}",
                envelope.id,
                session_id
            );
        }

        let mut guard = self
            .state
            .inbox
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock inbox".to_string()))?;
        guard.push_back(envelope);
        Ok(())
    }

    async fn execute_capability(
        &self,
        capability_id: &str,
        inputs: serde_json::Value,
    ) -> RuntimeResult<(serde_json::Value, bool)> {
        let rtfs_inputs = json_to_rtfs_value(&inputs)?;

        let rtfs_args: Vec<Value> = if let Value::Map(map) = &rtfs_inputs {
            map.values().cloned().collect()
        } else {
            vec![rtfs_inputs.clone()]
        };

        let session_id = inputs
            .get("session_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let run_id = inputs
            .get("run_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let step_id = inputs
            .get("step_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let plan_id = format!("chat-plan-{}", Utc::now().timestamp_millis());
        let intent_id = session_id.clone().unwrap_or_else(|| plan_id.clone());

        let action = match self.state.chain.lock() {
            Ok(mut chain) => {
                let mut meta = HashMap::new();
                if let Some(rid) = &run_id {
                    meta.insert("run_id".to_string(), Value::String(rid.clone()));
                }
                if let Some(sid) = &step_id {
                    meta.insert("step_id".to_string(), Value::String(sid.clone()));
                }
                // Include source code for execution capabilities so monitor can render
                // launch events with language + code before result arrives.
                let exec_language = match capability_id {
                    "ccos.execute.python" | "ccos.execution.python" | "ccos.sandbox.python" => {
                        Some("python")
                    }
                    "ccos.execute.javascript"
                    | "ccos.execution.javascript"
                    | "ccos.sandbox.javascript" => Some("javascript"),
                    "ccos.execute.rtfs" | "ccos.execution.rtfs" => Some("rtfs"),
                    _ => None,
                };
                if let (Some(language), Some(code)) =
                    (exec_language, inputs.get("code").and_then(|v| v.as_str()))
                {
                    meta.insert("code".to_string(), Value::String(code.to_string()));
                    meta.insert("language".to_string(), Value::String(language.to_string()));
                }

                if capability_id == "ccos.run.create" {
                    if let Some(goal) = inputs.get("goal").and_then(|v| v.as_str()) {
                        meta.insert(
                            "run_create_goal".to_string(),
                            Value::String(goal.to_string()),
                        );
                    }
                    if let Some(target_session_id) =
                        inputs.get("session_id").and_then(|v| v.as_str())
                    {
                        meta.insert(
                            "run_create_session_id".to_string(),
                            Value::String(target_session_id.to_string()),
                        );
                    }
                    if let Some(schedule) = inputs.get("schedule").and_then(|v| v.as_str()) {
                        meta.insert(
                            "run_create_schedule".to_string(),
                            Value::String(schedule.to_string()),
                        );
                    }
                    if let Some(next_run_at) = inputs.get("next_run_at").and_then(|v| v.as_str()) {
                        meta.insert(
                            "run_create_next_run_at".to_string(),
                            Value::String(next_run_at.to_string()),
                        );
                    }
                    if let Some(max_run) = inputs.get("max_run").and_then(|v| v.as_i64()) {
                        meta.insert("run_create_max_run".to_string(), Value::Integer(max_run));
                    } else if let Some(max_run) = inputs.get("max_run").and_then(|v| v.as_u64()) {
                        meta.insert(
                            "run_create_max_run".to_string(),
                            Value::Integer(max_run.min(i64::MAX as u64) as i64),
                        );
                    }
                    if let Some(budget_json) = inputs.get("budget") {
                        if let Ok(budget_value) = json_to_rtfs_value(budget_json) {
                            meta.insert("run_create_budget".to_string(), budget_value);
                        }
                    }
                    let trigger_capability_id = inputs
                        .get("trigger_capability_id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string());
                    let execution_mode = if trigger_capability_id.is_some() {
                        "capability_trigger"
                    } else {
                        "llm_agent"
                    };
                    meta.insert(
                        "run_create_execution_mode".to_string(),
                        Value::String(execution_mode.to_string()),
                    );
                    if let Some(trigger_capability_id) = trigger_capability_id {
                        meta.insert(
                            "run_create_trigger_capability_id".to_string(),
                            Value::String(trigger_capability_id),
                        );
                    }
                    if let Some(trigger_inputs_json) = inputs.get("trigger_inputs") {
                        if let Ok(trigger_inputs_value) = json_to_rtfs_value(trigger_inputs_json) {
                            meta.insert(
                                "run_create_trigger_inputs".to_string(),
                                trigger_inputs_value,
                            );
                        }
                    }
                    if let Some(parent_run_id) = inputs.get("run_id").and_then(|v| v.as_str()) {
                        meta.insert(
                            "run_create_parent_run_id".to_string(),
                            Value::String(parent_run_id.to_string()),
                        );
                    }
                } else if capability_id == "ccos.memory.store" {
                    meta.insert(
                        "memory_operation".to_string(),
                        Value::String("store".to_string()),
                    );
                    if let Some(memory_key) = inputs.get("key").and_then(|v| v.as_str()) {
                        meta.insert(
                            "memory_key".to_string(),
                            Value::String(memory_key.to_string()),
                        );
                    }
                    if let Some(value_json) = inputs.get("value") {
                        if let Ok(value_rtfs) = json_to_rtfs_value(value_json) {
                            meta.insert("memory_store_value".to_string(), value_rtfs);
                        }
                    }
                } else if capability_id == "ccos.memory.get" {
                    meta.insert(
                        "memory_operation".to_string(),
                        Value::String("get".to_string()),
                    );
                    if let Some(memory_key) = inputs.get("key").and_then(|v| v.as_str()) {
                        meta.insert(
                            "memory_key".to_string(),
                            Value::String(memory_key.to_string()),
                        );
                    }
                }

                chain
                    .log_capability_call_with_metadata(
                        session_id.as_deref(),
                        &plan_id,
                        &intent_id,
                        &capability_id.to_string(),
                        capability_id,
                        rtfs_args,
                        meta,
                    )
                    .ok()
            }
            Err(_) => None,
        };

        let cap_id = capability_id.to_string();
        let exec_result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                self.state
                    .marketplace
                    .execute_capability(&cap_id, &rtfs_inputs)
                    .await
            })
        });

        let (result_json, success, rtfs_result) = match exec_result {
            Ok(value) => (rtfs_value_to_json(&value)?, true, Some(value)),
            Err(e) => {
                log::error!("[Gateway] Capability {} failed: {}", capability_id, e);
                (
                    serde_json::json!({
                        "error": format!("{}", e),
                        "capability_id": capability_id,
                    }),
                    false,
                    None,
                )
            }
        };

        if let Some(action) = action {
            let mut result_metadata = HashMap::new();
            if capability_id == "ccos.memory.store" {
                result_metadata.insert(
                    "memory_operation".to_string(),
                    Value::String("store".to_string()),
                );
                if let Some(memory_key) = inputs.get("key").and_then(|v| v.as_str()) {
                    result_metadata.insert(
                        "memory_key".to_string(),
                        Value::String(memory_key.to_string()),
                    );
                }
                if let Some(value_json) = inputs.get("value") {
                    if let Ok(value_rtfs) = json_to_rtfs_value(value_json) {
                        result_metadata.insert("memory_store_value".to_string(), value_rtfs);
                    }
                }
                if let Some(entry_id) = result_json.get("entry_id").and_then(|v| v.as_str()) {
                    result_metadata.insert(
                        "memory_store_entry_id".to_string(),
                        Value::String(entry_id.to_string()),
                    );
                }
                if let Some(store_success) = result_json.get("success").and_then(|v| v.as_bool()) {
                    result_metadata.insert(
                        "memory_store_success".to_string(),
                        Value::Boolean(store_success),
                    );
                }
            } else if capability_id == "ccos.memory.get" {
                result_metadata.insert(
                    "memory_operation".to_string(),
                    Value::String("get".to_string()),
                );
                if let Some(memory_key) = inputs.get("key").and_then(|v| v.as_str()) {
                    result_metadata.insert(
                        "memory_key".to_string(),
                        Value::String(memory_key.to_string()),
                    );
                }
                if let Some(found) = result_json.get("found").and_then(|v| v.as_bool()) {
                    result_metadata.insert("memory_get_found".to_string(), Value::Boolean(found));
                }
                if let Some(expired) = result_json.get("expired").and_then(|v| v.as_bool()) {
                    result_metadata
                        .insert("memory_get_expired".to_string(), Value::Boolean(expired));
                }
                if let Some(value_json) = result_json.get("value") {
                    if let Ok(value_rtfs) = json_to_rtfs_value(value_json) {
                        result_metadata.insert("memory_get_value".to_string(), value_rtfs);
                    }
                }
            }
            if !success {
                if let Some(err) = result_json.get("error").and_then(|v| v.as_str()) {
                    result_metadata.insert("error".to_string(), Value::String(err.to_string()));
                }
                if let Some(cap_id) = result_json.get("capability_id").and_then(|v| v.as_str()) {
                    result_metadata.insert(
                        "capability_id".to_string(),
                        Value::String(cap_id.to_string()),
                    );
                }
            }
            let exec_result = crate::types::ExecutionResult {
                success,
                value: rtfs_result.unwrap_or(Value::Nil),
                metadata: result_metadata,
            };
            if let Ok(mut chain) = self.state.chain.lock() {
                let _ = chain.record_result(action, exec_result);
            }

            // Evaluate completion trigger
            if let (Some(sid), Some(rid)) = (&session_id, &run_id) {
                let _ = evaluate_run_completion(&self.state, sid, rid).await;
            }
        }

        Ok((result_json, success))
    }
}

fn resolve_default_spawn_llm_profile() -> Option<String> {
    if let Ok(profile) = std::env::var("CCOS_LLM_PROFILE") {
        let trimmed = profile.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    let config_path = std::env::var("CCOS_AGENT_CONFIG_PATH")
        .unwrap_or_else(|_| "config/agent_config.toml".to_string());

    let path = std::path::Path::new(&config_path);
    let resolved = if path.exists() {
        path.to_path_buf()
    } else {
        std::path::Path::new("..").join(&config_path)
    };

    let content = match std::fs::read_to_string(&resolved) {
        Ok(content) => content,
        Err(e) => {
            log::warn!(
                "[Gateway] Failed to read agent config '{}': {}",
                resolved.display(),
                e
            );
            return None;
        }
    };

    let normalized = if content.starts_with("# RTFS") {
        content.lines().skip(1).collect::<Vec<_>>().join("\n")
    } else {
        content
    };

    match toml::from_str::<AgentConfig>(&normalized) {
        Ok(cfg) => cfg.llm_profiles.and_then(|p| p.default),
        Err(e) => {
            log::warn!(
                "[Gateway] Failed to parse agent config '{}': {}",
                resolved.display(),
                e
            );
            None
        }
    }
}

#[derive(Debug, Deserialize)]
struct InboxQuery {
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
struct InboxResponse {
    messages: Vec<MessageEnvelope>,
}

#[derive(Debug, Serialize)]
struct ChatAuditEntryResponse {
    timestamp: u64,
    event_type: String,
    function_name: Option<String>,
    session_id: Option<String>,
    run_id: Option<String>,
    step_id: Option<String>,
    rule_id: Option<String>,
    decision: Option<String>,
    gate: Option<String>,
    message_id: Option<String>,
    payload_classification: Option<String>,
}

#[derive(Debug, Serialize)]
struct ChatAuditResponse {
    events: Vec<ChatAuditEntryResponse>,
}

async fn inbox_handler(
    State(state): State<Arc<GatewayState>>,
    Query(params): Query<InboxQuery>,
) -> Result<Json<InboxResponse>, StatusCode> {
    let limit = params.limit.unwrap_or(10).min(100);
    let mut guard = state
        .inbox
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut drained = Vec::with_capacity(limit);
    for _ in 0..limit {
        if let Some(msg) = guard.pop_front() {
            drained.push(msg);
        } else {
            break;
        }
    }
    Ok(Json(InboxResponse { messages: drained }))
}

#[derive(Debug, Deserialize)]
struct AuditQuery {
    limit: Option<usize>,
    run_id: Option<String>,
    session_id: Option<String>,
}

async fn audit_handler(
    State(state): State<Arc<GatewayState>>,
    Query(params): Query<AuditQuery>,
) -> Result<Json<ChatAuditResponse>, StatusCode> {
    let limit = params.limit.unwrap_or(200).min(1000);
    let guard = state
        .chain
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut query = CausalQuery::default();
    if let Some(session_id) = &params.session_id {
        query.session_id = Some(session_id.clone());
    } else {
        query.function_prefix = Some("chat.audit.".to_string());
    }
    let mut actions = guard.query_actions(&query);
    actions.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    let mut events = Vec::new();
    for action in actions.into_iter() {
        if let Some(run_id) = &params.run_id {
            let rid = action
                .metadata
                .get("run_id")
                .and_then(|v| v.as_string())
                .unwrap_or_default();
            if rid != run_id {
                continue;
            }
        }
        // session_id is already filtered by query_actions if present in params
        let mut meta_json = serde_json::Map::new();
        for (key, value) in &action.metadata {
            let json_val = rtfs_value_to_json(value)
                .unwrap_or_else(|_| serde_json::Value::String("<unserializable>".to_string()));
            meta_json.insert(key.clone(), json_val);
        }
        let meta = serde_json::Value::Object(meta_json);
        let get_str = |k: &str| meta.get(k).and_then(|v| v.as_str()).map(|s| s.to_string());

        events.push(ChatAuditEntryResponse {
            timestamp: action.timestamp,
            event_type: get_str("event_type").unwrap_or_else(|| match action.action_type {
                crate::types::ActionType::CapabilityCall => "capability.call".to_string(),
                crate::types::ActionType::CapabilityResult => "transform.output".to_string(),
                _ => "unknown".to_string(),
            }),
            function_name: action.function_name.clone(),
            session_id: get_str("session_id"),
            run_id: get_str("run_id"),
            step_id: get_str("step_id"),
            rule_id: get_str("rule_id"),
            decision: get_str("decision"),
            gate: get_str("gate"),
            message_id: get_str("message_id"),
            payload_classification: get_str("payload_classification"),
        });

        if events.len() >= limit {
            break;
        }
    }

    Ok(Json(ChatAuditResponse { events }))
}

/// Helper to record an audit event and evaluate completion predicates
pub(crate) async fn record_run_event(
    state: &GatewayState,
    session_id: &str,
    run_id: &str,
    step_id: &str,
    event_type: &str,
    metadata: HashMap<String, Value>,
) -> RuntimeResult<()> {
    record_chat_audit_event(
        &state.chain,
        "chat",
        "chat",
        session_id,
        run_id,
        step_id,
        event_type,
        metadata,
        crate::types::ActionType::CapabilityCall,
    )?;

    evaluate_run_completion(state, session_id, run_id).await
}

/// Evaluates if a run has satisfied its completion predicate
async fn evaluate_run_completion(
    state: &GatewayState,
    session_id: &str,
    run_id: &str,
) -> RuntimeResult<()> {
    // Evaluate completion if run is active
    let (should_eval, goal) = {
        if let Ok(store) = state.run_store.lock() {
            if let Some(run) = store.get_run(run_id) {
                (
                    run.state == crate::chat::run::RunState::Active
                        && run.completion_predicate.is_some(),
                    run.goal.clone(),
                )
            } else {
                (false, String::new())
            }
        } else {
            (false, String::new())
        }
    };

    if should_eval {
        let predicate = if let Ok(store) = state.run_store.lock() {
            store
                .get_run(run_id)
                .and_then(|run| run.completion_predicate.clone())
        } else {
            None
        };
        let actions_satisfied = {
            let guard = state.chain.lock().unwrap();
            let query = crate::causal_chain::CausalQuery {
                run_id: Some(run_id.to_string()),
                ..Default::default()
            };
            let actions = guard.query_actions(&query);
            if let Some(predicate) = predicate {
                predicate.evaluate(&actions)
            } else {
                false
            }
        };

        if actions_satisfied {
            log::info!(
                "[Gateway] Run {} satisfies completion predicate for goal '{}'. Transitioning to Done.",
                run_id, goal
            );

            // Audit the auto-completion
            let mut meta = HashMap::new();
            meta.insert(
                "reason".to_string(),
                Value::String("Predicate satisfied".into()),
            );
            let _ = record_chat_audit_event(
                &state.chain,
                "chat",
                "chat",
                session_id,
                run_id,
                &format!("auto-complete-{}", uuid::Uuid::new_v4()),
                "run.complete",
                meta,
                crate::types::ActionType::InternalStep,
            );

            // Complete the run and schedule next if recurring.
            let next_run_snapshot = {
                if let Ok(mut store) = state.run_store.lock() {
                    let next_run_id = store.finish_run_and_schedule_next(
                        run_id,
                        crate::chat::run::RunState::Done,
                        |sched| Scheduler::calculate_next_run(sched),
                    );
                    next_run_id.and_then(|new_run_id| {
                        store.get_run(&new_run_id).map(|next_run| {
                            let max_runs = next_run
                                .metadata
                                .get("scheduler_max_runs")
                                .and_then(|v| v.as_u64());
                            let run_number = next_run
                                .metadata
                                .get("scheduler_run_number")
                                .and_then(|v| v.as_u64());
                            (new_run_id, max_runs, run_number)
                        })
                    })
                } else {
                    None
                }
            };

            // C1: Write a scheduled-context entry to WorkingMemory so the next run can
            // be informed that a prior run existed for this session, including which
            // WorkingMemory keys were actually used so the LLM can reuse them exactly.
            {
                use crate::working_memory::{QueryParams, WorkingMemoryEntry, WorkingMemoryMeta};
                use std::collections::HashSet;
                use std::time::SystemTime;
                let now_s = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let entry_id = format!("scheduled-ctx:{}:{}", session_id, run_id);
                let mut tags: HashSet<String> = HashSet::new();
                tags.insert("scheduled-context".to_string());
                tags.insert(format!("session:{}", session_id));
                // Retrieve goal from run store for richer context
                let goal_text = state
                    .run_store
                    .lock()
                    .ok()
                    .and_then(|s| s.get_run(run_id).map(|r| r.goal.clone()))
                    .unwrap_or_default();
                // Discover which WM keys were written during this run (D-tagged entries).
                // Entry IDs follow "global:{key}" or "skill:{id}:{key}"; skip metadata.
                let memory_keys: Vec<String> = state
                    .working_memory
                    .lock()
                    .ok()
                    .and_then(|wm| {
                        let params = QueryParams::with_tags([format!("session:{}", session_id)])
                            .with_limit(Some(50));
                        wm.query(&params).ok().map(|result| {
                            result
                                .entries
                                .iter()
                                .filter_map(|e| {
                                    let id = &e.id;
                                    if id.starts_with("scheduled-ctx:") {
                                        None // skip our own metadata entries
                                    } else if let Some(key) = id.strip_prefix("global:") {
                                        Some(key.to_string())
                                    } else if id.starts_with("skill:") {
                                        // skill:{skill_id}:{key} — take everything after second ':'
                                        id.splitn(3, ':').nth(2).map(|k| k.to_string())
                                    } else {
                                        None
                                    }
                                })
                                .collect::<Vec<_>>()
                        })
                    })
                    .unwrap_or_default();
                let content = if memory_keys.is_empty() {
                    format!("run_id={} goal={}", run_id, goal_text)
                } else {
                    format!(
                        "run_id={} goal={} memory_keys=[{}]",
                        run_id,
                        goal_text,
                        memory_keys.join(", ")
                    )
                };
                let entry = WorkingMemoryEntry::new_with_estimate(
                    entry_id,
                    format!("scheduled-ctx:{}", run_id),
                    content,
                    tags,
                    now_s,
                    WorkingMemoryMeta::default(),
                );
                if let Ok(mut wm) = state.working_memory.lock() {
                    let _ = wm.append(entry);
                }
            }

            if let Some((new_run_id, max_runs, run_number)) = next_run_snapshot {
                log::info!(
                    "[Gateway] Created next recurring run {} after completing {}",
                    new_run_id,
                    run_id
                );

                let mut meta = HashMap::new();
                meta.insert(
                    "parent_run_id".to_string(),
                    Value::String(run_id.to_string()),
                );
                if let Some(max_runs) = max_runs {
                    meta.insert("max_run".to_string(), Value::Integer(max_runs as i64));
                }
                if let Some(run_number) = run_number {
                    meta.insert("run_number".to_string(), Value::Integer(run_number as i64));
                }
                let _ = record_chat_audit_event(
                    &state.chain,
                    "chat",
                    "chat",
                    session_id,
                    &new_run_id,
                    &format!("scheduled-next-{}", uuid::Uuid::new_v4()),
                    "run.create",
                    meta,
                    crate::types::ActionType::InternalStep,
                );
            }
        }
    }

    Ok(())
}

async fn health_handler(
    State(state): State<Arc<GatewayState>>,
) -> Result<Json<HealthStatusResponse>, StatusCode> {
    Ok(Json(HealthStatusResponse {
        ok: true,
        queue_depth: state
            .inbox
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .len(),
    }))
}

#[derive(Debug, Serialize)]
struct HealthStatusResponse {
    ok: bool,
    queue_depth: usize,
}

#[derive(Debug, Deserialize)]
struct ExecuteRequest {
    capability_id: String,
    inputs: serde_json::Value,
    session_id: String,
}

#[derive(Debug, Serialize)]
struct ExecuteResponse {
    success: bool,
    result: Option<serde_json::Value>,
    error: Option<String>,
}

async fn execute_handler(
    State(state): State<Arc<GatewayState>>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<ExecuteRequest>,
) -> Result<Json<ExecuteResponse>, StatusCode> {
    // Require X-Agent-Token header
    let token = headers
        .get("X-Agent-Token")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            log::warn!("[Gateway] Execute request missing X-Agent-Token header");
            StatusCode::UNAUTHORIZED
        })?;

    // Validate token against session registry
    let session = state
        .session_registry
        .validate_token(&payload.session_id, token)
        .await
        .ok_or_else(|| {
            log::warn!("[Gateway] Invalid token for session {}", payload.session_id);
            StatusCode::UNAUTHORIZED
        })?;

    // Check session status
    if session.status != crate::chat::session::SessionStatus::Active {
        log::warn!(
            "[Gateway] Execute rejected: session {} not active ({:?})",
            payload.session_id,
            session.status
        );
        return Ok(Json(ExecuteResponse {
            success: false,
            result: None,
            error: Some(format!("Session is not active: {:?}", session.status)),
        }));
    }

    fn allowed_while_paused(capability_id: &str) -> bool {
        capability_id.starts_with("ccos.chat.egress.")
            || capability_id.starts_with("ccos.chat.transform.")
    }

    let redacted_inputs = redact_json_for_logs(&payload.inputs);
    log::info!(
        "[Gateway] Execute request cap={} session={} run_id={:?} step_id={:?}",
        payload.capability_id,
        payload.session_id,
        payload.inputs.get("run_id").and_then(|v| v.as_str()),
        payload.inputs.get("step_id").and_then(|v| v.as_str())
    );
    log::debug!(
        "[Gateway] Execute inputs (redacted) cap={} inputs={}",
        payload.capability_id,
        redacted_inputs
    );

    // Human-gated completion must not be callable by autonomous agents.
    // Keep this path reserved for approved human/admin UX only.
    if payload.capability_id == "ccos.approval.complete" {
        return Ok(Json(ExecuteResponse {
            success: false,
            result: None,
            error: Some(
                "Capability ccos.approval.complete is restricted to human/admin approval flows"
                    .to_string(),
            ),
        }));
    }

    // Best-effort: if this execute is correlated to a known Run, enforce run state/budget and update its step counters.
    // (Runs are created via POST /chat/run and correlated via the run_id input.)
    let correlated_run_id = payload.inputs.get("run_id").and_then(|v| v.as_str());
    let run_id_for_log = correlated_run_id.unwrap_or("ephemeral");

    if let Some(run_id) = correlated_run_id {
        let step_id = payload
            .inputs
            .get("step_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let mut budget_exceeded_reason: Option<String> = None;
        let mut budget_exceeded_prev_state: Option<String> = None;
        if let Ok(mut store) = state.run_store.lock() {
            if let Some(run) = store.get_run_mut(run_id) {
                log::debug!(
                    "[Gateway] Run {} state={:?} budget={:?} for capability {}",
                    run_id,
                    run.state,
                    run.budget,
                    payload.capability_id
                );
                if run.state.is_terminal() && !allowed_while_paused(&payload.capability_id) {
                    log::warn!(
                        "[Gateway] Execute refused: run {} terminal ({:?}) cap={}",
                        run_id,
                        run.state,
                        payload.capability_id
                    );
                    return Ok(Json(ExecuteResponse {
                        success: false,
                        result: None,
                        error: Some(format!(
                            "Run is terminal ({:?}); refusing to execute capability {}",
                            run.state, payload.capability_id
                        )),
                    }));
                }
                if run.state.is_paused() && !allowed_while_paused(&payload.capability_id) {
                    log::warn!(
                        "[Gateway] Execute refused: run {} paused ({:?}) cap={}",
                        run_id,
                        run.state,
                        payload.capability_id
                    );
                    return Ok(Json(ExecuteResponse {
                        success: false,
                        result: None,
                        error: Some(format!(
                            "Run is paused ({:?}); refusing to execute capability {}",
                            run.state, payload.capability_id
                        )),
                    }));
                }

                if let Some(exceeded) = run.check_budget() {
                    budget_exceeded_reason = Some(format!("{:?}", exceeded));
                    budget_exceeded_prev_state = Some(format!("{:?}", run.state));
                    log::warn!(
                        "[Gateway] Budget exceeded for run {} ({:?}), pausing",
                        run_id,
                        exceeded
                    );
                    // Pause the run by default when budget is exceeded.
                    run.transition(crate::chat::run::RunState::PausedApproval {
                        reason: budget_exceeded_reason.clone().unwrap_or_default(),
                    });
                } else {
                    // Budget ok: increment step counter for correlated calls.
                    run.consumption.steps_taken = run.consumption.steps_taken.saturating_add(1);
                    if step_id.is_some() {
                        run.current_step_id = step_id.clone();
                    }
                    run.updated_at = Utc::now();
                }
            }
            store.save_to_disk();
        }

        if let Some(reason) = budget_exceeded_reason {
            // Best-effort audit: record both the transition + the budget-exceeded reason.
            let step = step_id.as_deref().unwrap_or("budget");

            let mut transition_meta = HashMap::new();
            if let Some(prev) = budget_exceeded_prev_state {
                transition_meta.insert("previous_state".to_string(), Value::String(prev));
            }
            transition_meta.insert(
                "new_state".to_string(),
                Value::String("PausedApproval".to_string()),
            );
            transition_meta.insert("reason".to_string(), Value::String(reason.clone()));
            transition_meta.insert(
                "rule_id".to_string(),
                Value::String("chat.run.budget".to_string()),
            );
            let _ = record_run_event(
                &state,
                &payload.session_id,
                run_id,
                step,
                "run.transition",
                transition_meta,
            )
            .await;

            let mut meta = HashMap::new();
            meta.insert(
                "capability_id".to_string(),
                Value::String(payload.capability_id.clone()),
            );
            meta.insert("reason".to_string(), Value::String(reason.clone()));
            meta.insert(
                "rule_id".to_string(),
                Value::String("chat.run.budget".to_string()),
            );
            let _ = record_run_event(
                &state,
                &payload.session_id,
                run_id,
                step,
                "run.budget_exceeded",
                meta,
            )
            .await;

            if !allowed_while_paused(&payload.capability_id) {
                SheriffLogger::log_budget(
                    run_id,
                    "PAUSED",
                    &format!("Refusing capability {}", payload.capability_id),
                );
                log::warn!(
                    "[Gateway] Execute refused: budget exceeded for run {} cap={}",
                    run_id,
                    payload.capability_id
                );
                return Ok(Json(ExecuteResponse {
                    success: false,
                    result: None,
                    error: Some(format!(
                        "Run budget exceeded ({}); refusing to execute capability {}",
                        reason, payload.capability_id
                    )),
                }));
            }
        }
    }

    SheriffLogger::log_action(run_id_for_log, &payload.capability_id, &payload.inputs);

    // Forward to capability marketplace (which enforces its own policy/approvals)
    let gateway = ChatGateway {
        state: state.clone(),
    };
    let mut effective_inputs = payload.inputs.clone();
    if let Some(obj) = effective_inputs.as_object_mut() {
        if !obj.contains_key("session_id") {
            obj.insert(
                "session_id".to_string(),
                serde_json::Value::String(payload.session_id.clone()),
            );
        }
    }

    match gateway
        .execute_capability(&payload.capability_id, effective_inputs)
        .await
    {
        Ok((result, success)) => {
            log::debug!(
                "[Gateway] Capability {} execution result: success={}",
                payload.capability_id,
                success
            );
            SheriffLogger::log_result(run_id_for_log, &payload.capability_id, success, &result);
            if !success {
                let err = result
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                log::warn!(
                    "[Gateway] Capability {} returned failure: {}",
                    payload.capability_id,
                    err
                );

                // Check if this is an approval-required error and transition run to PausedApproval
                if err.contains("requires approval") || err.contains("Approval ID:") {
                    if let Some(run_id) = &correlated_run_id {
                        // Extract approval ID if present
                        let approval_id = err
                            .split("Approval ID:")
                            .nth(1)
                            .and_then(|s| s.split_whitespace().next())
                            .map(|s| s.trim_end_matches('.').to_string());

                        let reason = if let Some(ref id) = approval_id {
                            format!("Waiting for approval: {}", id)
                        } else {
                            "Waiting for approval".to_string()
                        };

                        log::info!(
                            "[Gateway] Run {} requires approval, transitioning to PausedApproval",
                            run_id
                        );

                        let mut store = state.run_store.lock().unwrap();
                        store.update_run_state(run_id, RunState::PausedApproval { reason });

                        // Record the transition
                        let mut transition_meta = HashMap::new();
                        transition_meta.insert(
                            "reason".to_string(),
                            Value::String("approval_required".to_string()),
                        );
                        if let Some(ref id) = approval_id {
                            transition_meta
                                .insert("approval_id".to_string(), Value::String(id.clone()));
                        }
                        transition_meta.insert(
                            "rule_id".to_string(),
                            Value::String("chat.run.approval_required".to_string()),
                        );
                        let _ = record_run_event(
                            &state,
                            &payload.session_id,
                            run_id,
                            "approval-required",
                            "run.transition",
                            transition_meta,
                        );
                    }
                }
            }
            if let Some(run_id) = payload.inputs.get("run_id").and_then(|v| v.as_str()) {
                if let Ok(mut store) = state.run_store.lock() {
                    if let Some(run) = store.get_run_mut(run_id) {
                        if let Some(usage) = result.get("usage").and_then(|v| v.as_object()) {
                            if let Some(egress) =
                                usage.get("network_egress_bytes").and_then(|v| v.as_u64())
                            {
                                run.consumption.network_egress_bytes += egress;
                            }
                            if let Some(ingress) =
                                usage.get("network_ingress_bytes").and_then(|v| v.as_u64())
                            {
                                run.consumption.network_ingress_bytes += ingress;
                            }
                        }
                    }
                }
            }

            if payload.capability_id == "ccos.skill.load" && success {
                let skill_id = result
                    .get("skill_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let cap_count = result
                    .get("registered_capabilities")
                    .or_else(|| result.get("capabilities"))
                    .and_then(|v| v.as_array())
                    .map(|caps| caps.len())
                    .unwrap_or(0);
                let requires_approval = result
                    .get("requires_approval")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                SheriffLogger::log_agent(
                    "SKILL",
                    &payload.session_id,
                    &format!(
                        "Skill load result: id={}, registered_capabilities={}, requires_approval={}",
                        skill_id, cap_count, requires_approval
                    ),
                );
            }
            Ok(Json(ExecuteResponse {
                success,
                result: Some(result.clone()),
                error: if success {
                    None
                } else {
                    result
                        .get("error")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .or_else(|| Some("Capability returned failure result".to_string()))
                },
            }))
        }
        Err(e) => Ok(Json(ExecuteResponse {
            success: false,
            result: None,
            error: Some(format!("Capability execution failed: {}", e)),
        })),
    }
}

#[derive(Debug, Deserialize)]
struct SendRequest {
    channel_id: String,
    content: String,
    content_class: Option<String>,
    reply_to: Option<String>,
    metadata: Option<HashMap<String, String>>,
    session_id: String,
    run_id: String,
    step_id: String,
    policy_pack_version: Option<String>,
}

#[derive(Debug, Serialize)]
struct SendResponse {
    success: bool,
    error: Option<String>,
    message_id: Option<String>,
}

async fn send_handler(
    State(state): State<Arc<GatewayState>>,
    Json(payload): Json<SendRequest>,
) -> Result<Json<SendResponse>, StatusCode> {
    let label = payload
        .content_class
        .as_deref()
        .and_then(ChatDataLabel::parse)
        .unwrap_or(ChatDataLabel::Public);

    let labeled = attach_label(Value::String(payload.content.clone()), label, None);

    let mut inputs = HashMap::new();
    inputs.insert(MapKey::String("content".to_string()), labeled);
    inputs.insert(
        MapKey::String("session_id".to_string()),
        Value::String(payload.session_id.clone()),
    );
    inputs.insert(
        MapKey::String("run_id".to_string()),
        Value::String(payload.run_id.clone()),
    );
    inputs.insert(
        MapKey::String("step_id".to_string()),
        Value::String(payload.step_id.clone()),
    );
    inputs.insert(
        MapKey::String("policy_pack_version".to_string()),
        Value::String(
            payload
                .policy_pack_version
                .unwrap_or_else(|| state.policy_pack_version.clone()),
        ),
    );

    let gateway = ChatGateway {
        state: state.clone(),
    };
    let (prepared, _success) = gateway
        .execute_capability(
            "ccos.chat.egress.prepare_outbound",
            rtfs_value_to_json(&Value::Map(inputs))
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let content_text = prepared
        .get("value")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| prepared.as_str().unwrap_or(""))
        .to_string();

    if content_text.is_empty() {
        log::warn!(
            "[Gateway] Outbound prepare produced empty content (session={}, run={}, step={})",
            payload.session_id,
            payload.run_id,
            payload.step_id
        );
        return Ok(Json(SendResponse {
            success: false,
            error: Some("Prepared outbound content is empty".to_string()),
            message_id: None,
        }));
    }

    let channel_id = payload.channel_id.clone();
    let outbound = OutboundRequest {
        channel_id: payload.channel_id,
        content: content_text,
        reply_to: payload.reply_to,
        metadata: payload.metadata,
    };

    let result = state
        .connector
        .send(&state.connector_handle, outbound)
        .await
        .map_err(|e| e);

    match result {
        Ok(send_result) => {
            log::info!(
                "[Gateway] Outbound send result success={} message_id={:?} channel={}",
                send_result.success,
                send_result.message_id,
                channel_id
            );
            Ok(Json(SendResponse {
                success: send_result.success,
                error: send_result.error,
                message_id: send_result.message_id,
            }))
        }
        Err(e) => Ok(Json(SendResponse {
            success: false,
            error: Some(format!("send failed: {}", e)),
            message_id: None,
        })),
    }
}

/// Events endpoint for agents to poll for new messages (simplified - SSE in future)
#[derive(Debug, Serialize)]
struct EventsResponse {
    messages: Vec<ChatMessage>,
    has_more: bool,
}

async fn events_handler(
    State(state): State<Arc<GatewayState>>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> Result<Json<EventsResponse>, StatusCode> {
    // Require X-Agent-Token header
    let token = headers
        .get("X-Agent-Token")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // Validate token
    let session = state
        .session_registry
        .validate_token(&session_id, token)
        .await;

    if session.is_none() {
        let stored_token = state.session_registry.get_token(&session_id).await;
        let redacted_received = redact_token_for_logs(token);
        let redacted_stored = stored_token.as_deref().map(redact_token_for_logs);
        log::warn!(
            "[Gateway] Auth failed for session {}. Received token: '{}', Stored token: '{:?}'",
            session_id,
            redacted_received,
            redacted_stored
        );
        return Err(StatusCode::UNAUTHORIZED);
    }

    let _session = session.unwrap();

    // Get messages from inbox (atomically draining shared state)
    let messages = state
        .session_registry
        .drain_session_inbox(&session_id)
        .await;
    log::debug!(
        "[Gateway] Events poll for session {} -> {} messages",
        session_id,
        messages.len()
    );

    Ok(Json(EventsResponse {
        messages,
        has_more: false, // For now, agent needs to poll again
    }))
}
/// Get session info
#[derive(Debug, Serialize)]
struct SessionInfoResponse {
    session_id: String,
    status: String,
    created_at: String,
    last_activity: String,
    inbox_size: usize,
    agent_pid: Option<u32>,
    current_step: Option<u32>,
    memory_mb: Option<u64>,
    agent_running: bool,
}

async fn get_session_handler(
    State(state): State<Arc<GatewayState>>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> Result<Json<SessionInfoResponse>, StatusCode> {
    // Require X-Agent-Token header (optional for admin/debug)
    let provided_token = headers.get("X-Agent-Token").and_then(|v| v.to_str().ok());

    let session = if let Some(token) = provided_token {
        // Validate token
        state
            .session_registry
            .validate_token(&session_id, token)
            .await
            .ok_or(StatusCode::UNAUTHORIZED)?
    } else {
        // No token - return limited info if session exists
        state
            .session_registry
            .get_session(&session_id)
            .await
            .ok_or(StatusCode::NOT_FOUND)?
    };

    let agent_running = session.is_agent_running();
    Ok(Json(SessionInfoResponse {
        session_id: session.session_id,
        status: format!("{:?}", session.status),
        created_at: session.created_at.to_string(),
        last_activity: session.last_activity.to_string(),
        inbox_size: session.inbox.len(),
        agent_pid: session.agent_pid,
        current_step: session.current_step,
        memory_mb: session.memory_mb,
        agent_running,
    }))
}

async fn list_sessions_handler(
    State(state): State<Arc<GatewayState>>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Vec<SessionInfoResponse>>, StatusCode> {
    if !is_admin_request(&state, &headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Admin view: return all sessions including those with AgentNotRunning status
    // (e.g. persisted sessions restored after gateway restart).
    let sessions = state.session_registry.list_all_sessions().await;
    let response = sessions
        .into_iter()
        .map(|s| {
            let agent_running = s.is_agent_running();
            SessionInfoResponse {
                session_id: s.session_id,
                status: format!("{:?}", s.status),
                created_at: s.created_at.to_string(),
                last_activity: s.last_activity.to_string(),
                inbox_size: s.inbox.len(),
                agent_pid: s.agent_pid,
                current_step: s.current_step,
                memory_mb: s.memory_mb,
                agent_running,
            }
        })
        .collect();

    Ok(Json(response))
}

fn is_admin_request(state: &GatewayState, headers: &axum::http::HeaderMap) -> bool {
    let provided_token = headers
        .get("X-Admin-Token")
        .or_else(|| headers.get("X-Agent-Token"))
        .and_then(|v| v.to_str().ok());

    provided_token
        .map(|t| state.admin_tokens.contains(&t.to_string()))
        .unwrap_or(false)
}

#[derive(Debug, Serialize)]
struct SpawnLlmProfileResponse {
    profile: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SetSpawnLlmProfileRequest {
    profile: Option<String>,
}

async fn get_spawn_llm_profile_handler(
    State(state): State<Arc<GatewayState>>,
    headers: axum::http::HeaderMap,
) -> Result<Json<SpawnLlmProfileResponse>, StatusCode> {
    if !is_admin_request(&state, &headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let profile = state.spawn_llm_profile.read().await.clone();
    Ok(Json(SpawnLlmProfileResponse { profile }))
}

async fn set_spawn_llm_profile_handler(
    State(state): State<Arc<GatewayState>>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<SetSpawnLlmProfileRequest>,
) -> Result<Json<SpawnLlmProfileResponse>, StatusCode> {
    if !is_admin_request(&state, &headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let normalized = payload.profile.and_then(|p| {
        let t = p.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    });

    {
        let mut guard = state.spawn_llm_profile.write().await;
        *guard = normalized.clone();
    }

    log::info!(
        "[Gateway] Updated spawn LLM profile override to {:?}",
        normalized
    );

    Ok(Json(SpawnLlmProfileResponse {
        profile: normalized,
    }))
}

/// Capabilities list response
#[derive(Debug, Serialize)]
struct CapabilityInfo {
    id: String,
    name: String,
    description: String,
    version: String,
    input_schema: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct CapabilitiesListResponse {
    capabilities: Vec<CapabilityInfo>,
}

/// List available capabilities for the agent
async fn list_capabilities_handler(
    State(state): State<Arc<GatewayState>>,
    headers: axum::http::HeaderMap,
) -> Result<Json<CapabilitiesListResponse>, StatusCode> {
    // Require X-Agent-Token header for security
    let _token = headers
        .get("X-Agent-Token")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // Get capabilities from marketplace
    let capabilities = state.marketplace.list_capabilities().await;

    // Filter out transform capabilities - agent works with resolved content in transparent mode
    let infos: Vec<CapabilityInfo> = capabilities
        .into_iter()
        .filter(|cap| {
            !cap.id.starts_with("ccos.chat.transform.") && cap.id != "ccos.approval.complete"
        })
        .map(|cap| CapabilityInfo {
            id: cap.id,
            name: cap.name,
            description: cap.description,
            version: cap.version,
            input_schema: cap.input_schema.as_ref().and_then(|ts| ts.to_json().ok()),
        })
        .collect();

    Ok(Json(CapabilitiesListResponse {
        capabilities: infos,
    }))
}

// =============================================================================
// Run Orchestration Endpoints
// =============================================================================

/// Create a new run
#[derive(Debug, Deserialize)]
pub struct CreateRunRequest {
    pub session_id: String,
    pub goal: String,
    #[serde(default)]
    pub budget: Option<BudgetContextRequest>,
    pub completion_predicate: Option<serde_json::Value>,
    pub schedule: Option<String>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub max_run: Option<u32>,
    pub run_id: Option<String>,
    pub trigger_capability_id: Option<String>,
    pub trigger_inputs: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct BudgetContextRequest {
    max_steps: Option<u32>,
    max_duration_secs: Option<u64>,
    max_tokens: Option<u64>,
    max_network_egress_bytes: Option<u64>,
    max_network_ingress_bytes: Option<u64>,
}

#[derive(Debug, Serialize)]
struct CreateRunResponse {
    run_id: String,
    session_id: String,
    goal: String,
    state: String,
    correlation_id: String,
    next_run_at: Option<DateTime<Utc>>,
}

fn is_scheduled_payload(payload: &CreateRunRequest) -> bool {
    payload.schedule.is_some() || payload.next_run_at.is_some()
}

fn split_compact_interval_token(token: &str) -> Option<(u32, &str)> {
    let first_alpha_idx = token
        .char_indices()
        .find(|(_, ch)| ch.is_ascii_alphabetic())
        .map(|(idx, _)| idx)?;
    if first_alpha_idx == 0 {
        return None;
    }
    let (num_part, unit_part) = token.split_at(first_alpha_idx);
    if unit_part.is_empty() {
        return None;
    }
    let parsed = num_part.parse::<u32>().ok()?;
    if parsed == 0 {
        return None;
    }
    Some((parsed, unit_part))
}

fn normalize_schedule_input(schedule: &str) -> Option<String> {
    let trimmed = schedule.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Provider-native tool calls may occasionally emit stray quotes.
    // Normalize them so valid cron strings like `*/10 * * * * *"` still parse.
    let cleaned = trimmed
        .trim_matches(|c| c == '"' || c == '\'' || c == '`')
        .trim();
    if cleaned.is_empty() {
        return None;
    }

    if crate::chat::scheduler::Scheduler::calculate_next_run(cleaned).is_some() {
        return Some(cleaned.to_string());
    }

    let lower = cleaned.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("every")
        .map(|v| v.trim())
        .unwrap_or(lower.trim());
    if rest.is_empty() {
        return None;
    }
    let parts: Vec<&str> = rest.split_whitespace().collect();

    let (value, unit) = match parts.as_slice() {
        [unit] => {
            if let Some((parsed, parsed_unit)) = split_compact_interval_token(unit) {
                (parsed, parsed_unit)
            } else {
                (1_u32, *unit)
            }
        }
        [num, unit] => {
            let parsed = num.parse::<u32>().ok()?;
            if parsed == 0 {
                return None;
            }
            (parsed, *unit)
        }
        _ => return None,
    };

    let cron = match unit {
        "s" | "sec" | "secs" | "second" | "seconds" => format!("*/{} * * * * *", value),
        "m" | "min" | "mins" | "minute" | "minutes" => format!("*/{} * * * *", value),
        "h" | "hr" | "hrs" | "hour" | "hours" => format!("0 */{} * * *", value),
        "d" | "day" | "days" => format!("0 0 */{} * *", value),
        _ => return None,
    };

    if crate::chat::scheduler::Scheduler::calculate_next_run(&cron).is_some() {
        Some(cron)
    } else {
        None
    }
}

#[cfg(test)]
mod schedule_normalization_tests {
    use super::normalize_schedule_input;

    #[test]
    fn normalizes_short_second_syntax() {
        assert_eq!(
            normalize_schedule_input("every 10s").as_deref(),
            Some("*/10 * * * * *")
        );
        assert_eq!(
            normalize_schedule_input("every 10seconds").as_deref(),
            Some("*/10 * * * * *")
        );
    }

    #[test]
    fn normalizes_cron_with_trailing_quote() {
        assert_eq!(
            normalize_schedule_input("*/10 * * * * *\"").as_deref(),
            Some("*/10 * * * * *")
        );
    }

    #[test]
    fn rejects_invalid_schedules() {
        assert_eq!(normalize_schedule_input("every 0s"), None);
        assert_eq!(normalize_schedule_input("every banana"), None);
    }
}

async fn create_run_handler(
    State(state): State<Arc<GatewayState>>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<CreateRunRequest>,
) -> Result<Json<CreateRunResponse>, StatusCode> {
    let normalized_schedule = payload
        .schedule
        .as_deref()
        .and_then(normalize_schedule_input);

    if payload.schedule.is_some() && normalized_schedule.is_none() && payload.next_run_at.is_none()
    {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Require X-Agent-Token header OR X-Internal-Secret
    let maybe_token = headers.get("X-Agent-Token").and_then(|v| v.to_str().ok());
    let maybe_secret = headers
        .get("X-Internal-Secret")
        .and_then(|v| v.to_str().ok());

    match (maybe_token, maybe_secret) {
        (Some(token), _) => {
            // Validate agent token against session
            let _session = state
                .session_registry
                .validate_token(&payload.session_id, token)
                .await
                .ok_or(StatusCode::UNAUTHORIZED)?;
        }
        (_, Some(secret)) => {
            // Validate internal secret
            if secret != state.internal_api_secret {
                return Err(StatusCode::UNAUTHORIZED);
            }
        }
        _ => return Err(StatusCode::UNAUTHORIZED),
    }

    // Create budget from request
    let budget = payload.budget.as_ref().map(|b| BudgetContext {
        max_steps: b.max_steps.unwrap_or(50),
        max_duration_secs: b.max_duration_secs.unwrap_or(300),
        max_tokens: b.max_tokens,
        max_retries_per_step: 3,
        max_network_egress_bytes: b.max_network_egress_bytes,
        max_network_ingress_bytes: b.max_network_ingress_bytes,
    });

    // Check for duplicate scheduled runs with similar goals
    if payload.schedule.is_some() || payload.next_run_at.is_some() {
        let store = state
            .run_store
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        if let Some(parent_run_id) = payload.run_id.as_deref() {
            if let Some(parent_run) = store.get_run(parent_run_id) {
                if parent_run.schedule.is_some() {
                    log::info!(
                        "[Gateway] Suppressing nested scheduled run creation from scheduled parent run {}",
                        parent_run_id
                    );
                    SheriffLogger::log_run(
                        "DEDUP",
                        parent_run_id,
                        &payload.session_id,
                        "Suppressing nested run.create from scheduled parent run",
                    );
                    return Ok(Json(CreateRunResponse {
                        run_id: parent_run.id.clone(),
                        session_id: payload.session_id.clone(),
                        goal: parent_run.goal.clone(),
                        state: format!("{:?}", parent_run.state),
                        correlation_id: parent_run.correlation_id.clone(),
                        next_run_at: parent_run.next_run_at,
                    }));
                }
            }

            if let Some(existing_run) = store
                .get_runs_for_session(&payload.session_id)
                .into_iter()
                .rev()
                .find(|run| {
                    matches!(run.state, RunState::Scheduled)
                        && run
                            .metadata
                            .get("created_from_run_id")
                            .and_then(|v| v.as_str())
                            == Some(parent_run_id)
                })
            {
                log::info!(
                    "[Gateway] Deduplicating scheduled run by parent run_id {} -> existing run {}",
                    parent_run_id,
                    existing_run.id
                );
                SheriffLogger::log_run(
                    "DEDUP",
                    &existing_run.id,
                    &payload.session_id,
                    &format!(
                        "Returning existing scheduled run created by parent run {}",
                        parent_run_id
                    ),
                );
                return Ok(Json(CreateRunResponse {
                    run_id: existing_run.id.clone(),
                    session_id: payload.session_id.clone(),
                    goal: existing_run.goal.clone(),
                    state: format!("{:?}", existing_run.state),
                    correlation_id: existing_run.correlation_id.clone(),
                    next_run_at: existing_run.next_run_at,
                }));
            }
        }

        if let Some(existing_run_id) = store.find_similar_scheduled_run(
            &payload.session_id,
            &payload.goal,
            normalized_schedule.as_deref(),
            payload.next_run_at,
            payload.trigger_capability_id.as_deref(),
            payload.trigger_inputs.as_ref(),
        ) {
            log::info!(
                "[Gateway] Deduplicating scheduled run: existing run {} found with similar goal for session {}",
                existing_run_id,
                payload.session_id
            );
            SheriffLogger::log_run(
                "DEDUP",
                &existing_run_id,
                &payload.session_id,
                &format!(
                    "Returning existing scheduled run instead of creating duplicate for goal: {}",
                    payload.goal
                ),
            );
            // Return the existing run's info
            if let Some(existing_run) = store.get_run(&existing_run_id) {
                return Ok(Json(CreateRunResponse {
                    run_id: existing_run_id,
                    session_id: payload.session_id.clone(),
                    goal: existing_run.goal.clone(),
                    state: format!("{:?}", existing_run.state),
                    correlation_id: existing_run.correlation_id.clone(),
                    next_run_at: existing_run.next_run_at,
                }));
            }
        }
    }

    // Create the run
    // Create the run
    let mut run = if is_scheduled_payload(&payload) {
        if let Some(next_at) = payload.next_run_at {
            Run::new_scheduled(
                payload.session_id.clone(),
                payload.goal.clone(),
                normalized_schedule
                    .clone()
                    .unwrap_or_else(|| "one-off".to_string()),
                next_at,
                budget.clone(),
                payload.trigger_capability_id.clone(),
                payload.trigger_inputs.clone(),
            )
        } else if let Some(ref sched) = normalized_schedule {
            if let Some(next_at) = crate::chat::scheduler::Scheduler::calculate_next_run(sched) {
                Run::new_scheduled(
                    payload.session_id.clone(),
                    payload.goal.clone(),
                    sched.clone(),
                    next_at,
                    budget.clone(),
                    payload.trigger_capability_id.clone(),
                    payload.trigger_inputs.clone(),
                )
            } else {
                return Err(StatusCode::BAD_REQUEST);
            }
        } else {
            return Err(StatusCode::BAD_REQUEST);
        }
    } else {
        let mut r = Run::new(
            payload.session_id.clone(),
            payload.goal.clone(),
            budget.clone(),
        );
        r.trigger_capability_id = payload.trigger_capability_id.clone();
        r.trigger_inputs = payload.trigger_inputs.clone();
        r
    };

    if let Some(parent_run_id) = payload.run_id.as_ref() {
        run.metadata.insert(
            "created_from_run_id".to_string(),
            serde_json::Value::String(parent_run_id.clone()),
        );
    }
    if let Some(pred_value) = &payload.completion_predicate {
        let predicate: Option<crate::chat::Predicate> = if pred_value.is_string() {
            // Legacy/convenience migration
            let s = pred_value.as_str().unwrap();
            if s.starts_with("capability_succeeded:") {
                let id = &s["capability_succeeded:".len()..];
                Some(crate::chat::Predicate::ActionSucceeded {
                    function_name: id.to_string(),
                })
            } else {
                None
            }
        } else {
            serde_json::from_value(pred_value.clone()).ok()
        };
        run.completion_predicate = predicate;
    }

    if is_scheduled_payload(&payload) {
        run.metadata.insert(
            "scheduler_run_number".to_string(),
            serde_json::Value::from(1_u64),
        );
        if let Some(max_runs) = payload.max_run {
            run.metadata.insert(
                "scheduler_max_runs".to_string(),
                serde_json::Value::from(max_runs as u64),
            );
        }
    }

    let run_id = run.id.clone();
    let correlation_id = run.correlation_id.clone();
    let budget_audit = run.budget.clone();
    let state_str = format!("{:?}", run.state);
    let next_run_at = run.next_run_at;
    let is_scheduled = matches!(run.state, RunState::Scheduled);

    // Store the run (refuse if another active run exists for the session).
    {
        let mut store = state
            .run_store
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if !is_scheduled
            && store
                .get_active_run_for_session(&payload.session_id)
                .is_some()
        {
            return Err(StatusCode::CONFLICT);
        }
        store.create_run(run);
        log::debug!(
            "[Gateway] Run {} created for session {}",
            run_id,
            payload.session_id
        );
    }

    SheriffLogger::log_run(
        if is_scheduled { "SCHEDULED" } else { "CREATED" },
        &run_id,
        &payload.session_id,
        &payload.goal,
    );

    // Persist run creation to the causal chain (minimal audit trail).
    {
        let mut meta = HashMap::new();
        meta.insert("goal".to_string(), Value::String(payload.goal.clone()));
        meta.insert(
            "correlation_id".to_string(),
            Value::String(correlation_id.clone()),
        );
        meta.insert(
            "rule_id".to_string(),
            Value::String("chat.run.create".to_string()),
        );
        meta.insert(
            "budget_max_steps".to_string(),
            Value::Integer(budget_audit.max_steps as i64),
        );
        meta.insert(
            "budget_max_duration_secs".to_string(),
            Value::Integer(budget_audit.max_duration_secs as i64),
        );
        if let Some(sched) = &payload.schedule {
            meta.insert("schedule".to_string(), Value::String(sched.clone()));
        }
        if let Some(next) = next_run_at {
            meta.insert("next_run_at".to_string(), Value::String(next.to_rfc3339()));
        }
        if let Some(max_runs) = payload.max_run {
            meta.insert("max_run".to_string(), Value::Integer(max_runs as i64));
            meta.insert("run_number".to_string(), Value::Integer(1));
        }

        let _ = record_run_event(
            &state,
            &payload.session_id,
            &run_id,
            &format!("create-{}", uuid::Uuid::new_v4()),
            "run.create",
            meta,
        )
        .await;
        log::debug!(
            "[Gateway] Persistence for run {} creation requested",
            run_id
        );
    }

    if !is_scheduled {
        // Spawn agent for this run
        if let Err(e) = state
            .spawn_agent_for_run(&payload.session_id, &run_id, budget)
            .await
        {
            SheriffLogger::log_run(
                "SPAWN_ERROR",
                &run_id,
                &payload.session_id,
                &format!("Failed to spawn agent: {}", e),
            );
        }

        // Kick off the run by enqueueing a synthetic system message with the goal.
        let channel_id = payload
            .session_id
            .split(':')
            .nth(1)
            .unwrap_or("default")
            .to_string();
        let kickoff = format!("Run started ({}). Goal:\n{}", run_id, payload.goal);
        if let Err(e) = state
            .session_registry
            .push_message_to_session(
                &payload.session_id,
                channel_id,
                kickoff,
                "system".to_string(),
                Some(run_id.clone()),
            )
            .await
        {
            log::warn!(
                "[Gateway] Failed to enqueue run kickoff message for {}: {}",
                payload.session_id,
                e
            );
        }
    }

    log::debug!(
        "[Gateway] Created {}run {} for session {} with goal: {}",
        if is_scheduled { "scheduled " } else { "" },
        run_id,
        payload.session_id,
        payload.goal
    );

    Ok(Json(CreateRunResponse {
        run_id,
        session_id: payload.session_id,
        goal: payload.goal,
        state: state_str,
        correlation_id,
        next_run_at,
    }))
}

/// Resume a paused run.
///
/// Handles two distinct pause kinds:
/// - `PausedCheckpoint`: loads the checkpoint, transitions to `Active`, and re-spawns the agent.
/// - `PausedSchedule`:   recalculates `next_run_at` and transitions back to `Scheduled`.
async fn resume_run_handler(
    State(state): State<Arc<GatewayState>>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(run_id): axum::extract::Path<String>,
) -> Result<StatusCode, StatusCode> {
    // 1. Auth check (admin, agent token, or internal secret)
    let is_admin = is_admin_request(&state, &headers);
    if !is_admin {
        let maybe_token = headers.get("X-Agent-Token").and_then(|v| v.to_str().ok());
        let maybe_secret = headers
            .get("X-Internal-Secret")
            .and_then(|v| v.to_str().ok());

        match (maybe_token, maybe_secret) {
            (Some(token), _) => {
                let session_id = {
                    let store = state
                        .run_store
                        .lock()
                        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                    let run = store.get_run(&run_id).ok_or(StatusCode::NOT_FOUND)?;
                    run.session_id.clone()
                };

                let _session = state
                    .session_registry
                    .validate_token(&session_id, token)
                    .await
                    .ok_or(StatusCode::UNAUTHORIZED)?;
            }
            (_, Some(secret)) => {
                if secret != state.internal_api_secret {
                    return Err(StatusCode::UNAUTHORIZED);
                }
            }
            _ => return Err(StatusCode::UNAUTHORIZED),
        }
    }

    // 2. Inspect current state to decide which resume path to take
    let current_state = {
        let store = state
            .run_store
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let run = store.get_run(&run_id).ok_or(StatusCode::NOT_FOUND)?;
        run.state.clone()
    };

    match current_state {
        // ── PausedSchedule: simply unfreeze the scheduler entry ──────────────
        RunState::PausedSchedule { .. } => {
            let session_id = {
                let store = state
                    .run_store
                    .lock()
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                store
                    .get_run(&run_id)
                    .ok_or(StatusCode::NOT_FOUND)?
                    .session_id
                    .clone()
            };

            SheriffLogger::log_run(
                "RESUME_SCHEDULE",
                &run_id,
                &session_id,
                "Resuming paused scheduled run",
            );

            let resumed = {
                let mut store = state
                    .run_store
                    .lock()
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                store.resume_scheduled_run(&run_id, |sched| {
                    crate::chat::Scheduler::calculate_next_run(sched)
                })
            };

            if !resumed {
                return Err(StatusCode::CONFLICT);
            }

            let _ = record_chat_audit_event(
                &state.chain,
                "chat",
                "chat",
                &session_id,
                &run_id,
                "run.resume_schedule",
                "run.resume_schedule",
                {
                    let mut m = HashMap::new();
                    m.insert(
                        "rule_id".to_string(),
                        Value::String("chat.run.resume_schedule".to_string()),
                    );
                    m
                },
                crate::types::ActionType::InternalStep,
            );

            Ok(StatusCode::OK)
        }

        // ── PausedCheckpoint: reload checkpoint and re-spawn agent ────────────
        RunState::PausedCheckpoint { .. } => {
            let (session_id, budget, checkpoint_id) = {
                let store = state
                    .run_store
                    .lock()
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                let run = store.get_run(&run_id).ok_or(StatusCode::NOT_FOUND)?;
                (
                    run.session_id.clone(),
                    run.budget.clone(),
                    run.latest_checkpoint_id.clone(),
                )
            };

            SheriffLogger::log_run(
                "RESUME",
                &run_id,
                &session_id,
                &format!("Resuming from checkpoint {:?}", checkpoint_id),
            );

            let ckpt_id = checkpoint_id.ok_or(StatusCode::BAD_REQUEST)?;
            let ckpt = state
                .checkpoint_store
                .get_checkpoint(&ckpt_id)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .ok_or(StatusCode::NOT_FOUND)?;

            {
                let mut store = state
                    .run_store
                    .lock()
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                store.update_run_state(&run_id, RunState::Active);
            }

            if let Err(e) = state
                .spawn_agent_for_run(&session_id, &run_id, Some(budget))
                .await
            {
                log::error!(
                    "[Gateway] Failed to spawn agent for resume {}: {}",
                    run_id,
                    e
                );
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }

            let channel_id = session_id
                .split(':')
                .nth(1)
                .unwrap_or("default")
                .to_string();
            let resume_msg = format!("Resuming run {}. Checkpoint: {}", run_id, ckpt.id);
            let _ = state
                .session_registry
                .push_message_to_session(
                    &session_id,
                    channel_id,
                    resume_msg,
                    "system".to_string(),
                    Some(run_id),
                )
                .await;

            Ok(StatusCode::OK)
        }

        _ => Err(StatusCode::BAD_REQUEST),
    }
}

/// Create a checkpoint for an active run
#[derive(Debug, Deserialize)]
struct CheckpointRunRequest {
    env: serde_json::Value,
    ir_pos: usize,
    reason: String,
}

async fn checkpoint_run_handler(
    State(state): State<Arc<GatewayState>>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(run_id): axum::extract::Path<String>,
    Json(payload): Json<CheckpointRunRequest>,
) -> Result<Json<String>, StatusCode> {
    // 1. Auth check
    let maybe_token = headers.get("X-Agent-Token").and_then(|v| v.to_str().ok());
    let maybe_secret = headers
        .get("X-Internal-Secret")
        .and_then(|v| v.to_str().ok());

    match (maybe_token, maybe_secret) {
        (Some(token), _) => {
            let session_id = {
                let store = state
                    .run_store
                    .lock()
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                let run = store.get_run(&run_id).ok_or(StatusCode::NOT_FOUND)?;
                run.session_id.clone()
            };

            let _session = state
                .session_registry
                .validate_token(&session_id, token)
                .await
                .ok_or(StatusCode::UNAUTHORIZED)?;
        }
        (_, Some(secret)) => {
            // Validate internal secret
            if secret != state.internal_api_secret {
                return Err(StatusCode::UNAUTHORIZED);
            }
        }
        _ => return Err(StatusCode::UNAUTHORIZED),
    }

    let session_id = {
        let store = state
            .run_store
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        store
            .get_run(&run_id)
            .map(|r| r.session_id.clone())
            .ok_or(StatusCode::NOT_FOUND)?
    };

    // 2. Create checkpoint
    let ckpt = Checkpoint::new(run_id.clone(), payload.env, payload.ir_pos);
    let ckpt_id = ckpt.id.clone();

    SheriffLogger::log_run(
        "CHECKPOINT",
        &run_id,
        &session_id,
        &format!("Saving state (IR pos: {})", payload.ir_pos),
    );

    state
        .checkpoint_store
        .store_checkpoint(ckpt)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // 3. Update run state
    {
        let mut store = state
            .run_store
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let run = store.get_run_mut(&run_id).ok_or(StatusCode::NOT_FOUND)?;
        run.state = RunState::PausedCheckpoint {
            reason: payload.reason,
            checkpoint_id: ckpt_id.clone(),
        };
        run.latest_checkpoint_id = Some(ckpt_id.clone());
        store.save_to_disk();
    }

    Ok(Json(ckpt_id))
}

/// Get run status
#[derive(Debug, Serialize)]
struct GetRunResponse {
    run_id: String,
    session_id: String,
    goal: String,
    state: String,
    steps_taken: u32,
    elapsed_secs: u64,
    budget_elapsed_secs: u64,
    budget_max_steps: u32,
    budget_max_duration_secs: u64,
    budget_max_tokens: Option<u64>,
    created_at: String,
    updated_at: String,
    current_step_id: Option<String>,
    completion_predicate: Option<crate::chat::Predicate>,
    next_run_at: Option<String>,
}

async fn get_run_handler(
    State(state): State<Arc<GatewayState>>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(run_id): axum::extract::Path<String>,
) -> Result<Json<GetRunResponse>, StatusCode> {
    // Require X-Agent-Token header OR X-Internal-Secret
    let maybe_token = headers.get("X-Agent-Token").and_then(|v| v.to_str().ok());
    let maybe_secret = headers
        .get("X-Internal-Secret")
        .and_then(|v| v.to_str().ok());

    match (maybe_token, maybe_secret) {
        (Some(token), _) => {
            // Resolve the owning session_id for authorization (drop lock before await).
            let session_id = {
                let store = state
                    .run_store
                    .lock()
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                store
                    .get_run(&run_id)
                    .map(|r| r.session_id.clone())
                    .ok_or(StatusCode::NOT_FOUND)?
            };

            // Validate token against the owning session
            let _session = state
                .session_registry
                .validate_token(&session_id, token)
                .await
                .ok_or(StatusCode::UNAUTHORIZED)?;
        }
        (_, Some(secret)) => {
            // Validate internal secret
            if secret != state.internal_api_secret {
                return Err(StatusCode::UNAUTHORIZED);
            }
        }
        _ => return Err(StatusCode::UNAUTHORIZED),
    }

    let store = state
        .run_store
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let run = store.get_run(&run_id).ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(GetRunResponse {
        run_id: run.id.clone(),
        session_id: run.session_id.clone(),
        goal: run.goal.clone(),
        state: format!("{:?}", run.state),
        steps_taken: run.consumption.steps_taken,
        elapsed_secs: run.elapsed_secs(),
        budget_elapsed_secs: run.budget_elapsed_secs(),
        budget_max_steps: run.budget.max_steps,
        budget_max_duration_secs: run.budget.max_duration_secs,
        budget_max_tokens: run.budget.max_tokens,
        created_at: run.created_at.to_rfc3339(),
        updated_at: run.updated_at.to_rfc3339(),
        current_step_id: run.current_step_id.clone(),
        completion_predicate: run.completion_predicate.clone(),
        next_run_at: run.next_run_at.map(|dt| dt.to_rfc3339()),
    }))
}

#[derive(Debug, Deserialize)]
struct ListRunsQuery {
    session_id: String,
}

#[derive(Debug, Serialize)]
struct RunSummaryResponse {
    run_id: String,
    goal: String,
    state: String,
    steps_taken: u32,
    elapsed_secs: u64,
    created_at: String,
    updated_at: String,
    current_step_id: Option<String>,
    next_run_at: Option<String>,
    /// Cron/interval schedule expression, if this is a recurring run
    schedule: Option<String>,
    /// Stable ID shared by all instances of the same recurring schedule
    schedule_group_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct ListRunsResponse {
    session_id: String,
    runs: Vec<RunSummaryResponse>,
}

async fn list_runs_handler(
    State(state): State<Arc<GatewayState>>,
    headers: axum::http::HeaderMap,
    Query(query): Query<ListRunsQuery>,
) -> Result<Json<ListRunsResponse>, StatusCode> {
    if is_admin_request(&state, &headers) {
        let store = state
            .run_store
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let mut runs = store.get_runs_for_session(&query.session_id);

        runs.sort_by_key(|r| r.updated_at);
        runs.reverse();

        let runs = runs
            .into_iter()
            .map(|run| RunSummaryResponse {
                run_id: run.id.clone(),
                goal: run.goal.clone(),
                state: format!("{:?}", run.state),
                steps_taken: run.consumption.steps_taken,
                elapsed_secs: run.elapsed_secs(),
                created_at: run.created_at.to_rfc3339(),
                updated_at: run.updated_at.to_rfc3339(),
                current_step_id: run.current_step_id.clone(),
                next_run_at: run.next_run_at.map(|dt| dt.to_rfc3339()),
                schedule: run.schedule.clone(),
                schedule_group_id: run
                    .metadata
                    .get("schedule_group_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
            })
            .collect();

        return Ok(Json(ListRunsResponse {
            session_id: query.session_id,
            runs,
        }));
    }

    // Require X-Agent-Token header OR X-Internal-Secret
    let maybe_token = headers.get("X-Agent-Token").and_then(|v| v.to_str().ok());
    let maybe_secret = headers
        .get("X-Internal-Secret")
        .and_then(|v| v.to_str().ok());

    match (maybe_token, maybe_secret) {
        (Some(token), _) => {
            // Validate agent token against session
            let _session = state
                .session_registry
                .validate_token(&query.session_id, token)
                .await
                .ok_or(StatusCode::UNAUTHORIZED)?;
        }
        (_, Some(secret)) => {
            // Validate internal secret
            if secret != state.internal_api_secret {
                return Err(StatusCode::UNAUTHORIZED);
            }
        }
        _ => return Err(StatusCode::UNAUTHORIZED),
    }

    let store = state
        .run_store
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut runs = store.get_runs_for_session(&query.session_id);

    // Present latest-first for convenience.
    runs.sort_by_key(|r| r.updated_at);
    runs.reverse();

    let runs = runs
        .into_iter()
        .map(|run| RunSummaryResponse {
            run_id: run.id.clone(),
            goal: run.goal.clone(),
            state: format!("{:?}", run.state),
            steps_taken: run.consumption.steps_taken,
            elapsed_secs: run.elapsed_secs(),
            created_at: run.created_at.to_rfc3339(),
            updated_at: run.updated_at.to_rfc3339(),
            current_step_id: run.current_step_id.clone(),
            next_run_at: run.next_run_at.map(|dt| dt.to_rfc3339()),
            schedule: run.schedule.clone(),
            schedule_group_id: run
                .metadata
                .get("schedule_group_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        })
        .collect();

    Ok(Json(ListRunsResponse {
        session_id: query.session_id,
        runs,
    }))
}

#[derive(Debug, Deserialize)]
struct ListRunActionsQuery {
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
struct RunActionEntryResponse {
    action_id: String,
    parent_action_id: Option<String>,
    action_type: String,
    function_name: Option<String>,
    timestamp: u64,
    run_id: Option<String>,
    step_id: Option<String>,
    success: Option<bool>,
}

#[derive(Debug, Serialize)]
struct ListRunActionsResponse {
    run_id: String,
    actions: Vec<RunActionEntryResponse>,
}

async fn list_run_actions_handler(
    State(state): State<Arc<GatewayState>>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(run_id): axum::extract::Path<String>,
    Query(params): Query<ListRunActionsQuery>,
) -> Result<Json<ListRunActionsResponse>, StatusCode> {
    // Require X-Agent-Token header OR X-Internal-Secret
    let maybe_token = headers.get("X-Agent-Token").and_then(|v| v.to_str().ok());
    let maybe_secret = headers
        .get("X-Internal-Secret")
        .and_then(|v| v.to_str().ok());

    match (maybe_token, maybe_secret) {
        (Some(token), _) => {
            // Resolve owning session for authorization.
            let session_id = {
                let store = state
                    .run_store
                    .lock()
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                store
                    .get_run(&run_id)
                    .map(|r| r.session_id.clone())
                    .ok_or(StatusCode::NOT_FOUND)?
            };

            let _session = state
                .session_registry
                .validate_token(&session_id, token)
                .await
                .ok_or(StatusCode::UNAUTHORIZED)?;
        }
        (_, Some(secret)) => {
            // Validate internal secret
            if secret != state.internal_api_secret {
                return Err(StatusCode::UNAUTHORIZED);
            }
        }
        _ => return Err(StatusCode::UNAUTHORIZED),
    }

    let limit = params.limit.unwrap_or(200).min(2000);
    let guard = state
        .chain
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut actions = Vec::new();
    for action in guard.get_all_actions().iter().rev() {
        let action_run_id = action.metadata.get("run_id").and_then(|v| v.as_string());
        if action_run_id != Some(&run_id) {
            continue;
        }

        let step_id = action
            .metadata
            .get("step_id")
            .and_then(|v| v.as_string())
            .map(|s| s.to_string());

        actions.push(RunActionEntryResponse {
            action_id: action.action_id.clone(),
            parent_action_id: action.parent_action_id.clone(),
            action_type: format!("{:?}", action.action_type),
            function_name: action.function_name.clone(),
            timestamp: action.timestamp,
            run_id: action_run_id.map(|s| s.to_string()),
            step_id,
            success: action.result.as_ref().map(|r| r.success),
        });

        if actions.len() >= limit {
            break;
        }
    }

    Ok(Json(ListRunActionsResponse { run_id, actions }))
}

/// Cancel a run
#[derive(Debug, Serialize)]
struct CancelRunResponse {
    run_id: String,
    cancelled: bool,
    previous_state: String,
}

async fn cancel_run_handler(
    State(state): State<Arc<GatewayState>>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(run_id): axum::extract::Path<String>,
) -> Result<Json<CancelRunResponse>, StatusCode> {
    // Resolve run info (needed for auth and audit)
    let (previous_state, session_id) = {
        let store = state
            .run_store
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let run = store.get_run(&run_id).ok_or(StatusCode::NOT_FOUND)?;
        (format!("{:?}", run.state), run.session_id.clone())
    };

    // Require admin token OR X-Agent-Token header OR X-Internal-Secret
    if is_admin_request(&state, &headers) {
        let cancelled = {
            let mut store = state
                .run_store
                .lock()
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            store.cancel_run(&run_id)
        };

        SheriffLogger::log_run("CANCELLED", &run_id, &session_id, "Run cancelled via API");

        {
            let mut meta = HashMap::new();
            meta.insert(
                "previous_state".to_string(),
                Value::String(previous_state.clone()),
            );
            meta.insert("cancelled".to_string(), Value::Boolean(cancelled));
            meta.insert(
                "rule_id".to_string(),
                Value::String("chat.run.cancel".to_string()),
            );
            let _ = record_chat_audit_event(
                &state.chain,
                "chat",
                "chat",
                &session_id,
                &run_id,
                "run.cancel",
                "run.cancel",
                meta,
                crate::types::ActionType::InternalStep,
            );
        }

        return Ok(Json(CancelRunResponse {
            run_id,
            cancelled,
            previous_state,
        }));
    }

    // Require X-Agent-Token header OR X-Internal-Secret
    let maybe_token = headers.get("X-Agent-Token").and_then(|v| v.to_str().ok());
    let maybe_secret = headers
        .get("X-Internal-Secret")
        .and_then(|v| v.to_str().ok());

    match (maybe_token, maybe_secret) {
        (Some(token), _) => {
            // Validate session ownership (async)
            let _session = state
                .session_registry
                .validate_token(&session_id, token)
                .await
                .ok_or(StatusCode::UNAUTHORIZED)?;
        }
        (_, Some(secret)) => {
            // Validate internal secret
            if secret != state.internal_api_secret {
                return Err(StatusCode::UNAUTHORIZED);
            }
        }
        _ => return Err(StatusCode::UNAUTHORIZED),
    }

    // Re-acquire lock and cancel
    let cancelled = {
        let mut store = state
            .run_store
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        store.cancel_run(&run_id)
    }; // Guard dropped here

    SheriffLogger::log_run("CANCELLED", &run_id, &session_id, "Run cancelled via API");

    // Persist run cancellation to the causal chain (minimal audit trail).
    {
        let mut meta = HashMap::new();
        meta.insert(
            "previous_state".to_string(),
            Value::String(previous_state.clone()),
        );
        meta.insert("cancelled".to_string(), Value::Boolean(cancelled));
        meta.insert(
            "rule_id".to_string(),
            Value::String("chat.run.cancel".to_string()),
        );
        let _ = record_chat_audit_event(
            &state.chain,
            "chat",
            "chat",
            &session_id,
            &run_id,
            "run.cancel",
            "run.cancel",
            meta,
            crate::types::ActionType::InternalStep,
        );
    }

    Ok(Json(CancelRunResponse {
        run_id,
        cancelled,
        previous_state,
    }))
}

#[derive(Debug, Serialize)]
struct PauseRunResponse {
    run_id: String,
    paused: bool,
    previous_state: String,
}

/// Pause a `Scheduled` run so the scheduler skips it until it is explicitly resumed.
/// Only runs in `Scheduled` state can be paused.
async fn pause_run_handler(
    State(state): State<Arc<GatewayState>>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(run_id): axum::extract::Path<String>,
) -> Result<Json<PauseRunResponse>, StatusCode> {
    let (previous_state, session_id) = {
        let store = state
            .run_store
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let run = store.get_run(&run_id).ok_or(StatusCode::NOT_FOUND)?;
        (format!("{:?}", run.state), run.session_id.clone())
    };

    // Auth: admin token, agent token, or internal secret
    if !is_admin_request(&state, &headers) {
        let maybe_token = headers.get("X-Agent-Token").and_then(|v| v.to_str().ok());
        let maybe_secret = headers
            .get("X-Internal-Secret")
            .and_then(|v| v.to_str().ok());

        match (maybe_token, maybe_secret) {
            (Some(token), _) => {
                let _session = state
                    .session_registry
                    .validate_token(&session_id, token)
                    .await
                    .ok_or(StatusCode::UNAUTHORIZED)?;
            }
            (_, Some(secret)) => {
                if secret != state.internal_api_secret {
                    return Err(StatusCode::UNAUTHORIZED);
                }
            }
            _ => return Err(StatusCode::UNAUTHORIZED),
        }
    }

    let paused = {
        let mut store = state
            .run_store
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        store.pause_scheduled_run(&run_id, "Paused via API".to_string())
    };

    SheriffLogger::log_run(
        "PAUSED_SCHEDULE",
        &run_id,
        &session_id,
        "Run paused via API",
    );

    {
        let mut meta = HashMap::new();
        meta.insert(
            "previous_state".to_string(),
            Value::String(previous_state.clone()),
        );
        meta.insert("paused".to_string(), Value::Boolean(paused));
        meta.insert(
            "rule_id".to_string(),
            Value::String("chat.run.pause_schedule".to_string()),
        );
        let _ = record_chat_audit_event(
            &state.chain,
            "chat",
            "chat",
            &session_id,
            &run_id,
            "run.pause_schedule",
            "run.pause_schedule",
            meta,
            crate::types::ActionType::InternalStep,
        );
    }

    Ok(Json(PauseRunResponse {
        run_id,
        paused,
        previous_state,
    }))
}

/// Transition a run to a new state (generic administrative endpoint).
#[derive(Debug, Deserialize)]
struct TransitionRunRequest {
    new_state: String,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    budget: Option<BudgetContextRequest>,
}

#[derive(Debug, Serialize)]
struct TransitionRunResponse {
    run_id: String,
    previous_state: String,
    new_state: String,
    transitioned: bool,
}

async fn transition_run_handler(
    State(state): State<Arc<GatewayState>>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(run_id): axum::extract::Path<String>,
    Json(payload): Json<TransitionRunRequest>,
) -> Result<Json<TransitionRunResponse>, StatusCode> {
    // Resolve run info (needed for auth and audit)
    let (previous_state, session_id) = {
        let store = state
            .run_store
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let run = store.get_run(&run_id).ok_or(StatusCode::NOT_FOUND)?;
        (format!("{:?}", run.state), run.session_id.clone())
    };

    // Require X-Agent-Token header OR X-Internal-Secret
    let maybe_token = headers.get("X-Agent-Token").and_then(|v| v.to_str().ok());
    let maybe_secret = headers
        .get("X-Internal-Secret")
        .and_then(|v| v.to_str().ok());

    match (maybe_token, maybe_secret) {
        (Some(token), _) => {
            // Validate session ownership (async)
            let _session = state
                .session_registry
                .validate_token(&session_id, token)
                .await
                .ok_or(StatusCode::UNAUTHORIZED)?;
        }
        (_, Some(secret)) => {
            // Validate internal secret
            if secret != state.internal_api_secret {
                return Err(StatusCode::UNAUTHORIZED);
            }
        }
        _ => return Err(StatusCode::UNAUTHORIZED),
    }

    // Parse requested state
    let new_state = match payload.new_state.as_str() {
        "Active" => crate::chat::run::RunState::Active,
        "Done" => crate::chat::run::RunState::Done,
        "Cancelled" => crate::chat::run::RunState::Cancelled,
        "Failed" => crate::chat::run::RunState::Failed {
            error: payload
                .reason
                .clone()
                .unwrap_or_else(|| "run marked failed".to_string()),
        },
        "PausedApproval" => crate::chat::run::RunState::PausedApproval {
            reason: payload
                .reason
                .clone()
                .unwrap_or_else(|| "paused pending approval".to_string()),
        },
        "PausedExternalEvent" => crate::chat::run::RunState::PausedExternalEvent {
            event_type: payload
                .reason
                .clone()
                .unwrap_or_else(|| "external event".to_string()),
        },
        _ => {
            return Err(StatusCode::BAD_REQUEST);
        }
    };

    // Optional completion predicate enforcement for Done.
    if matches!(new_state, crate::chat::run::RunState::Done) {
        let predicate = {
            let store = state
                .run_store
                .lock()
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            store
                .get_run(&run_id)
                .and_then(|r| r.completion_predicate.clone())
        };

        if let Some(predicate) = predicate {
            let actions_satisfied = {
                let guard = state.chain.lock().unwrap();
                let query = crate::causal_chain::CausalQuery {
                    run_id: Some(run_id.clone()),
                    ..Default::default()
                };
                let actions = guard.query_actions(&query);
                predicate.evaluate(&actions)
            };

            if !actions_satisfied {
                SheriffLogger::log_run(
                    "TRANSITION_REJECTED",
                    &run_id,
                    &session_id,
                    "Manual transition to Done rejected: completion predicate not satisfied.",
                );
                return Err(StatusCode::CONFLICT);
            }
        }
    }

    // Re-acquire lock and transition (and optionally reset/update budget).
    let (transitioned, next_run_id) = {
        SheriffLogger::log_run(
            "TRANSITION",
            &run_id,
            &session_id,
            &format!("{} -> {}", previous_state, payload.new_state),
        );
        let mut store = state
            .run_store
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        if let Some(run) = store.get_run_mut(&run_id) {
            if let Some(b) = &payload.budget {
                run.budget.max_steps = b.max_steps.unwrap_or(run.budget.max_steps);
                run.budget.max_duration_secs =
                    b.max_duration_secs.unwrap_or(run.budget.max_duration_secs);
                run.budget.max_tokens = b.max_tokens.or(run.budget.max_tokens);
            }

            if matches!(new_state, crate::chat::run::RunState::Active) {
                // Resuming a run: reset the budget window so "continue" can actually progress.
                run.reset_budget_window();
            }

            run.transition(new_state.clone());

            // If transitioning to Done or Failed, check if this is a recurring run and schedule next
            let next_run_id = if new_state.is_terminal()
                && !matches!(new_state, crate::chat::run::RunState::Cancelled)
            {
                store.finish_run_and_schedule_next(&run_id, new_state.clone(), |sched| {
                    Scheduler::calculate_next_run(sched)
                })
            } else {
                store.update_run_state(&run_id, new_state.clone());
                None
            };

            (true, next_run_id)
        } else {
            (false, None)
        }
    };

    // Log and audit next run creation if this was a recurring task
    if let Some(ref new_run_id) = next_run_id {
        log::info!(
            "[Gateway] Created next recurring run {} after completing {} via API transition",
            new_run_id,
            run_id
        );

        SheriffLogger::log_run(
            "SCHEDULED-NEXT",
            new_run_id,
            &session_id,
            &format!("Created after {} completed", run_id),
        );

        // Audit the next run creation
        let mut meta = HashMap::new();
        meta.insert("parent_run_id".to_string(), Value::String(run_id.clone()));
        if let Ok(store) = state.run_store.lock() {
            if let Some(next_run) = store.get_run(new_run_id) {
                if let Some(max_runs) = next_run
                    .metadata
                    .get("scheduler_max_runs")
                    .and_then(|v| v.as_u64())
                {
                    meta.insert("max_run".to_string(), Value::Integer(max_runs as i64));
                }
                if let Some(run_number) = next_run
                    .metadata
                    .get("scheduler_run_number")
                    .and_then(|v| v.as_u64())
                {
                    meta.insert("run_number".to_string(), Value::Integer(run_number as i64));
                }
            }
        }
        let _ = record_chat_audit_event(
            &state.chain,
            "chat",
            "chat",
            &session_id,
            new_run_id,
            &format!("scheduled-next-{}", uuid::Uuid::new_v4()),
            "run.create",
            meta,
            crate::types::ActionType::InternalStep,
        );
    }

    // Persist transition to the causal chain (minimal audit trail).
    {
        let mut meta = HashMap::new();
        meta.insert(
            "previous_state".to_string(),
            Value::String(previous_state.clone()),
        );
        meta.insert(
            "new_state".to_string(),
            Value::String(payload.new_state.clone()),
        );
        if let Some(reason) = &payload.reason {
            meta.insert("reason".to_string(), Value::String(reason.clone()));
        }
        meta.insert(
            "rule_id".to_string(),
            Value::String("chat.run.transition".to_string()),
        );
        let _ = record_run_event(
            &state,
            &session_id,
            &run_id,
            &format!("transition-{}", uuid::Uuid::new_v4()),
            "run.transition",
            meta,
        )
        .await;
    }

    Ok(Json(TransitionRunResponse {
        run_id,
        previous_state,
        new_state: payload.new_state,
        transitioned,
    }))
}

/// WebSocket handler for real-time event streaming
async fn event_stream_handler(
    ws: WebSocketUpgrade,
    Path(session_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    State(state): State<Arc<GatewayState>>,
) -> impl IntoResponse {
    let token = params.get("token").cloned().unwrap_or_default();

    // Validate session and token before upgrading
    // Admin token bypass for system clients
    let is_admin_token = state.admin_tokens.contains(&token);

    if !is_admin_token
        && state
            .session_registry
            .validate_token(&session_id, &token)
            .await
            .is_none()
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    ws.on_upgrade(move |socket| handle_websocket(socket, session_id, state))
}

/// Handle WebSocket connection for real-time event streaming
async fn handle_websocket(socket: WebSocket, session_id: String, state: Arc<GatewayState>) {
    use futures::{SinkExt, StreamExt};

    // Validate session exists
    if state
        .session_registry
        .get_session(&session_id)
        .await
        .is_none()
    {
        let _ = socket.close().await;
        return;
    }

    // Split socket into sender and receiver
    let (mut ws_sink, mut ws_stream) = socket.split();

    // Subscribe to real-time events
    let mut rx = state.realtime_sink.subscribe(&session_id).await;

    // Replay recent history from Causal Chain
    let history = {
        match state.chain.lock() {
            Ok(chain_guard) => state
                .realtime_sink
                .get_session_history(&chain_guard, &session_id),
            Err(_) => {
                log::error!("Causal Chain lock poisoned in WebSocket handler");
                Vec::new()
            }
        }
    };

    // Send historical events
    for event in history {
        let msg = Message::Text(serde_json::to_string(&event).unwrap_or_default());
        if ws_sink.send(msg).await.is_err() {
            return; // Client disconnected
        }
    }

    // Start ping interval (10 seconds)
    let mut ping_interval = interval(Duration::from_secs(10));

    loop {
        tokio::select! {
            // Forward events to client
            Ok(event) = rx.recv() => {
                let msg = Message::Text(serde_json::to_string(&event).unwrap_or_default());
                if ws_sink.send(msg).await.is_err() {
                    break; // Client disconnected
                }
            }

            // Send ping every 10 seconds
            _ = ping_interval.tick() => {
                let ping_event = crate::chat::realtime_sink::SessionEvent::Ping {
                    timestamp: Utc::now().timestamp() as u64,
                };
                let msg = Message::Text(serde_json::to_string(&ping_event).unwrap_or_default());
                if ws_sink.send(msg).await.is_err() {
                    break; // Client disconnected
                }
            }

            // Handle client messages (pong, close, etc.)
            Some(msg) = ws_stream.next() => {
                match msg {
                    Ok(Message::Close(_)) => break,
                    Ok(Message::Pong(_)) => {}, // Keepalive response
                    Err(_) => break,
                    _ => {},
                }
            }
        }
    }

    // Cleanup
    state.realtime_sink.cleanup_session(&session_id).await;
}

/// Heartbeat request payload
#[derive(Deserialize)]
struct HeartbeatRequest {
    current_step: u32,
    memory_mb: Option<u64>,
}

/// Heartbeat handler - receives agent status updates
async fn heartbeat_handler(
    Path(session_id): Path<String>,
    State(state): State<Arc<GatewayState>>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<HeartbeatRequest>,
) -> impl IntoResponse {
    // Validate token
    let token = headers
        .get("X-Agent-Token")
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default();

    // Validate session and token
    let session = match state
        .session_registry
        .validate_token(&session_id, token)
        .await
    {
        Some(s) => s,
        None => return StatusCode::UNAUTHORIZED.into_response(),
    };

    // Update session state (NOT Causal Chain - this is ephemeral state)
    let updated_session = crate::chat::session::SessionState {
        session_id: session.session_id,
        auth_token: session.auth_token,
        status: session.status,
        agent_pid: session.agent_pid,
        current_step: Some(payload.current_step),
        memory_mb: payload.memory_mb,
        created_at: session.created_at,
        last_activity: Utc::now(),
        inbox: session.inbox,
    };

    state
        .session_registry
        .update_session(updated_session.clone())
        .await;

    // Broadcast state update to WebSocket clients
    let state_snapshot = crate::chat::realtime_sink::SessionStateSnapshot {
        agent_pid: updated_session.agent_pid,
        current_step: updated_session.current_step,
        memory_mb: updated_session.memory_mb,
        is_healthy: true,
    };

    state
        .realtime_sink
        .broadcast_state_update(&session_id, &state_snapshot)
        .await;

    StatusCode::OK.into_response()
}

// =============================================================================
// Agent LLM Consultation Logging Endpoint
// =============================================================================

/// Handler for POST /chat/agent/log
///
/// Logs agent LLM consultation events to the Causal Chain.
/// This preserves a complete timeline of agent decision-making,
/// separate from capability executions.
async fn agent_log_handler(
    State(state): State<Arc<GatewayState>>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<AgentLogRequest>,
) -> Result<Json<AgentLogResponse>, StatusCode> {
    // Require X-Agent-Token header
    let token = headers
        .get("X-Agent-Token")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            log::warn!("[Gateway] Agent log request missing X-Agent-Token header");
            StatusCode::UNAUTHORIZED
        })?;

    // Validate token against session registry
    let _session = state
        .session_registry
        .validate_token(&payload.session_id, token)
        .await
        .ok_or_else(|| {
            log::warn!(
                "[Gateway] Invalid token for agent log session {}",
                payload.session_id
            );
            StatusCode::UNAUTHORIZED
        })?;

    // Log the agent LLM consultation to the Causal Chain
    let action_id = uuid::Uuid::new_v4().to_string();
    let plan_id = format!("agent-plan-{}", Utc::now().timestamp_millis());

    // Build metadata from the request
    let mut metadata = HashMap::new();
    metadata.insert(
        "session_id".to_string(),
        Value::String(payload.session_id.clone()),
    );
    metadata.insert("run_id".to_string(), Value::String(payload.run_id.clone()));
    metadata.insert(
        "step_id".to_string(),
        Value::String(payload.step_id.clone()),
    );
    metadata.insert(
        "iteration".to_string(),
        Value::Integer(payload.iteration as i64),
    );
    metadata.insert("is_initial".to_string(), Value::Boolean(payload.is_initial));
    metadata.insert(
        "understanding".to_string(),
        Value::String(payload.understanding.clone()),
    );
    metadata.insert(
        "reasoning".to_string(),
        Value::String(payload.reasoning.clone()),
    );
    metadata.insert(
        "task_complete".to_string(),
        Value::Boolean(payload.task_complete),
    );

    // Add planned capabilities
    let caps: Vec<Value> = payload
        .planned_capabilities
        .iter()
        .map(|c| {
            let mut cap_map = HashMap::new();
            cap_map.insert(
                MapKey::String("capability_id".to_string()),
                Value::String(c.capability_id.clone()),
            );
            cap_map.insert(
                MapKey::String("reasoning".to_string()),
                Value::String(c.reasoning.clone()),
            );
            if let Some(ref inputs) = c.inputs {
                if let Ok(rtfs_val) = json_to_rtfs_value(inputs) {
                    cap_map.insert(MapKey::String("inputs".to_string()), rtfs_val);
                }
            }
            Value::Map(cap_map)
        })
        .collect();
    metadata.insert("planned_capabilities".to_string(), Value::Vector(caps));

    // Add token usage if available
    if let Some(ref usage) = payload.token_usage {
        let mut usage_map = HashMap::new();
        usage_map.insert(
            MapKey::String("prompt_tokens".to_string()),
            Value::Integer(usage.prompt_tokens as i64),
        );
        usage_map.insert(
            MapKey::String("completion_tokens".to_string()),
            Value::Integer(usage.completion_tokens as i64),
        );
        usage_map.insert(
            MapKey::String("total_tokens".to_string()),
            Value::Integer(usage.total_tokens as i64),
        );
        metadata.insert("token_usage".to_string(), Value::Map(usage_map));
    }

    // Add model if available
    if let Some(ref model) = payload.model {
        metadata.insert("model".to_string(), Value::String(model.clone()));
    }

    // Add prompt/response excerpts if available
    if let Some(ref prompt) = payload.prompt {
        metadata.insert("prompt".to_string(), Value::String(prompt.clone()));
    }
    if let Some(ref response) = payload.response {
        metadata.insert("response".to_string(), Value::String(response.clone()));
    }

    // Create the action
    let action = crate::types::Action {
        action_id: action_id.clone(),
        parent_action_id: None,
        session_id: Some(payload.session_id.clone()),
        plan_id,
        intent_id: payload.run_id.clone(),
        action_type: crate::types::ActionType::AgentLlmConsultation,
        function_name: Some(format!(
            "agent.llm.{}",
            if payload.is_initial {
                "initial"
            } else {
                "follow_up"
            }
        )),
        arguments: None,
        result: None,
        cost: None,
        duration_ms: None,
        timestamp: Utc::now().timestamp_millis() as u64,
        metadata,
    };

    // Append to Causal Chain
    match state.chain.lock() {
        Ok(mut chain) => {
            if let Err(e) = chain.append(&action) {
                log::error!(
                    "[Gateway] Failed to append agent LLM consultation to Causal Chain: {}",
                    e
                );
                return Ok(Json(AgentLogResponse {
                    success: false,
                    action_id: String::new(),
                    error: Some(format!("Failed to append to Causal Chain: {}", e)),
                }));
            }
        }
        Err(e) => {
            log::error!("[Gateway] Failed to lock Causal Chain: {}", e);
            return Ok(Json(AgentLogResponse {
                success: false,
                action_id: String::new(),
                error: Some("Failed to lock Causal Chain".to_string()),
            }));
        }
    }

    log::debug!(
        "[Gateway] Logged agent LLM consultation: session={} run={} iteration={} complete={}",
        payload.session_id,
        payload.run_id,
        payload.iteration,
        payload.task_complete
    );

    Ok(Json(AgentLogResponse {
        success: true,
        action_id,
        error: None,
    }))
}
