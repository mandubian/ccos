use log;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::extract::{Json, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::Router;
use chrono::Utc;
use rtfs::ast::MapKey;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::RwLock;

use crate::approval::{
    storage_file::FileApprovalStorage, ApprovalCategory, ApprovalConsumer, ApprovalRequest,
    UnifiedApprovalQueue,
};
use crate::capabilities::registry::CapabilityRegistry;
use crate::capability_marketplace::CapabilityMarketplace;
use crate::causal_chain::{CausalChain, CausalQuery};
use crate::chat::run::{BudgetContext, Run, RunStore, SharedRunStore};
use crate::chat::{
    attach_label, record_chat_audit_event, register_chat_capabilities, AgentSpawner, ChatDataLabel,
    ChatMessage, FileQuarantineStore, MessageDirection, MessageEnvelope, QuarantineKey,
    QuarantineStore, SessionRegistry, SpawnConfig, SpawnerFactory,
};
use crate::utils::log_redaction::{redact_json_for_logs, redact_token_for_logs};
use crate::utils::value_conversion::{json_to_rtfs_value, rtfs_value_to_json};

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
}

#[derive(Clone)]
pub struct ChatGateway {
    state: Arc<GatewayState>,
}

#[allow(dead_code)]
struct GatewayState {
    marketplace: Arc<CapabilityMarketplace>,
    quarantine: Arc<dyn QuarantineStore>,
    chain: Arc<Mutex<CausalChain>>,
    _approvals: Option<UnifiedApprovalQueue<FileApprovalStorage>>,
    connector: Arc<dyn ChatConnector>,
    connector_handle: ConnectionHandle,
    inbox: Mutex<VecDeque<MessageEnvelope>>,
    policy_pack_version: String,
    /// Session registry for managing agent sessions
    session_registry: SessionRegistry,
    /// Agent spawner for launching agent runtimes
    spawner: Arc<dyn AgentSpawner>,
    /// Run store for managing autonomous runs
    run_store: SharedRunStore,
}

