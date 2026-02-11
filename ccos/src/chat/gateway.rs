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
use crate::utils::log_redaction::{redact_json_for_logs, redact_token_for_logs};
use crate::utils::value_conversion::{json_to_rtfs_value, rtfs_value_to_json};

use crate::chat::agent_log::{AgentLogRequest, AgentLogResponse};
use super::connector::{
    ChatConnector, ConnectionHandle, LoopbackConnectorConfig, LoopbackWebhookConnector,
    OutboundRequest,
};

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
            // Notify chat about resolution
            let status_msg = match &request.status {
                crate::approval::types::ApprovalStatus::Approved { .. } => {
                    "✅ Approved.".to_string()
                }
                crate::approval::types::ApprovalStatus::Rejected { reason, .. } => {
                    format!("❌ Rejected: {}", reason)
                }
                _ => return,
            };
            self.send_chat_message(
                session_id,
                format!("Approval `{}`: {}", request.id, status_msg),
            )
            .await;

            if request.status.is_approved() {
                let run_id = {
                    let store = self.run_store.lock().unwrap();
                    store.get_latest_paused_approval_run_id_for_session(session_id)
                };

                if let Some(run_id) = run_id {
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
                }
            }
        }
    }
}

impl GatewayState {
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
        let message = match category {
            ApprovalCategory::SecretWrite {
                key, description, ..
            } => {
                format!("🔓 I need your approval to store a secret for **{}** ({}) .\n\nID: `{}`\n\nPlease visit the [Approval UI]({}/approvals) to complete this step.", key, description, approval_id, approval_ui_url)
            }
            ApprovalCategory::HumanActionRequest {
                title, skill_id, ..
            } => {
                format!("👋 Human intervention required for **{}** (Skill: {})\n\nID: `{}`\n\nPlease visit the [Approval UI]({}/approvals) to proceed.", title, skill_id, approval_id, approval_ui_url)
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

        let spawn_config = spawn_config.with_run_id(run_id);
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

        let connector = Arc::new(LoopbackWebhookConnector::new(
            config.connector.clone(),
            quarantine.clone(),
        ));
        let handle = connector.connect().await?;

        // Rebuild stores from causal chain (restart-safe autonomy).
        // These stores are also shared with chat capabilities (resource ingestion).
        let run_store: SharedRunStore = Arc::new(Mutex::new(RunStore::new()));
        let resource_store: SharedResourceStore = Arc::new(Mutex::new(ResourceStore::new()));
        {
            let guard = chain.lock().unwrap();
            let actions = guard.get_all_actions();
            if let Ok(mut store) = run_store.lock() {
                *store = RunStore::rebuild_from_chain(&actions);
            }
            if let Ok(mut store) = resource_store.lock() {
                *store = ResourceStore::rebuild_from_chain(&actions);
            }
        }

        // Register minimal non-chat primitives needed by autonomy flows.
        // We keep this intentionally small (no full marketplace bootstrap) to preserve
        // the chat security contract while still enabling governed resource ingestion.
        crate::capabilities::network::register_network_capabilities(&*marketplace).await?;

        // Generate internal secret for native capabilities
        let internal_api_secret = uuid::Uuid::new_v4().to_string();

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

        // Initialize session management
        let session_registry = SessionRegistry::new();
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
            admin_tokens: config.admin_tokens.clone(),
            approval_ui_url: config.approval_ui_url.clone(),
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
            .route("/chat/agent/log", post(agent_log_handler))
            .route("/chat/run", post(create_run_handler).get(list_runs_handler))
            .route("/chat/run/:run_id", get(get_run_handler))
            .route("/chat/run/:run_id/actions", get(list_run_actions_handler))
            .route("/chat/run/:run_id/cancel", post(cancel_run_handler))
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
            chosen.unwrap_or_else(|| format!("chat-run-{}", Utc::now().timestamp_millis()))
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

            match self
                .state
                .spawner
                .spawn(
                    session.session_id.clone(),
                    token.clone(),
                    SpawnConfig::default(),
                )
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
                    Ok(_) => {
                        self.state
                            .send_chat_message(
                                &session_id,
                                format!("Processing approval `{}`...", id),
                            )
                            .await;
                    }
                    Err(e) => {
                        self.state
                            .send_chat_message(&session_id, format!("❌ Approval failed: {}", e))
                            .await;
                    }
                }
                return Ok(()); // Command handled, don't push to agent
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
            )
            .await
        {
            log::error!(
                "[Gateway] Failed to push message to session {}: {}",
                session_id,
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
            let exec_result = crate::types::ExecutionResult {
                success,
                value: rtfs_result.unwrap_or(Value::Nil),
                metadata: HashMap::new(),
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
async fn record_run_event(
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
        if let Ok(mut store) = state.run_store.lock() {
            if let Some(run) = store.get_run_mut(run_id) {
                if let Some(predicate) = &run.completion_predicate {
                    let actions_satisfied = {
                        let guard = state.chain.lock().unwrap();
                        let query = crate::causal_chain::CausalQuery {
                            run_id: Some(run_id.to_string()),
                            ..Default::default()
                        };
                        let actions = guard.query_actions(&query);
                        predicate.evaluate(&actions)
                    };

                    if actions_satisfied {
                        log::info!(
                            "[Gateway] Run {} satisfies completion predicate for goal '{}'. Transitioning to Done.",
                            run_id, goal
                        );
                        run.transition(crate::chat::run::RunState::Done);

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
                        );
                    }
                }
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
    match gateway
        .execute_capability(&payload.capability_id, payload.inputs.clone())
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
    let provided_token = headers
        .get("X-Admin-Token")
        .or_else(|| headers.get("X-Agent-Token"))
        .and_then(|v| v.to_str().ok());

    let is_admin = provided_token
        .map(|t| state.admin_tokens.contains(&t.to_string()))
        .unwrap_or(false);

    if !is_admin {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let sessions = state.session_registry.list_active_sessions().await;
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
        .filter(|cap| !cap.id.starts_with("ccos.chat.transform."))
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

async fn create_run_handler(
    State(state): State<Arc<GatewayState>>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<CreateRunRequest>,
) -> Result<Json<CreateRunResponse>, StatusCode> {
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

    // Create the run
    // Create the run
    let mut run = if payload.schedule.is_some() || payload.next_run_at.is_some() {
        let schedule = payload
            .schedule
            .clone()
            .unwrap_or_else(|| "one-off".to_string());
        let next_run = payload.next_run_at.unwrap_or_else(|| {
            // Simple delay-based schedule if next_run_at not specified
            // In future, parse cron expression here
            Utc::now() + chrono::Duration::seconds(5)
        });
        Run::new_scheduled(
            payload.session_id.clone(),
            payload.goal.clone(),
            schedule,
            next_run,
            budget.clone(),
        )
    } else {
        Run::new(
            payload.session_id.clone(),
            payload.goal.clone(),
            budget.clone(),
        )
    };

    if let Some(pred_value) = payload.completion_predicate {
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
            serde_json::from_value(pred_value).ok()
        };
        run.completion_predicate = predicate;
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

/// Resume a paused run from checkpoint
async fn resume_run_handler(
    State(state): State<Arc<GatewayState>>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(run_id): axum::extract::Path<String>,
) -> Result<StatusCode, StatusCode> {
    // 1. Auth check
    let token = headers
        .get("X-Agent-Token")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let (session_id, budget, checkpoint_id) = {
        let store = state
            .run_store
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let run = store.get_run(&run_id).ok_or(StatusCode::NOT_FOUND)?;

        if !matches!(run.state, RunState::PausedCheckpoint { .. }) {
            return Err(StatusCode::BAD_REQUEST);
        }

        (
            run.session_id.clone(),
            run.budget.clone(),
            run.latest_checkpoint_id.clone(),
        )
    };

    let _session = state
        .session_registry
        .validate_token(&session_id, token)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;

    SheriffLogger::log_run(
        "RESUME",
        &run_id,
        &session_id,
        &format!("Resuming from checkpoint {:?}", checkpoint_id),
    );

    // 2. Load checkpoint
    let ckpt_id = checkpoint_id.ok_or(StatusCode::BAD_REQUEST)?;
    let ckpt = state
        .checkpoint_store
        .get_checkpoint(&ckpt_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // 3. Update state to Active
    {
        let mut store = state
            .run_store
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        store.update_run_state(&run_id, RunState::Active);
    }

    // 4. Trigger resume in agent
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

    // Push resume message
    let channel_id = session_id
        .split(':')
        .nth(1)
        .unwrap_or("default")
        .to_string();
    let resume_msg = format!("Resuming run {}. Checkpoint: {}", run_id, ckpt.id);
    let _ = state
        .session_registry
        .push_message_to_session(&session_id, channel_id, resume_msg, "system".to_string())
        .await;

    Ok(StatusCode::OK)
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
    let token = headers
        .get("X-Agent-Token")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

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
    // Require X-Agent-Token header
    let token = headers
        .get("X-Agent-Token")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

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
    // Require X-Agent-Token header
    let token = headers
        .get("X-Agent-Token")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // Validate token against session
    let _session = state
        .session_registry
        .validate_token(&query.session_id, token)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;

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
    let token = headers
        .get("X-Agent-Token")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

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
    // Require X-Agent-Token header
    let token = headers
        .get("X-Agent-Token")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // Get run info in a block scope to ensure guard is dropped
    let (previous_state, session_id) = {
        let store = state
            .run_store
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let run = store.get_run(&run_id).ok_or(StatusCode::NOT_FOUND)?;
        (format!("{:?}", run.state), run.session_id.clone())
    }; // Guard dropped here

    // Validate session ownership (async)
    let _session = state
        .session_registry
        .validate_token(&session_id, token)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;

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
        );
    }

    Ok(Json(CancelRunResponse {
        run_id,
        cancelled,
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
    // Require X-Agent-Token header
    let token = headers
        .get("X-Agent-Token")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // Get run info in a block scope to ensure guard is dropped
    let (previous_state, session_id) = {
        let store = state
            .run_store
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let run = store.get_run(&run_id).ok_or(StatusCode::NOT_FOUND)?;
        (format!("{:?}", run.state), run.session_id.clone())
    };

    // Validate session ownership (async)
    let _session = state
        .session_registry
        .validate_token(&session_id, token)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;

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
    let transitioned = {
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
            true
        } else {
            false
        }
    };

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
    metadata.insert("session_id".to_string(), Value::String(payload.session_id.clone()));
    metadata.insert("run_id".to_string(), Value::String(payload.run_id.clone()));
    metadata.insert("step_id".to_string(), Value::String(payload.step_id.clone()));
    metadata.insert("iteration".to_string(), Value::Integer(payload.iteration as i64));
    metadata.insert("is_initial".to_string(), Value::Boolean(payload.is_initial));
    metadata.insert("understanding".to_string(), Value::String(payload.understanding.clone()));
    metadata.insert("reasoning".to_string(), Value::String(payload.reasoning.clone()));
    metadata.insert("task_complete".to_string(), Value::Boolean(payload.task_complete));
    
    // Add planned capabilities
    let caps: Vec<Value> = payload.planned_capabilities
        .iter()
        .map(|c| {
            let mut cap_map = HashMap::new();
            cap_map.insert(MapKey::String("capability_id".to_string()), Value::String(c.capability_id.clone()));
            cap_map.insert(MapKey::String("reasoning".to_string()), Value::String(c.reasoning.clone()));
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
        usage_map.insert(MapKey::String("prompt_tokens".to_string()), Value::Integer(usage.prompt_tokens as i64));
        usage_map.insert(MapKey::String("completion_tokens".to_string()), Value::Integer(usage.completion_tokens as i64));
        usage_map.insert(MapKey::String("total_tokens".to_string()), Value::Integer(usage.total_tokens as i64));
        metadata.insert("token_usage".to_string(), Value::Map(usage_map));
    }
    
    // Add model if available
    if let Some(ref model) = payload.model {
        metadata.insert("model".to_string(), Value::String(model.clone()));
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
            if payload.is_initial { "initial" } else { "follow_up" }
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
