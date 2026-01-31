use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::{Duration as StdDuration, Instant};

use async_trait::async_trait;
use axum::extract::{Json, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::{routing::post, Router};
use base64::Engine as _;
use chrono::Utc;
use reqwest::Client;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::{oneshot, Mutex, RwLock};
use tokio::task::JoinHandle;
use uuid::Uuid;

use super::quarantine::QuarantineStore;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageDirection {
    Inbound,
    Outbound,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentRef {
    pub id: String,
    pub content_ref: String,
    pub content_type: String,
    pub size_bytes: u64,
    pub filename: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivationMetadata {
    pub matched: bool,
    pub trigger: Option<String>,
    pub rule_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEnvelope {
    pub id: String,
    pub channel_id: String,
    pub sender_id: String,
    pub timestamp: String,
    pub direction: MessageDirection,
    pub content_ref: String,
    pub attachments: Vec<AttachmentRef>,
    pub thread_id: Option<String>,
    pub reply_to: Option<String>,
    pub activation: Option<ActivationMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundRequest {
    pub channel_id: String,
    pub content: String,
    pub reply_to: Option<String>,
    pub metadata: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendResult {
    pub success: bool,
    pub message_id: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub ok: bool,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionHandle {
    pub id: String,
    pub bind_addr: String,
}

pub type EnvelopeCallback = Arc<
    dyn Fn(MessageEnvelope) -> futures::future::BoxFuture<'static, RuntimeResult<()>>
        + Send
        + Sync,
>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivationRules {
    pub allowed_senders: Vec<String>,
    pub allowed_channels: Vec<String>,
    pub required_mentions: Vec<String>,
    pub required_keywords: Vec<String>,
}

impl ActivationRules {
    pub fn is_allowed_sender(&self, sender_id: &str) -> bool {
        if self.allowed_senders.is_empty() {
            return false;
        }
        self.allowed_senders.iter().any(|s| s == sender_id)
    }

    pub fn is_allowed_channel(&self, channel_id: &str) -> bool {
        if self.allowed_channels.is_empty() {
            return false;
        }
        self.allowed_channels.iter().any(|c| c == channel_id)
    }

    pub fn match_trigger(&self, text: &str) -> Option<String> {
        for mention in &self.required_mentions {
            if text.contains(mention) {
                return Some(format!("mention:{}", mention));
            }
        }
        for keyword in &self.required_keywords {
            if text.contains(keyword) {
                return Some(format!("keyword:{}", keyword));
            }
        }
        None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopbackConnectorConfig {
    pub bind_addr: String,
    pub shared_secret: String,
    pub activation: ActivationRules,
    pub outbound_url: Option<String>,
    pub default_ttl_seconds: i64,
    pub min_send_interval_ms: u64,
}

#[async_trait]
pub trait ChatConnector: Send + Sync {
    async fn connect(&self) -> RuntimeResult<ConnectionHandle>;
    async fn disconnect(&self, handle: &ConnectionHandle) -> RuntimeResult<()>;
    async fn subscribe(&self, handle: &ConnectionHandle, callback: EnvelopeCallback)
        -> RuntimeResult<()>;
    async fn send(&self, handle: &ConnectionHandle, outbound: OutboundRequest) -> RuntimeResult<SendResult>;
    async fn health(&self, handle: &ConnectionHandle) -> RuntimeResult<HealthStatus>;
}

struct LoopbackConnectorState {
    config: LoopbackConnectorConfig,
    quarantine: Arc<dyn QuarantineStore>,
    callback: RwLock<Option<EnvelopeCallback>>,
}

#[derive(Clone)]
pub struct LoopbackWebhookConnector {
    state: Arc<LoopbackConnectorState>,
    client: Client,
    server_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
    shutdown_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    last_send_at: Arc<StdMutex<Option<Instant>>>,
}

impl LoopbackWebhookConnector {
    pub fn new(config: LoopbackConnectorConfig, quarantine: Arc<dyn QuarantineStore>) -> Self {
        let state = LoopbackConnectorState {
            config,
            quarantine,
            callback: RwLock::new(None),
        };
        Self {
            state: Arc::new(state),
            client: Client::new(),
            server_handle: Arc::new(Mutex::new(None)),
            shutdown_tx: Arc::new(Mutex::new(None)),
            last_send_at: Arc::new(StdMutex::new(None)),
        }
    }

    async fn start_server(&self) -> RuntimeResult<()> {
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let mut guard = self.shutdown_tx.lock().await;
        *guard = Some(shutdown_tx);
        drop(guard);

        let state = self.state.clone();
        let router = Router::new()
            .route("/connector/loopback/inbound", post(inbound_handler))
            .with_state(state);

        let addr: SocketAddr = self
            .state
            .config
            .bind_addr
            .parse()
            .map_err(|_| RuntimeError::Generic("Invalid bind_addr".to_string()))?;

        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to bind connector: {}", e)))?;
        let server = axum::serve(listener, router.into_make_service()).with_graceful_shutdown(
            async move {
                let _ = shutdown_rx.await;
            },
        );

        let handle = tokio::spawn(async move {
            let _ = server.await;
        });

        let mut handle_guard = self.server_handle.lock().await;
        *handle_guard = Some(handle);
        Ok(())
    }

    fn enforce_rate_limit(&self) -> RuntimeResult<()> {
        let mut guard = self
            .last_send_at
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock rate limiter".to_string()))?;
        let now = Instant::now();
        if let Some(last) = *guard {
            let elapsed = now.duration_since(last);
            let min_interval = StdDuration::from_millis(self.state.config.min_send_interval_ms);
            if elapsed < min_interval {
                return Err(RuntimeError::Generic("Outbound rate limit exceeded".to_string()));
            }
        }
        *guard = Some(now);
        Ok(())
    }
}

#[async_trait]
impl ChatConnector for LoopbackWebhookConnector {
    async fn connect(&self) -> RuntimeResult<ConnectionHandle> {
        self.start_server().await?;
        Ok(ConnectionHandle {
            id: Uuid::new_v4().to_string(),
            bind_addr: self.state.config.bind_addr.clone(),
        })
    }

    async fn disconnect(&self, _handle: &ConnectionHandle) -> RuntimeResult<()> {
        let mut guard = self.shutdown_tx.lock().await;
        if let Some(tx) = guard.take() {
            let _ = tx.send(());
        }
        Ok(())
    }

    async fn subscribe(
        &self,
        _handle: &ConnectionHandle,
        callback: EnvelopeCallback,
    ) -> RuntimeResult<()> {
        let mut guard = self.state.callback.write().await;
        *guard = Some(callback);
        Ok(())
    }

    async fn send(&self, _handle: &ConnectionHandle, outbound: OutboundRequest) -> RuntimeResult<SendResult> {
        self.enforce_rate_limit()?;
        let Some(outbound_url) = &self.state.config.outbound_url else {
            return Err(RuntimeError::Generic("Outbound URL not configured".to_string()));
        };

        let resp = self
            .client
            .post(outbound_url)
            .json(&outbound)
            .send()
            .await
            .map_err(|e| RuntimeError::Generic(format!("Outbound send failed: {}", e)))?;

        if !resp.status().is_success() {
            return Ok(SendResult {
                success: false,
                message_id: None,
                error: Some(format!("Outbound returned status {}", resp.status())),
            });
        }

        Ok(SendResult {
            success: true,
            message_id: Some(Uuid::new_v4().to_string()),
            error: None,
        })
    }

    async fn health(&self, _handle: &ConnectionHandle) -> RuntimeResult<HealthStatus> {
        Ok(HealthStatus {
            ok: true,
            details: Some("loopback connector active".to_string()),
        })
    }
}

#[derive(Debug, Deserialize)]
struct LoopbackInboundPayload {
    channel_id: String,
    sender_id: String,
    text: String,
    thread_id: Option<String>,
    reply_to: Option<String>,
    timestamp: Option<String>,
    attachments: Option<Vec<LoopbackInboundAttachment>>,
}

#[derive(Debug, Deserialize)]
struct LoopbackInboundAttachment {
    content_b64: String,
    content_type: Option<String>,
    filename: Option<String>,
}

#[derive(Debug, Serialize)]
struct LoopbackInboundResponse {
    accepted: bool,
    message_id: Option<String>,
    error: Option<String>,
}

async fn inbound_handler(
    State(state): State<Arc<LoopbackConnectorState>>,
    headers: HeaderMap,
    Json(payload): Json<LoopbackInboundPayload>,
) -> impl IntoResponse {
    let secret = headers
        .get("x-ccos-connector-secret")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if secret != state.config.shared_secret {
        return (
            StatusCode::UNAUTHORIZED,
            Json(LoopbackInboundResponse {
                accepted: false,
                message_id: None,
                error: Some("unauthorized".to_string()),
            }),
        );
    }

    if !state.config.activation.is_allowed_sender(&payload.sender_id)
        || !state.config.activation.is_allowed_channel(&payload.channel_id)
    {
        return (
            StatusCode::OK,
            Json(LoopbackInboundResponse {
                accepted: false,
                message_id: None,
                error: None,
            }),
        );
    }

    let trigger = state.config.activation.match_trigger(&payload.text);
    if trigger.is_none() {
        return (
            StatusCode::OK,
            Json(LoopbackInboundResponse {
                accepted: false,
                message_id: None,
                error: None,
            }),
        );
    }

    let ttl = chrono::Duration::seconds(state.config.default_ttl_seconds);
    let content_ref = match state.quarantine.put_bytes(payload.text.into_bytes(), ttl) {
        Ok(pointer) => pointer,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(LoopbackInboundResponse {
                    accepted: false,
                    message_id: None,
                    error: Some(format!("quarantine error: {}", e)),
                }),
            );
        }
    };

    let mut attachments = Vec::new();
    if let Some(inbound_attachments) = payload.attachments {
        for attachment in inbound_attachments {
            let bytes = match base64::engine::general_purpose::STANDARD
                .decode(attachment.content_b64.as_bytes())
            {
                Ok(b) => b,
                Err(_) => {
                    continue;
                }
            };
            let size_bytes = bytes.len() as u64;
            let pointer = match state.quarantine.put_bytes(bytes, ttl) {
                Ok(p) => p,
                Err(_) => continue,
            };
            attachments.push(AttachmentRef {
                id: Uuid::new_v4().to_string(),
                content_ref: pointer,
                content_type: attachment
                    .content_type
                    .unwrap_or_else(|| "application/octet-stream".to_string()),
                size_bytes,
                filename: attachment.filename,
            });
        }
    }

    let message_id = Uuid::new_v4().to_string();
    let timestamp = payload
        .timestamp
        .unwrap_or_else(|| Utc::now().to_rfc3339());
    let envelope = MessageEnvelope {
        id: message_id.clone(),
        channel_id: payload.channel_id,
        sender_id: payload.sender_id,
        timestamp,
        direction: MessageDirection::Inbound,
        content_ref,
        attachments,
        thread_id: payload.thread_id,
        reply_to: payload.reply_to,
        activation: Some(ActivationMetadata {
            matched: true,
            trigger,
            rule_id: Some("loopback.activation".to_string()),
        }),
        session_id: None,
        run_id: None,
        step_id: None,
    };

    if let Some(callback) = state.callback.read().await.clone() {
        if let Err(e) = callback(envelope).await {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(LoopbackInboundResponse {
                    accepted: false,
                    message_id: None,
                    error: Some(format!("callback error: {}", e)),
                }),
            );
        }
    }

    (
        StatusCode::OK,
        Json(LoopbackInboundResponse {
            accepted: true,
            message_id: Some(message_id),
            error: None,
        }),
    )
}