#[async_trait]
impl ApprovalConsumer for GatewayState {
    async fn on_approval_requested(&self, request: &ApprovalRequest) {
        log::info!("[Gateway] Approval requested: {}", request.id);
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

                    log::info!(
                        "[Gateway] Auto-resumed run {} after approval {}",
                        run_id,
                        request.id
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
        let message = match category {
            ApprovalCategory::SecretWrite {
                key, description, ..
            } => {
                format!("🔓 I need your approval to store a secret for **{}** ({}) .\n\nID: `{}`\n\nPlease visit the [Approval UI](http://localhost:3000/approvals) to complete this step.", key, description, approval_id)
            }
            ApprovalCategory::HumanActionRequest {
                title, skill_id, ..
            } => {
                format!("👋 Human intervention required for **{}** (Skill: {})\n\nID: `{}`\n\nPlease visit the [Approval UI](http://localhost:3000/approvals) to proceed.", title, skill_id, approval_id)
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
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));

        let connector = Arc::new(LoopbackWebhookConnector::new(
            config.connector.clone(),
            quarantine.clone(),
        ));
        let handle = connector.connect().await?;

        register_chat_capabilities(
            marketplace.clone(),
            quarantine.clone(),
            chain.clone(),
            approvals.clone(),
            Some(connector.clone() as Arc<dyn ChatConnector>),
            Some(handle.clone()),
        )
        .await?;

        // Initialize session management
        let session_registry = SessionRegistry::new();
        let spawner = SpawnerFactory::create();

        // Rebuild run store from causal chain
        let run_store = {
            let guard = chain.lock().unwrap();
            let run_events = guard.get_all_actions();
            Arc::new(Mutex::new(RunStore::rebuild_from_chain(&run_events)))
        };
        // guard is dropped here

        let state = Arc::new(GatewayState {
            marketplace: marketplace.clone(),
            quarantine,
            chain,
            _approvals: approvals.clone(),
            connector: connector.clone(),
            connector_handle: handle.clone(),
            inbox: Mutex::new(VecDeque::new()),
            policy_pack_version: config.policy_pack_version.clone(),
            session_registry,
            spawner: spawner.into(),
            run_store,
        });

        // Register gateway as approval consumer
        if let Some(ref approvals_queue) = approvals {
            approvals_queue.add_consumer(state.clone()).await;
        }

        let gateway = ChatGateway {
            state: state.clone(),
        };
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
            .route("/chat/session/:session_id", get(get_session_handler))
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
        log::info!(
            "[Gateway] Received inbound message {} from {} in channel {}",
            envelope.id,
            envelope.sender_id,
            envelope.channel_id
        );
        let session_id = format!("chat:{}:{}", envelope.channel_id, envelope.sender_id);
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

        // Check if session exists, if not create one and spawn agent
        let session_exists = self
            .state
            .session_registry
            .get_session(&session_id)
            .await
            .is_some();

        if !session_exists {
            // Create new session
            let session = self
                .state
                .session_registry
                .create_session(Some(session_id.clone()))
                .await;
            let token = session.auth_token.clone();

            log::info!(
                "[Gateway] Created new session {} for channel {} sender {}",
                session.session_id,
                envelope.channel_id,
                envelope.sender_id
            );

            // Legacy spawn: agent spawned per-session with default budget
            // For budget-controlled runs, use POST /chat/run which spawns with Run's budget
            // This fallback allows existing integrations to work without Run creation
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
                    log::info!(
                        "[Gateway] Spawned agent for session {} (legacy/default budget): {}",
                        session.session_id,
                        spawn_result.message
                    );
                }
                Err(e) => {
                    log::error!(
                        "[Gateway] Failed to spawn agent for session {}: {}",
                        session.session_id,
                        e
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
            log::info!(
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
    let query = CausalQuery {
        function_prefix: Some("chat.audit.".to_string()),
        ..Default::default()
    };
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
        if let Some(session_id) = &params.session_id {
            let sid = action
                .metadata
                .get("session_id")
                .and_then(|v| v.as_string())
                .unwrap_or_default();
            if sid != session_id {
                continue;
            }
        }
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
            event_type: get_str("event_type").unwrap_or_else(|| "unknown".to_string()),
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
        "[Gateway] Executing capability {} for session {} (agent PID: {:?}) with inputs: {}",
        payload.capability_id,
        session.session_id,
        session.agent_pid,
        redacted_inputs
    );

    // Best-effort: if this execute is correlated to a known Run, enforce run state/budget and update its step counters.
    // (Runs are created via POST /chat/run and correlated via the run_id input.)
    if let Some(run_id) = payload.inputs.get("run_id").and_then(|v| v.as_str()) {
        let step_id = payload
            .inputs
            .get("step_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let mut budget_exceeded_reason: Option<String> = None;
        let mut budget_exceeded_prev_state: Option<String> = None;
        if let Ok(mut store) = state.run_store.lock() {
            if let Some(run) = store.get_run_mut(run_id) {
                if run.state.is_terminal() && !allowed_while_paused(&payload.capability_id) {
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

    // Forward to capability marketplace (which enforces its own policy/approvals)
    let gateway = ChatGateway { state };
    match gateway
        .execute_capability(&payload.capability_id, payload.inputs)
        .await
    {
        Ok((result, success)) => {
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
                log::info!(
                    "[Gateway] Skill load result: id={}, registered_capabilities={}, requires_approval={}",
                    skill_id, cap_count, requires_approval
                );
            }
            Ok(Json(ExecuteResponse {
                success,
                result: Some(result),
                error: None,
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
        return Ok(Json(SendResponse {
            success: false,
            error: Some("Prepared outbound content is empty".to_string()),
            message_id: None,
        }));
    }

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
        Ok(send_result) => Ok(Json(SendResponse {
            success: send_result.success,
            error: send_result.error,
            message_id: send_result.message_id,
        })),
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

    Ok(Json(SessionInfoResponse {
        session_id: session.session_id,
        status: format!("{:?}", session.status),
        created_at: session.created_at.to_string(),
        last_activity: session.last_activity.to_string(),
        inbox_size: session.inbox.len(),
        agent_pid: session.agent_pid,
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
struct CreateRunRequest {
    session_id: String,
    goal: String,
    #[serde(default)]
    budget: Option<BudgetContextRequest>,
    completion_predicate: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct BudgetContextRequest {
    max_steps: Option<u32>,
    max_duration_secs: Option<u64>,
    max_tokens: Option<u64>,
}

#[derive(Debug, Serialize)]
struct CreateRunResponse {
    run_id: String,
    session_id: String,
    goal: String,
    state: String,
    correlation_id: String,
}

async fn create_run_handler(
    State(state): State<Arc<GatewayState>>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<CreateRunRequest>,
) -> Result<Json<CreateRunResponse>, StatusCode> {
    // Require X-Agent-Token header
    let token = headers
        .get("X-Agent-Token")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // Validate token against session
    let session = state
        .session_registry
        .validate_token(&payload.session_id, token)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // Create budget from request
    let budget = payload.budget.as_ref().map(|b| BudgetContext {
        max_steps: b.max_steps.unwrap_or(50),
        max_duration_secs: b.max_duration_secs.unwrap_or(300),
        max_tokens: b.max_tokens,
        max_retries_per_step: 3,
    });

    // Create SpawnConfig from budget
    let spawn_config = if let Some(ref b) = payload.budget {
        SpawnConfig::new()
            .with_max_steps(b.max_steps.unwrap_or(50))
            .with_max_duration_secs(b.max_duration_secs.unwrap_or(300))
            .with_budget_policy("pause_approval")
    } else {
        SpawnConfig::default()
    };

    // Create the run
    let mut run = Run::new(payload.session_id.clone(), payload.goal.clone(), budget);
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

    // Store the run (refuse if another active run exists for the session).
    {
        let mut store = state
            .run_store
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if store
            .get_active_run_for_session(&payload.session_id)
            .is_some()
        {
            return Err(StatusCode::CONFLICT);
        }
        store.create_run(run);
    }

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

        let _ = record_run_event(
            &state,
            &payload.session_id,
            &run_id,
            &format!("create-{}", uuid::Uuid::new_v4()),
            "run.create",
            meta,
        )
        .await;
    }

    // Spawn agent for this run with budget from SpawnConfig
    let spawn_config = spawn_config.with_run_id(&run_id);
    match state
        .spawner
        .spawn(
            session.session_id.clone(),
            session.auth_token.clone(),
            spawn_config,
        )
        .await
    {
        Ok(spawn_result) => {
            if let Some(pid) = spawn_result.pid {
                let _ = state
                    .session_registry
                    .set_agent_pid(&session.session_id, pid)
                    .await;
            }
            log::info!(
                "[Gateway] Spawned agent for run {} (session {}): {}",
                run_id,
                payload.session_id,
                spawn_result.message
            );
        }
        Err(e) => {
            log::error!("[Gateway] Failed to spawn agent for run {}: {}", run_id, e);
            // Don't fail the run creation - the run exists but agent spawn failed
        }
    }

    // Kick off the run by enqueueing a synthetic system message with the goal.
    // This keeps the Gateway/Agent generic: a Run is simply a goal message delivered to the agent.
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

    log::info!(
        "[Gateway] Created run {} for session {} with goal: {}",
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
    }))
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

    log::info!(
        "[Gateway] Cancelled run {}: previous_state={}, cancelled={}",
        run_id,
        previous_state,
        cancelled
    );

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

/// Resume a run
#[derive(Debug, Serialize)]
struct ResumeRunResponse {
    run_id: String,
    resumed: bool,
    previous_state: String,
}

async fn resume_run_handler(
    State(state): State<Arc<GatewayState>>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(run_id): axum::extract::Path<String>,
) -> Result<Json<ResumeRunResponse>, StatusCode> {
    let token = headers
        .get("X-Agent-Token")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let (previous_state, session_id) = {
        let store = state
            .run_store
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let run = store.get_run(&run_id).ok_or(StatusCode::NOT_FOUND)?;
        (format!("{:?}", run.state), run.session_id.clone())
    };

    let _session = state
        .session_registry
        .validate_token(&session_id, token)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let resumed = {
        let mut store = state
            .run_store
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        store.update_run_state(&run_id, crate::chat::run::RunState::Active)
    };

    if resumed {
        let meta = HashMap::new();
        let _ = record_run_event(
            &state,
            &session_id,
            &run_id,
            &format!("resume-{}", uuid::Uuid::new_v4()),
            "run.resume",
            meta,
        )
        .await;
    }

    Ok(Json(ResumeRunResponse {
        run_id,
        resumed,
        previous_state,
    }))
}

/// Checkpoint a run
#[derive(Debug, Deserialize)]
struct CheckpointRunRequest {
    reason: String,
}

#[derive(Debug, Serialize)]
struct CheckpointRunResponse {
    run_id: String,
    checkpoint_id: String,
    previous_state: String,
}

async fn checkpoint_run_handler(
    State(state): State<Arc<GatewayState>>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(run_id): axum::extract::Path<String>,
    Json(payload): Json<CheckpointRunRequest>,
) -> Result<Json<CheckpointRunResponse>, StatusCode> {
    let token = headers
        .get("X-Agent-Token")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let (previous_state, session_id) = {
        let store = state
            .run_store
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let run = store.get_run(&run_id).ok_or(StatusCode::NOT_FOUND)?;
        (format!("{:?}", run.state), run.session_id.clone())
    };

    let _session = state
        .session_registry
        .validate_token(&session_id, token)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let checkpoint_id = {
        let mut store = state
            .run_store
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let run = store.get_run_mut(&run_id).ok_or(StatusCode::NOT_FOUND)?;
        run.checkpoint(payload.reason.clone())
    };

    let mut meta = HashMap::new();
    meta.insert("reason".to_string(), Value::String(payload.reason));
    meta.insert(
        "checkpoint_id".to_string(),
        Value::String(checkpoint_id.clone()),
    );

    let _ = record_run_event(
        &state,
        &session_id,
        &run_id,
        &format!("chk-{}", uuid::Uuid::new_v4()),
        "run.checkpoint",
        meta,
    )
    .await;

    Ok(Json(CheckpointRunResponse {
        run_id,
        checkpoint_id,
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
                log::warn!(
                    "[Gateway] Manual transition to Done rejected for run {}: completion predicate not satisfied.",
                    run_id
                );
                return Err(StatusCode::CONFLICT);
            }
        }
    }

    // Re-acquire lock and transition (and optionally reset/update budget).
    let transitioned = {
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
