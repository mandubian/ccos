use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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

use crate::approval::{storage_file::FileApprovalStorage, UnifiedApprovalQueue};
use crate::capabilities::registry::CapabilityRegistry;
use crate::capability_marketplace::CapabilityMarketplace;
use crate::causal_chain::{CausalChain, CausalQuery};
use crate::chat::{
    attach_label, record_chat_audit_event, register_chat_capabilities, AgentSpawner, ChatDataLabel,
    ChatMessage, FileQuarantineStore, MessageDirection, MessageEnvelope, QuarantineKey,
    QuarantineStore, SessionRegistry, SpawnerFactory,
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

        let state = Arc::new(GatewayState {
            marketplace: marketplace.clone(),
            quarantine,
            chain,
            _approvals: approvals,
            connector: connector.clone(),
            connector_handle: handle.clone(),
            inbox: Mutex::new(VecDeque::new()),
            policy_pack_version: config.policy_pack_version.clone(),
            session_registry,
            spawner: spawner.into(),
        });

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
        let run_id = format!("chat-run-{}", Utc::now().timestamp_millis());
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

        record_chat_audit_event(
            &self.state.chain,
            "chat",
            "chat",
            &session_id,
            &run_id,
            &step_id,
            "message.ingest",
            meta,
        )?;

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

            // Spawn agent for this session
            match self
                .state
                .spawner
                .spawn(session.session_id.clone(), token.clone())
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
                        "[Gateway] Spawned agent for session {}: {}",
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
        let plan_id = format!("chat-plan-{}", Utc::now().timestamp_millis());
        let intent_id = session_id.clone().unwrap_or_else(|| plan_id.clone());

        let action = match self.state.chain.lock() {
            Ok(mut chain) => chain
                .log_capability_call(
                    session_id.as_deref(),
                    &plan_id,
                    &intent_id,
                    &capability_id.to_string(),
                    capability_id,
                    rtfs_args,
                )
                .ok(),
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
    for action in actions.into_iter().take(limit) {
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
    }

    Ok(Json(ChatAuditResponse { events }))
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

    let redacted_inputs = redact_json_for_logs(&payload.inputs);
    log::info!(
        "[Gateway] Executing capability {} for session {} (agent PID: {:?}) with inputs: {}",
        payload.capability_id,
        session.session_id,
        session.agent_pid,
        redacted_inputs
    );

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
