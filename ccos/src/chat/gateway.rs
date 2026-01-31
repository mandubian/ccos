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
    attach_label, record_chat_audit_event, register_chat_capabilities, ChatDataLabel,
    FileQuarantineStore, MessageEnvelope, MessageDirection, QuarantineKey, QuarantineStore,
};
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
    _quarantine: Arc<dyn QuarantineStore>,
    chain: Arc<Mutex<CausalChain>>,
    _approvals: Option<UnifiedApprovalQueue<FileApprovalStorage>>,
    connector: Arc<dyn ChatConnector>,
    connector_handle: ConnectionHandle,
    inbox: Mutex<VecDeque<MessageEnvelope>>,
    policy_pack_version: String,
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

        register_chat_capabilities(
            marketplace.as_ref(),
            quarantine.clone(),
            chain.clone(),
            approvals.clone(),
        )
        .await?;

        let connector = Arc::new(LoopbackWebhookConnector::new(
            config.connector.clone(),
            quarantine.clone(),
        ));
        let handle = connector.connect().await?;

        let state = Arc::new(GatewayState {
            marketplace: marketplace.clone(),
            _quarantine: quarantine,
            chain,
            _approvals: approvals,
            connector: connector.clone(),
            connector_handle: handle.clone(),
            inbox: Mutex::new(VecDeque::new()),
            policy_pack_version: config.policy_pack_version.clone(),
        });

        let gateway = ChatGateway { state: state.clone() };
        connector
            .subscribe(&handle, Arc::new(move |envelope| {
                let gateway = gateway.clone();
                Box::pin(async move { gateway.handle_inbound(envelope).await })
            }))
            .await?;

        let app_state = state.clone();
        let router = Router::new()
            .route("/chat/health", get(health_handler))
            .route("/chat/inbox", get(inbox_handler))
            .route("/chat/execute", post(execute_handler))
            .route("/chat/send", post(send_handler))
            .route("/chat/audit", get(audit_handler))
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
        let session_id = format!(
            "chat:{}:{}",
            envelope.channel_id,
            envelope.sender_id
        );
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
    ) -> RuntimeResult<serde_json::Value> {
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
                self.state.marketplace.execute_capability(&cap_id, &rtfs_inputs).await
            })
        });

        let (result_json, success, rtfs_result) = match exec_result {
            Ok(value) => (rtfs_value_to_json(&value)?, true, Some(value)),
            Err(e) => (
                serde_json::json!({
                    "error": format!("{}", e),
                    "capability_id": capability_id,
                }),
                false,
                None,
            ),
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

        Ok(result_json)
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

async fn health_handler(State(state): State<Arc<GatewayState>>) -> Result<Json<HealthStatusResponse>, StatusCode> {
    Ok(Json(HealthStatusResponse {
        ok: true,
        queue_depth: state.inbox.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?.len(),
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
}

async fn execute_handler(
    State(state): State<Arc<GatewayState>>,
    Json(payload): Json<ExecuteRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !(payload.capability_id.starts_with("ccos.chat.transform.")
        || payload.capability_id == "ccos.chat.egress.prepare_outbound")
    {
        return Err(StatusCode::FORBIDDEN);
    }

    let gateway = ChatGateway { state };
    let result = gateway
        .execute_capability(&payload.capability_id, payload.inputs)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(result))
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

    let gateway = ChatGateway { state: state.clone() };
    let prepared = gateway
        .execute_capability("ccos.chat.egress.prepare_outbound", rtfs_value_to_json(&Value::Map(inputs)).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?)
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
        .map_err(|e| {
            e
        });

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
