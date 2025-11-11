// streaming.rs
// All stream-related types, traits, and aliases for CCOS/RTFS

use chrono::Utc;
use eventsource_stream::Event as SseMessage;
use futures::StreamExt;
use log::{error, info, warn};
use reqwest::{Client, Url};
use reqwest_eventsource::{Event as ReqwestEvent, EventSource};
use rtfs::ast::{Keyword, MapKey, TypeExpr};
use rtfs::runtime::{
    error::{RuntimeError, RuntimeResult},
    type_validator::{TypeCheckingConfig, TypeValidator, VerificationContext},
    values::Value,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::{hash_map::Entry, HashMap, HashSet, VecDeque};
use std::env;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Default and environment configuration for MCP streaming transports
pub const DEFAULT_LOCAL_MCP_SSE_ENDPOINT: &str = "http://127.0.0.1:2025/sse";
pub const ENV_MCP_STREAM_ENDPOINT: &str = "CCOS_MCP_STREAM_ENDPOINT";
pub const ENV_LOCAL_MCP_SSE_URL: &str = "CCOS_MCP_LOCAL_SSE_URL";
pub const ENV_LEGACY_CLOUDFLARE_DOCS_SSE_URL: &str = "CCOS_MCP_CLOUDFLARE_DOCS_SSE_URL";
pub const ENV_MCP_STREAM_AUTH_HEADER: &str = "CCOS_MCP_STREAM_AUTH_HEADER";
pub const ENV_MCP_STREAM_BEARER_TOKEN: &str = "CCOS_MCP_STREAM_BEARER_TOKEN";
pub const ENV_MCP_STREAM_AUTO_CONNECT: &str = "CCOS_MCP_STREAM_AUTO_CONNECT";

/// Streaming type for capabilities
#[derive(Debug, Clone, PartialEq)]
pub enum StreamType {
    Unidirectional,
    Bidirectional,
    Duplex,
}

/// Bidirectional stream configuration
#[derive(Debug, Clone, PartialEq)]
pub struct BidirectionalConfig {
    pub client_channel: String,
    pub server_channel: String,
    pub buffer_size: usize,
}

/// Duplex channel configuration
#[derive(Debug, Clone, PartialEq)]
pub struct DuplexChannels {
    pub input_channel: String,
    pub output_channel: String,
    pub buffer_size: usize,
}

/// Progress notification for streaming
#[derive(Debug, Clone, PartialEq)]
pub struct ProgressNotification {
    pub progress: f32,
    pub message: Option<String>,
}

/// Stream callback trait
pub trait StreamCallback: Send + Sync {
    fn on_progress(&self, notification: &ProgressNotification);
    fn on_complete(&self);
    fn on_error(&self, error: &str);
}

/// Optional callbacks for stream configuration
#[derive(Clone)]
pub struct StreamCallbacks {
    pub progress: Option<Arc<dyn StreamCallback>>,
    pub complete: Option<Arc<dyn StreamCallback>>,
    pub error: Option<Arc<dyn StreamCallback>>,
}

impl std::fmt::Debug for StreamCallbacks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamCallbacks")
            .field("progress", &self.progress.is_some())
            .field("complete", &self.complete.is_some())
            .field("error", &self.error.is_some())
            .finish()
    }
}

/// Stream configuration
#[derive(Clone)]
pub struct StreamConfig {
    pub callbacks: Option<StreamCallbacks>,
    pub auto_reconnect: bool,
    pub max_retries: u32,
}

impl std::fmt::Debug for StreamConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamConfig")
            .field("callbacks", &self.callbacks.as_ref().map(|_| "<callbacks>"))
            .field("auto_reconnect", &self.auto_reconnect)
            .field("max_retries", &self.max_retries)
            .finish()
    }
}

impl PartialEq for StreamConfig {
    fn eq(&self, other: &Self) -> bool {
        self.auto_reconnect == other.auto_reconnect && self.max_retries == other.max_retries
    }
}

#[async_trait::async_trait]
pub trait StreamChunkSink: Send + Sync {
    async fn ingest_chunk(
        &self,
        stream_id: &str,
        chunk: Value,
        metadata: Value,
    ) -> RuntimeResult<()>;
}

pub struct StreamTransportArgs {
    pub stream_id: String,
    pub endpoint: String,
    pub stop_rx: mpsc::Receiver<()>,
    pub sink: Arc<dyn StreamChunkSink>,
    pub client_config: McpClientConfig,
}

#[async_trait::async_trait]
pub trait StreamTransport: Send + Sync {
    async fn run(&self, args: StreamTransportArgs) -> RuntimeResult<()>;
}

/// Handle for managing a stream
#[derive(Debug, Clone)]
pub struct StreamHandle {
    pub stream_id: String,
    pub stop_tx: mpsc::Sender<()>,
}

/// Trait for streaming capability providers
#[async_trait::async_trait]
pub trait StreamingCapability {
    /// Start a stream
    fn start_stream(
        &self,
        params: &rtfs::runtime::values::Value,
    ) -> rtfs::runtime::error::RuntimeResult<StreamHandle>;
    /// Stop a stream
    fn stop_stream(&self, handle: &StreamHandle) -> rtfs::runtime::error::RuntimeResult<()>;
    /// Start a stream with extended configuration
    async fn start_stream_with_config(
        &self,
        params: &rtfs::runtime::values::Value,
        config: &StreamConfig,
    ) -> rtfs::runtime::error::RuntimeResult<StreamHandle>;
    /// Send data to a stream
    async fn send_to_stream(
        &self,
        handle: &StreamHandle,
        data: &rtfs::runtime::values::Value,
    ) -> rtfs::runtime::error::RuntimeResult<()>;
    /// Start a bidirectional stream
    fn start_bidirectional_stream(
        &self,
        params: &rtfs::runtime::values::Value,
    ) -> rtfs::runtime::error::RuntimeResult<StreamHandle>;
    /// Start a bidirectional stream with extended configuration
    async fn start_bidirectional_stream_with_config(
        &self,
        params: &rtfs::runtime::values::Value,
        config: &StreamConfig,
    ) -> rtfs::runtime::error::RuntimeResult<StreamHandle>;
    /// Provide access to the concrete type for dynamic downcasting
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Type alias for a thread-safe, shareable streaming capability provider
pub type StreamingProvider = Arc<dyn StreamingCapability + Send + Sync>;

/// Persisted snapshot of a stream processor registration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamSnapshot {
    pub stream_id: String,
    pub processor_fn: String,
    pub current_state: Value,
    pub status: StreamStatus,
    pub continuation: Vec<u8>,
}

/// Storage trait abstracting persistence of stream snapshots for Phase 4.
pub trait StreamPersistence: Send + Sync {
    fn persist_snapshot(&self, snapshot: &StreamSnapshot) -> Result<(), String>;
    fn load_snapshot(&self, stream_id: &str) -> Result<Option<StreamSnapshot>, String>;
    fn remove_snapshot(&self, stream_id: &str) -> Result<(), String>;
}

/// Simple in-memory persistence used for tests and bootstrap scenarios.
#[derive(Default, Debug, Clone)]
pub struct InMemoryStreamPersistence {
    inner: Arc<Mutex<HashMap<String, StreamSnapshot>>>,
}

impl InMemoryStreamPersistence {
    pub fn new() -> Self {
        Self::default()
    }
}

impl StreamPersistence for InMemoryStreamPersistence {
    fn persist_snapshot(&self, snapshot: &StreamSnapshot) -> Result<(), String> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| "stream persistence poisoned".to_string())?;
        guard.insert(snapshot.stream_id.clone(), snapshot.clone());
        Ok(())
    }

    fn load_snapshot(&self, stream_id: &str) -> Result<Option<StreamSnapshot>, String> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| "stream persistence poisoned".to_string())?;
        Ok(guard.get(stream_id).cloned())
    }

    fn remove_snapshot(&self, stream_id: &str) -> Result<(), String> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| "stream persistence poisoned".to_string())?;
        guard.remove(stream_id);
        Ok(())
    }
}

/// MCP-specific streaming provider for Model Context Protocol endpoints
#[derive(Clone)]
pub struct McpStreamingProvider {
    /// Base MCP client configuration
    pub client_config: McpClientConfig,
    /// Stream processor registry for continuation management
    pub stream_processors: Arc<Mutex<HashMap<String, StreamProcessorRegistration>>>,
    /// Optional processor invoker hook (Phase 3) allowing real RTFS function invocation
    /// Signature: (processor_fn, state, chunk, metadata) -> result map
    processor_invoker:
        Option<Arc<dyn Fn(&str, &Value, &Value, &Value) -> RuntimeResult<Value> + Send + Sync>>,
    /// Optional persistence backend (Phase 4) for continuation snapshots
    persistence: Option<Arc<dyn StreamPersistence>>,
    /// Active background transport tasks keyed by stream id (Phase 6)
    stream_tasks: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
    /// Transport implementation responsible for fetching incoming chunks
    transport: Arc<dyn StreamTransport>,
    /// Type validator shared across schema checks
    type_validator: Arc<TypeValidator>,
    /// Optional schema declarations keyed by stream id
    stream_schemas: Arc<Mutex<HashMap<String, Option<TypeExpr>>>>,
}

#[derive(Debug, Clone)]
pub struct McpClientConfig {
    pub server_url: String,
    pub timeout_ms: u64,
    pub retry_attempts: u32,
    pub auth_header: Option<String>,
    pub auto_connect: bool,
}

#[derive(Clone)]
pub struct StreamProcessorRegistration {
    pub processor_fn: String, // Function name to call (placeholder until real invocation)
    pub continuation: Vec<u8>, // Serialized continuation data (future)
    pub initial_state: Value, // Original starting state
    pub current_state: Value, // Mutable logical state updated per chunk (Phase 1 prototype)
    pub status: StreamStatus, // Directive/status lifecycle tracking (Phase 2 prototype)
    pub queue_capacity: usize, // Maximum number of queued chunks (Phase 5)
    pub stats: StreamStats,   // Basic metrics for introspection (Phase 5)
    pub result_schema: Option<TypeExpr>,
    queue: VecDeque<QueuedItem>, // Pending chunks waiting to be processed (Phase 5)
}

impl StreamProcessorRegistration {
    fn enqueue_chunk(&mut self, chunk: Value, metadata: Value) -> bool {
        if self.queue.len() >= self.queue_capacity {
            self.status = StreamStatus::Paused;
            return false;
        }
        self.queue.push_back(QueuedItem {
            chunk,
            metadata,
            enqueued_at: Instant::now(),
        });
        self.stats.queued_chunks = self.queue.len();
        true
    }

    fn dequeue_next(&mut self) -> Option<QueuedItem> {
        let item = self.queue.pop_front();
        self.stats.queued_chunks = self.queue.len();
        item
    }
}

/// Lifecycle status for a stream processor registration.
/// This will expand as richer directives are supported (e.g., backpressure, inject, error details).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamStatus {
    Active,
    Paused,
    Cancelled,
    Completed,
    Stopped,
    Error(String),
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StreamStats {
    pub processed_chunks: usize,
    pub queued_chunks: usize,
    pub last_latency_ms: Option<u128>,
    #[serde(default)]
    pub last_event_epoch_ms: Option<i64>,
}

#[derive(Clone)]
struct QueuedItem {
    chunk: Value,
    metadata: Value,
    enqueued_at: Instant,
}

#[derive(Clone, Copy, Debug)]
pub struct StreamInspectOptions {
    pub include_state: bool,
    pub include_initial_state: bool,
    pub include_queue: bool,
}

impl Default for StreamInspectOptions {
    fn default() -> Self {
        Self {
            include_state: true,
            include_initial_state: true,
            include_queue: true,
        }
    }
}

impl McpStreamingProvider {
    pub fn new(server_url: String) -> Self {
        Self::new_internal(server_url, None, None, None)
    }

    /// Construct with a custom processor invoker (used by Phase 3 tests to supply evaluator)
    pub fn new_with_invoker(
        server_url: String,
        invoker: Arc<dyn Fn(&str, &Value, &Value, &Value) -> RuntimeResult<Value> + Send + Sync>,
    ) -> Self {
        Self::new_internal(server_url, None, Some(invoker), None)
    }

    /// Construct with explicit persistence backend and optional processor invoker.
    pub fn new_with_persistence(
        server_url: String,
        persistence: Arc<dyn StreamPersistence>,
        invoker: Option<
            Arc<dyn Fn(&str, &Value, &Value, &Value) -> RuntimeResult<Value> + Send + Sync>,
        >,
    ) -> Self {
        Self::new_internal(server_url, Some(persistence), invoker, None)
    }

    /// Construct with an explicit transport implementation (primarily for tests or alternative transports).
    pub fn new_with_transport(server_url: String, transport: Arc<dyn StreamTransport>) -> Self {
        Self::new_internal(server_url, None, None, Some(transport))
    }

    fn new_internal(
        server_url: String,
        persistence: Option<Arc<dyn StreamPersistence>>,
        invoker: Option<
            Arc<dyn Fn(&str, &Value, &Value, &Value) -> RuntimeResult<Value> + Send + Sync>,
        >,
        transport: Option<Arc<dyn StreamTransport>>,
    ) -> Self {
        let client_config = Self::resolve_client_config(server_url);
        let default_transport: Arc<dyn StreamTransport> = transport.unwrap_or_else(|| {
            let http_client = Client::builder()
                .cookie_store(true)
                .build()
                .unwrap_or_else(|_| Client::new());
            Arc::new(SseStreamTransport::new(http_client)) as Arc<dyn StreamTransport>
        });

        Self {
            client_config,
            stream_processors: Arc::new(Mutex::new(HashMap::new())),
            processor_invoker: invoker,
            persistence,
            stream_tasks: Arc::new(Mutex::new(HashMap::new())),
            transport: default_transport,
            type_validator: Arc::new(TypeValidator::new()),
            stream_schemas: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn resolve_client_config(server_url: String) -> McpClientConfig {
        let trimmed = server_url.trim();
        let explicit = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };

        let resolved_url = explicit
            .clone()
            .or_else(|| {
                env::var(ENV_MCP_STREAM_ENDPOINT)
                    .ok()
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty())
            })
            .or_else(|| {
                env::var(ENV_LOCAL_MCP_SSE_URL)
                    .ok()
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty())
            })
            .or_else(|| {
                env::var(ENV_LEGACY_CLOUDFLARE_DOCS_SSE_URL)
                    .ok()
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty())
            })
            .unwrap_or_else(|| DEFAULT_LOCAL_MCP_SSE_ENDPOINT.to_string());

        let header_from_env = env::var(ENV_MCP_STREAM_AUTH_HEADER)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        let bearer = env::var(ENV_MCP_STREAM_BEARER_TOKEN)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        let auth_header = match (header_from_env, bearer) {
            (Some(header), _) => Some(header),
            (None, Some(token)) => Some(format!("Authorization: Bearer {}", token)),
            _ => None,
        };

        let auto_connect = match env::var(ENV_MCP_STREAM_AUTO_CONNECT) {
            Ok(value) => {
                let normalized = value.trim().to_ascii_lowercase();
                matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
            }
            Err(_) => explicit.is_none(),
        };

        McpClientConfig {
            server_url: resolved_url,
            timeout_ms: 30000,
            retry_attempts: 3,
            auth_header,
            auto_connect,
        }
    }

    /// Expose resolved client configuration (clone) for inspection / tests.
    pub fn client_config(&self) -> McpClientConfig {
        self.client_config.clone()
    }

    fn spawn_transport_task(
        &self,
        stream_id: String,
        endpoint: String,
        stop_rx: mpsc::Receiver<()>,
    ) {
        let tasks_map = self.stream_tasks.clone();
        let transport = self.transport.clone();
        let cfg = self.client_config.clone();
        let sink: Arc<dyn StreamChunkSink> = Arc::new(self.clone());
        let stream_id_for_map = stream_id.clone();
        let stream_id_for_task = stream_id.clone();
        let args = StreamTransportArgs {
            stream_id,
            endpoint,
            stop_rx,
            sink,
            client_config: cfg,
        };
        let task = tokio::spawn(async move {
            if let Err(err) = transport.run(args).await {
                error!(
                    "Transport loop for stream {} ended with error: {}",
                    stream_id_for_task, err
                );
            }
            if let Ok(mut guard) = tasks_map.lock() {
                guard.remove(&stream_id_for_task);
            }
        });
        if let Ok(mut guard) = self.stream_tasks.lock() {
            guard.insert(stream_id_for_map, task);
        }
    }

    /// Register a stream processor for continuation-based processing
    pub fn register_processor(
        &self,
        stream_id: String,
        processor_fn: String,
        continuation: Vec<u8>,
        initial_state: Value,
    ) {
        let existing_schema = self
            .stream_schemas
            .lock()
            .map(|guard| guard.get(&stream_id).cloned())
            .unwrap_or(None)
            .flatten();
        let mut processors = self.stream_processors.lock().unwrap();
        processors.insert(
            stream_id.clone(),
            StreamProcessorRegistration {
                processor_fn,
                continuation,
                current_state: initial_state.clone(),
                initial_state,
                status: StreamStatus::Active,
                queue_capacity: self.default_queue_capacity(),
                stats: StreamStats::default(),
                result_schema: existing_schema,
                queue: VecDeque::new(),
            },
        );
        if let Some(registration) = processors.get(&stream_id) {
            self.persist_registration(&stream_id, registration);
        }
    }

    /// Update schema metadata for an existing stream processor
    pub fn set_processor_schema(&self, stream_id: &str, schema: Option<TypeExpr>) {
        if let Ok(mut schemas) = self.stream_schemas.lock() {
            if let Some(schema_clone) = schema.clone() {
                schemas.insert(stream_id.to_string(), Some(schema_clone));
            } else {
                schemas.remove(stream_id);
            }
        }

        if let Ok(mut processors) = self.stream_processors.lock() {
            if let Some(registration) = processors.get_mut(stream_id) {
                registration.result_schema = schema;
            }
        }
    }

    fn snapshot_for(
        &self,
        stream_id: &str,
        registration: &StreamProcessorRegistration,
    ) -> StreamSnapshot {
        StreamSnapshot {
            stream_id: stream_id.to_string(),
            processor_fn: registration.processor_fn.clone(),
            current_state: registration.current_state.clone(),
            status: registration.status.clone(),
            continuation: registration.continuation.clone(),
        }
    }

    fn persist_registration(&self, stream_id: &str, registration: &StreamProcessorRegistration) {
        if let Some(persistence) = &self.persistence {
            let snapshot = self.snapshot_for(stream_id, registration);
            if let Err(err) = persistence.persist_snapshot(&snapshot) {
                warn!(
                    "Failed to persist snapshot for stream {}: {}",
                    stream_id, err
                );
            }
        }
    }

    fn remove_persisted(&self, stream_id: &str) {
        if let Some(persistence) = &self.persistence {
            if let Err(err) = persistence.remove_snapshot(stream_id) {
                warn!(
                    "Failed to remove persisted snapshot for stream {}: {}",
                    stream_id, err
                );
            }
        }
    }

    fn stats_to_value(stats: &StreamStats) -> Value {
        use rtfs::ast::{Keyword, MapKey};
        let mut map = HashMap::new();
        map.insert(
            MapKey::Keyword(Keyword("processed-chunks".into())),
            Value::Integer(stats.processed_chunks as i64),
        );
        map.insert(
            MapKey::Keyword(Keyword("queued-chunks".into())),
            Value::Integer(stats.queued_chunks as i64),
        );
        map.insert(
            MapKey::Keyword(Keyword("last-latency-ms".into())),
            stats
                .last_latency_ms
                .map(|ms| Value::Integer(ms as i64))
                .unwrap_or(Value::Nil),
        );
        map.insert(
            MapKey::Keyword(Keyword("last-event-epoch-ms".into())),
            stats
                .last_event_epoch_ms
                .map(Value::Integer)
                .unwrap_or(Value::Nil),
        );
        Value::Map(map)
    }

    fn status_to_value(status: &StreamStatus) -> Value {
        use rtfs::ast::{Keyword, MapKey};
        let mut map = HashMap::new();
        let status_keyword = match status {
            StreamStatus::Active => "active",
            StreamStatus::Paused => "paused",
            StreamStatus::Cancelled => "cancelled",
            StreamStatus::Completed => "completed",
            StreamStatus::Stopped => "stopped",
            StreamStatus::Error(_) => "error",
        };
        map.insert(
            MapKey::Keyword(Keyword("state".into())),
            Value::Keyword(Keyword(status_keyword.to_string())),
        );
        if let StreamStatus::Error(err) = status {
            map.insert(
                MapKey::Keyword(Keyword("message".into())),
                Value::String(err.clone()),
            );
        }
        Value::Map(map)
    }

    fn queued_item_to_value(item: &QueuedItem) -> Value {
        use rtfs::ast::{Keyword, MapKey};
        let mut map = HashMap::new();
        map.insert(MapKey::Keyword(Keyword("chunk".into())), item.chunk.clone());
        map.insert(
            MapKey::Keyword(Keyword("metadata".into())),
            item.metadata.clone(),
        );
        map.insert(
            MapKey::Keyword(Keyword("waiting-ms".into())),
            Value::Integer(item.enqueued_at.elapsed().as_millis() as i64),
        );
        Value::Map(map)
    }

    pub fn inspect_stream(
        &self,
        stream_id: &str,
        options: StreamInspectOptions,
    ) -> RuntimeResult<Value> {
        let registration = {
            let processors = self.stream_processors.lock().unwrap();
            processors.get(stream_id).cloned()
        }
        .ok_or_else(|| {
            RuntimeError::Generic(format!("No processor registered for stream: {}", stream_id))
        })?;

        let (task_present, task_active) = {
            let tasks = self.stream_tasks.lock().unwrap();
            if let Some(handle) = tasks.get(stream_id) {
                (true, !handle.is_finished())
            } else {
                (false, false)
            }
        };

        let queue_snapshot = if options.include_queue {
            registration
                .queue
                .iter()
                .map(Self::queued_item_to_value)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        use rtfs::ast::{Keyword, MapKey};
        let mut result = HashMap::new();
        result.insert(
            MapKey::Keyword(Keyword("stream-id".into())),
            Value::String(stream_id.to_string()),
        );
        result.insert(
            MapKey::Keyword(Keyword("processor".into())),
            Value::String(registration.processor_fn.clone()),
        );
        result.insert(
            MapKey::Keyword(Keyword("status".into())),
            Self::status_to_value(&registration.status),
        );
        result.insert(
            MapKey::Keyword(Keyword("queue-capacity".into())),
            Value::Integer(registration.queue_capacity as i64),
        );
        result.insert(
            MapKey::Keyword(Keyword("stats".into())),
            Self::stats_to_value(&registration.stats),
        );
        result.insert(
            MapKey::Keyword(Keyword("observed-at-epoch-ms".into())),
            Value::Integer(Utc::now().timestamp_millis()),
        );
        result.insert(
            MapKey::Keyword(Keyword("persistence-enabled".into())),
            Value::Boolean(self.persistence.is_some()),
        );

        let mut transport_map = HashMap::new();
        transport_map.insert(
            MapKey::Keyword(Keyword("auto-connect".into())),
            Value::Boolean(self.client_config.auto_connect),
        );
        transport_map.insert(
            MapKey::Keyword(Keyword("task-present".into())),
            Value::Boolean(task_present),
        );
        transport_map.insert(
            MapKey::Keyword(Keyword("task-active".into())),
            Value::Boolean(task_active),
        );
        transport_map.insert(
            MapKey::Keyword(Keyword("timeout-ms".into())),
            Value::Integer(self.client_config.timeout_ms as i64),
        );
        transport_map.insert(
            MapKey::Keyword(Keyword("retry-attempts".into())),
            Value::Integer(self.client_config.retry_attempts as i64),
        );
        transport_map.insert(
            MapKey::Keyword(Keyword("server-url".into())),
            Value::String(self.client_config.server_url.clone()),
        );
        result.insert(
            MapKey::Keyword(Keyword("transport".into())),
            Value::Map(transport_map),
        );

        if options.include_state {
            result.insert(
                MapKey::Keyword(Keyword("current-state".into())),
                registration.current_state.clone(),
            );
        }
        if options.include_initial_state {
            result.insert(
                MapKey::Keyword(Keyword("initial-state".into())),
                registration.initial_state.clone(),
            );
        }
        if options.include_queue {
            result.insert(
                MapKey::Keyword(Keyword("queue".into())),
                Value::Vector(queue_snapshot),
            );
        }

        Ok(Value::Map(result))
    }

    pub fn inspect_streams(&self, options: StreamInspectOptions) -> Value {
        let stream_ids = {
            let processors = self.stream_processors.lock().unwrap();
            processors.keys().cloned().collect::<Vec<_>>()
        };

        let mut streams = Vec::with_capacity(stream_ids.len());
        for stream_id in stream_ids {
            if let Ok(value) = self.inspect_stream(&stream_id, options) {
                streams.push(value);
            }
        }

        use rtfs::ast::{Keyword, MapKey};
        let mut map = HashMap::new();
        map.insert(
            MapKey::Keyword(Keyword("total".into())),
            Value::Integer(streams.len() as i64),
        );
        map.insert(
            MapKey::Keyword(Keyword("observed-at-epoch-ms".into())),
            Value::Integer(Utc::now().timestamp_millis()),
        );
        map.insert(
            MapKey::Keyword(Keyword("auto-connect".into())),
            Value::Boolean(self.client_config.auto_connect),
        );
        map.insert(
            MapKey::Keyword(Keyword("persistence-enabled".into())),
            Value::Boolean(self.persistence.is_some()),
        );
        map.insert(
            MapKey::Keyword(Keyword("server-url".into())),
            Value::String(self.client_config.server_url.clone()),
        );
        map.insert(
            MapKey::Keyword(Keyword("streams".into())),
            Value::Vector(streams),
        );

        Value::Map(map)
    }

    /// Process a stream chunk by resuming RTFS execution
    pub async fn process_chunk(
        &self,
        stream_id: &str,
        chunk: Value,
        metadata: Value,
    ) -> RuntimeResult<()> {
        let mut processors = self.stream_processors.lock().unwrap();
        if let Some(registration) = processors.get_mut(stream_id) {
            registration.stats.last_event_epoch_ms = Some(Utc::now().timestamp_millis());
            match &registration.status {
                StreamStatus::Completed
                | StreamStatus::Stopped
                | StreamStatus::Error(_)
                | StreamStatus::Cancelled => {
                    return Ok(());
                }
                StreamStatus::Paused => {
                    if let Some(action) = Self::extract_action(&chunk) {
                        self.handle_directive_chunk(registration, chunk, metadata, action)?;
                    } else if !registration.enqueue_chunk(chunk, metadata) {
                        self.persist_registration(stream_id, registration);
                        warn!(
                            "Stream queue full for {} (processor: {})",
                            stream_id, registration.processor_fn
                        );
                        return Err(RuntimeError::Generic("Stream queue is full".into()));
                    }
                    self.persist_registration(stream_id, registration);
                    return Ok(());
                }
                StreamStatus::Active => {}
            }

            if let Some(action) = Self::extract_action(&chunk) {
                self.handle_directive_chunk(
                    registration,
                    chunk.clone(),
                    metadata.clone(),
                    action.clone(),
                )?;
                if matches!(
                    registration.status,
                    StreamStatus::Completed
                        | StreamStatus::Stopped
                        | StreamStatus::Error(_)
                        | StreamStatus::Cancelled
                ) {
                    self.persist_registration(stream_id, registration);
                    return Ok(());
                }
                if registration.status == StreamStatus::Paused {
                    self.persist_registration(stream_id, registration);
                    return Ok(());
                }
            } else {
                if !registration.enqueue_chunk(chunk, metadata) {
                    return Err(RuntimeError::Generic("Stream queue is full".into()));
                }
            }

            while let Some(next_item) = registration.dequeue_next() {
                let item = next_item;
                if registration.status == StreamStatus::Paused {
                    registration.enqueue_chunk(item.chunk, item.metadata);
                    self.persist_registration(stream_id, registration);
                    break;
                }

                if let Err(e) = self.process_single_chunk(registration, item) {
                    return Err(e);
                }

                if matches!(
                    registration.status,
                    StreamStatus::Completed
                        | StreamStatus::Stopped
                        | StreamStatus::Error(_)
                        | StreamStatus::Cancelled
                ) {
                    break;
                }
            }

            self.persist_registration(stream_id, registration);
            info!(
                "Processed chunk for stream {} (status: {:?}, processed: {})",
                stream_id, registration.status, registration.stats.processed_chunks
            );
            Ok(())
        } else {
            Err(RuntimeError::Generic(format!(
                "No processor registered for stream: {}",
                stream_id
            )))
        }
    }

    /// Get current state for a stream (testing/introspection helper)
    pub fn get_current_state(&self, stream_id: &str) -> Option<Value> {
        let processors = self.stream_processors.lock().unwrap();
        processors.get(stream_id).map(|r| r.current_state.clone())
    }

    /// Get current status of a stream for testing/introspection
    pub fn get_status(&self, stream_id: &str) -> Option<StreamStatus> {
        let processors = self.stream_processors.lock().unwrap();
        processors.get(stream_id).map(|r| r.status.clone())
    }

    /// Resume a stream from persisted snapshot (Phase 4)
    pub fn resume_stream(&self, stream_id: &str) -> RuntimeResult<()> {
        let persistence = self.persistence.clone().ok_or_else(|| {
            RuntimeError::Generic("Stream persistence backend not configured".into())
        })?;
        let snapshot = persistence
            .load_snapshot(stream_id)
            .map_err(|e| RuntimeError::Generic(format!("Failed to load snapshot: {}", e)))?
            .ok_or_else(|| {
                RuntimeError::Generic(format!("No persisted snapshot for stream: {}", stream_id))
            })?;

        let existing_schema = self
            .stream_schemas
            .lock()
            .map(|guard| guard.get(stream_id).cloned())
            .unwrap_or(None)
            .flatten();

        let mut processors = self.stream_processors.lock().unwrap();
        processors.insert(
            stream_id.to_string(),
            StreamProcessorRegistration {
                processor_fn: snapshot.processor_fn,
                continuation: snapshot.continuation,
                initial_state: snapshot.current_state.clone(),
                current_state: snapshot.current_state,
                status: snapshot.status,
                queue_capacity: self.default_queue_capacity(),
                stats: StreamStats::default(),
                result_schema: existing_schema,
                queue: VecDeque::new(),
            },
        );
        if let Some(registration) = processors.get(stream_id) {
            self.persist_registration(stream_id, registration);
        }
        Ok(())
    }

    fn default_queue_capacity(&self) -> usize {
        32
    }

    fn process_single_chunk(
        &self,
        registration: &mut StreamProcessorRegistration,
        item: QueuedItem,
    ) -> RuntimeResult<()> {
        let start_time = item.enqueued_at;
        let chunk = item.chunk;
        let metadata = item.metadata;

        let mut invoked = false;
        if let Some(invoker) = &self.processor_invoker {
            if !registration.processor_fn.is_empty() {
                match invoker(
                    &registration.processor_fn,
                    &registration.current_state,
                    &chunk,
                    &metadata,
                ) {
                    Ok(result_val) => {
                        invoked = true;
                        self.validate_processor_output(registration, &result_val)?;
                        self.apply_processor_result(registration, result_val)?;
                    }
                    Err(e) => {
                        registration.status =
                            StreamStatus::Error(format!("Processor invocation error: {}", e));
                        return Err(e);
                    }
                }
            }
        }

        if !invoked {
            let mut new_state = registration.current_state.clone();
            if let Value::Map(m) = &mut new_state {
                use rtfs::ast::{Keyword, MapKey};
                let count_key = MapKey::Keyword(Keyword("count".to_string()));
                let current = m
                    .get(&count_key)
                    .and_then(|v| {
                        if let Value::Integer(i) = v {
                            Some(*i)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0);
                m.insert(count_key, Value::Integer(current + 1));

                let last_chunk_key = MapKey::Keyword(Keyword("last-chunk".to_string()));
                m.insert(last_chunk_key, chunk.clone());

                let last_metadata_key = MapKey::Keyword(Keyword("last-metadata".to_string()));
                m.insert(last_metadata_key, metadata.clone());

                let messages_key = MapKey::Keyword(Keyword("messages".to_string()));
                match m.entry(messages_key) {
                    Entry::Occupied(mut entry) => {
                        if let Value::Vector(vec) = entry.get_mut() {
                            vec.push(chunk.clone());
                        } else {
                            *entry.get_mut() = Value::Vector(vec![chunk.clone()]);
                        }
                    }
                    Entry::Vacant(entry) => {
                        entry.insert(Value::Vector(vec![chunk.clone()]));
                    }
                }

                let metadata_key = MapKey::Keyword(Keyword("metadata".to_string()));
                match m.entry(metadata_key) {
                    Entry::Occupied(mut entry) => {
                        if let Value::Vector(vec) = entry.get_mut() {
                            vec.push(metadata.clone());
                        } else {
                            *entry.get_mut() = Value::Vector(vec![metadata.clone()]);
                        }
                    }
                    Entry::Vacant(entry) => {
                        entry.insert(Value::Vector(vec![metadata.clone()]));
                    }
                }
            }
            registration.current_state = new_state;

            if let Value::Map(m) = &chunk {
                use rtfs::ast::{Keyword, MapKey};
                let action_key = MapKey::Keyword(Keyword("action".to_string()));
                if let Some(Value::Keyword(Keyword(action_kw))) = m.get(&action_key) {
                    self.apply_action_directive(registration, action_kw);
                }
            }
        }

        registration.stats.processed_chunks += 1;
        registration.stats.last_latency_ms =
            Some(Instant::now().duration_since(start_time).as_millis());
        Ok(())
    }

    fn validate_processor_output(
        &self,
        registration: &mut StreamProcessorRegistration,
        result_val: &Value,
    ) -> RuntimeResult<()> {
        let Some(schema) = registration.result_schema.as_ref() else {
            return Ok(());
        };

        let Value::Map(map) = result_val else {
            let message = format!(
                "Processor '{}' must return a map when schema is declared",
                registration.processor_fn
            );
            registration.status = StreamStatus::Error(message.clone());
            return Err(RuntimeError::Generic(message));
        };

        let output_key = MapKey::Keyword(Keyword("output".to_string()));
        let output_val = map.get(&output_key).ok_or_else(|| {
            let message = format!(
                "Processor '{}' must return :output when schema is declared",
                registration.processor_fn
            );
            registration.status = StreamStatus::Error(message.clone());
            RuntimeError::Generic(message)
        })?;

        let config = TypeCheckingConfig::default();
        let context = VerificationContext::external_data(&format!(
            "stream-processor:{}",
            registration.processor_fn
        ));

        if let Err(err) = self
            .type_validator
            .validate_with_config(output_val, schema, &config, &context)
        {
            let message = format!(
                "Processor '{}' output failed schema validation: {}",
                registration.processor_fn, err
            );
            registration.status = StreamStatus::Error(message.clone());
            return Err(RuntimeError::Generic(message));
        }

        Ok(())
    }

    fn apply_processor_result(
        &self,
        registration: &mut StreamProcessorRegistration,
        result_val: Value,
    ) -> RuntimeResult<()> {
        use rtfs::ast::{Keyword, MapKey};
        match result_val {
            Value::Map(m) => {
                let state_key = MapKey::Keyword(Keyword("state".to_string()));
                let action_key = MapKey::Keyword(Keyword("action".to_string()));
                let output_key = MapKey::Keyword(Keyword("output".to_string()));
                let mut recognized = false;
                if let Some(new_state) = m.get(&state_key) {
                    registration.current_state = new_state.clone();
                    recognized = true;
                }
                if let Some(Value::Keyword(Keyword(action_kw))) = m.get(&action_key) {
                    self.apply_action_directive(registration, action_kw);
                }
                if m.get(&output_key).is_some() {
                    recognized = true;
                }
                if !recognized {
                    registration.current_state = Value::Map(m.clone());
                }
                Ok(())
            }
            other => {
                registration.status = StreamStatus::Error(format!(
                    "Processor '{}' returned invalid shape (expected map)",
                    registration.processor_fn
                ));
                Err(RuntimeError::Generic(format!(
                    "Processor '{}' returned invalid shape (expected map), got: {:?}",
                    registration.processor_fn, other
                )))
            }
        }
    }

    fn apply_action_directive(
        &self,
        registration: &mut StreamProcessorRegistration,
        action_kw: &str,
    ) {
        match action_kw {
            "complete" => registration.status = StreamStatus::Completed,
            "stop" => registration.status = StreamStatus::Stopped,
            "pause" => registration.status = StreamStatus::Paused,
            "resume" => registration.status = StreamStatus::Active,
            "cancel" => registration.status = StreamStatus::Cancelled,
            other => {
                registration.status =
                    StreamStatus::Error(format!("Unknown action directive: {}", other))
            }
        }
    }

    fn extract_action(chunk: &Value) -> Option<String> {
        if let Value::Map(m) = chunk {
            use rtfs::ast::{Keyword, MapKey};
            let action_key = MapKey::Keyword(Keyword("action".to_string()));
            if let Some(Value::Keyword(Keyword(action_kw))) = m.get(&action_key) {
                return Some(action_kw.clone());
            }
        }
        None
    }

    fn handle_directive_chunk(
        &self,
        registration: &mut StreamProcessorRegistration,
        chunk: Value,
        metadata: Value,
        action: String,
    ) -> RuntimeResult<()> {
        match action.as_str() {
            "pause" => {
                registration.status = StreamStatus::Paused;
                self.process_single_chunk(
                    registration,
                    QueuedItem {
                        chunk,
                        metadata,
                        enqueued_at: Instant::now(),
                    },
                )
            }
            "resume" => {
                registration.status = StreamStatus::Active;
                Ok(())
            }
            "cancel" => {
                registration.status = StreamStatus::Cancelled;
                registration.queue.clear();
                registration.stats.queued_chunks = 0;
                Ok(())
            }
            "complete" | "stop" => self.process_single_chunk(
                registration,
                QueuedItem {
                    chunk,
                    metadata,
                    enqueued_at: Instant::now(),
                },
            ),
            other => {
                registration.status =
                    StreamStatus::Error(format!("Unknown action directive: {}", other));
                Err(RuntimeError::Generic(format!(
                    "Unknown action directive: {}",
                    other
                )))
            }
        }
    }
}

#[async_trait::async_trait]
impl StreamChunkSink for McpStreamingProvider {
    async fn ingest_chunk(
        &self,
        stream_id: &str,
        chunk: Value,
        metadata: Value,
    ) -> RuntimeResult<()> {
        self.process_chunk(stream_id, chunk, metadata).await
    }
}

#[derive(Clone)]
pub struct SseStreamTransport {
    http_client: Client,
}

impl SseStreamTransport {
    pub fn new(http_client: Client) -> Self {
        Self { http_client }
    }

    fn max_backoff() -> Duration {
        Duration::from_secs(5)
    }

    fn initial_backoff() -> Duration {
        Duration::from_millis(250)
    }

    async fn wait_for_retry(stop_rx: &mut mpsc::Receiver<()>, duration: Duration) -> bool {
        tokio::select! {
            _ = stop_rx.recv() => true,
            _ = tokio::time::sleep(duration) => false,
        }
    }

    async fn run_loop(
        &self,
        stream_id: String,
        endpoint: String,
        stop_rx: &mut mpsc::Receiver<()>,
        client_config: McpClientConfig,
        sink: Arc<dyn StreamChunkSink>,
    ) -> RuntimeResult<SseLoopOutcome> {
        let url = Self::compose_stream_url(&client_config.server_url, &endpoint);
        let mut request = self
            .http_client
            .get(&url)
            .header("Accept", "text/event-stream");

        if let Some(header) = &client_config.auth_header {
            if let Some((name, value)) = Self::parse_auth_header_parts(header) {
                request = request.header(name, value);
            }
        }

        let mut event_source = EventSource::new(request).map_err(|e| {
            RuntimeError::Generic(format!("Failed to connect to SSE {}: {}", url, e))
        })?;
        let mut fetched_targets: HashSet<String> = HashSet::new();

        loop {
            tokio::select! {
                _ = stop_rx.recv() => {
                    event_source.close();
                    return Ok(SseLoopOutcome::Stopped);
                }
                event = event_source.next() => {
                    match event {
                        Some(Ok(ReqwestEvent::Open)) => {}
                        Some(Ok(ReqwestEvent::Message(message))) => {
                            let (chunk_value, metadata_value) = Self::convert_sse_message(&message, &client_config.server_url);
                            Self::deliver_chunk(&sink, &stream_id, chunk_value, metadata_value).await;

                            if let Some(target) = Self::extract_followup_target(&message) {
                                if fetched_targets.insert(target.clone()) {
                                    match self.fetch_followup_payload(&client_config, &target).await {
                                        Ok(pairs) => {
                                            for (chunk, metadata) in pairs {
                                                Self::deliver_chunk(&sink, &stream_id, chunk, metadata).await;
                                            }
                                        }
                                        Err(err) => {
                                            warn!(
                                                "Failed to fetch follow-up payload {} for stream {}: {}",
                                                target, stream_id, err
                                            );
                                            let resolved = Self::resolve_followup_url(&client_config.server_url, &target)
                                                .unwrap_or_else(|_| target.clone());
                                            let error_chunk = Self::build_followup_error_chunk(&resolved, &err);
                                            let error_metadata = Self::build_followup_metadata(
                                                &client_config.server_url,
                                                &resolved,
                                                "follow-up-error",
                                            );
                                            Self::deliver_chunk(&sink, &stream_id, error_chunk, error_metadata).await;
                                        }
                                    }
                                }
                            }
                        }
                        Some(Err(err)) => {
                            event_source.close();
                            return Err(RuntimeError::Generic(format!(
                                "SSE stream error for {}: {}",
                                stream_id, err
                            )));
                        }
                        None => break,
                    }
                }
            }
        }

        Ok(SseLoopOutcome::Ended)
    }

    async fn deliver_chunk(
        sink: &Arc<dyn StreamChunkSink>,
        stream_id: &str,
        chunk: Value,
        metadata: Value,
    ) {
        if let Err(err) = sink.ingest_chunk(stream_id, chunk, metadata).await {
            error!("Error processing chunk for stream {}: {}", stream_id, err);
        }
    }

    fn compose_stream_url(base_url: &str, endpoint: &str) -> String {
        if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
            endpoint.to_string()
        } else if endpoint.is_empty() {
            base_url.to_string()
        } else {
            let trimmed_base = base_url.trim_end_matches('/');
            let trimmed_endpoint = endpoint.trim_start_matches('/');
            format!("{}/{}", trimmed_base, trimmed_endpoint)
        }
    }

    fn parse_auth_header_parts(header: &str) -> Option<(String, String)> {
        if header.trim().is_empty() {
            return None;
        }
        if let Some((name, value)) = header.split_once(':') {
            Some((name.trim().to_string(), value.trim().to_string()))
        } else {
            Some(("Authorization".to_string(), header.trim().to_string()))
        }
    }

    fn json_to_rtfs_value(json: JsonValue) -> Value {
        match json {
            JsonValue::Null => Value::Nil,
            JsonValue::Bool(b) => Value::Boolean(b),
            JsonValue::Number(num) => {
                if let Some(i) = num.as_i64() {
                    Value::Integer(i)
                } else if let Some(f) = num.as_f64() {
                    Value::Float(f)
                } else {
                    Value::String(num.to_string())
                }
            }
            JsonValue::String(s) => Value::String(s),
            JsonValue::Array(arr) => {
                let mut vec = Vec::with_capacity(arr.len());
                for v in arr {
                    vec.push(Self::json_to_rtfs_value(v));
                }
                Value::Vector(vec)
            }
            JsonValue::Object(obj) => {
                let mut map = HashMap::with_capacity(obj.len());
                for (k, v) in obj {
                    map.insert(rtfs::ast::MapKey::String(k), Self::json_to_rtfs_value(v));
                }
                Value::Map(map)
            }
        }
    }

    fn convert_sse_message(message: &SseMessage, origin: &str) -> (Value, Value) {
        let chunk_value = if message.data.trim().is_empty() {
            Value::Nil
        } else {
            match serde_json::from_str::<JsonValue>(&message.data) {
                Ok(json) => Self::json_to_rtfs_value(json),
                Err(_) => Value::String(message.data.clone()),
            }
        };

        let mut meta_map = HashMap::new();
        use rtfs::ast::{Keyword, MapKey};
        if !message.event.is_empty() {
            meta_map.insert(
                MapKey::Keyword(Keyword("event".into())),
                Value::String(message.event.clone()),
            );
        }
        if !message.id.is_empty() {
            meta_map.insert(
                MapKey::Keyword(Keyword("id".into())),
                Value::String(message.id.clone()),
            );
        }
        meta_map.insert(
            MapKey::Keyword(Keyword("origin".into())),
            Value::String(origin.to_string()),
        );

        (chunk_value, Value::Map(meta_map))
    }

    fn extract_followup_target(message: &SseMessage) -> Option<String> {
        if message.event.trim() != "endpoint" {
            return None;
        }
        let trimmed = message.data.trim();
        if trimmed.is_empty() {
            return None;
        }
        if trimmed.starts_with('/')
            || trimmed.starts_with("http://")
            || trimmed.starts_with("https://")
        {
            Some(trimmed.to_string())
        } else {
            None
        }
    }

    async fn fetch_followup_payload(
        &self,
        client_config: &McpClientConfig,
        target: &str,
    ) -> RuntimeResult<Vec<(Value, Value)>> {
        let resolved_url = Self::resolve_followup_url(&client_config.server_url, target)?;
        let mut request = self.http_client.get(&resolved_url);

        if let Some(header) = &client_config.auth_header {
            if let Some((name, value)) = Self::parse_auth_header_parts(header) {
                request = request.header(name, value);
            }
        }

        let response = request.send().await.map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to fetch follow-up payload {}: {}",
                resolved_url, e
            ))
        })?;

        let status = response.status();
        if !status.is_success() {
            return Err(RuntimeError::Generic(format!(
                "Follow-up payload {} returned HTTP status {}",
                resolved_url, status
            )));
        }

        let text = response.text().await.map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to read follow-up payload {}: {}",
                resolved_url, e
            ))
        })?;

        let chunk_value = if text.trim().is_empty() {
            Value::Nil
        } else if let Ok(json) = serde_json::from_str::<JsonValue>(&text) {
            Self::json_to_rtfs_value(json)
        } else {
            Value::String(text)
        };

        let metadata_value =
            Self::build_followup_metadata(&client_config.server_url, &resolved_url, "follow-up");

        Ok(vec![(chunk_value, metadata_value)])
    }

    fn build_followup_metadata(origin: &str, resolved_url: &str, kind: &str) -> Value {
        use rtfs::ast::{Keyword, MapKey};
        let mut meta_map = HashMap::new();
        meta_map.insert(
            MapKey::Keyword(Keyword("origin".into())),
            Value::String(origin.to_string()),
        );
        meta_map.insert(
            MapKey::Keyword(Keyword("source".into())),
            Value::String(resolved_url.to_string()),
        );
        meta_map.insert(
            MapKey::Keyword(Keyword("kind".into())),
            Value::String(kind.to_string()),
        );

        Value::Map(meta_map)
    }

    fn build_followup_error_chunk(target: &str, error: &RuntimeError) -> Value {
        use rtfs::ast::{Keyword, MapKey};
        let mut map = HashMap::new();
        map.insert(
            MapKey::Keyword(Keyword("target".into())),
            Value::String(target.to_string()),
        );
        map.insert(
            MapKey::Keyword(Keyword("error".into())),
            Value::String(error.to_string()),
        );

        Value::Map(map)
    }

    fn resolve_followup_url(base_url: &str, target: &str) -> RuntimeResult<String> {
        if target.starts_with("http://") || target.starts_with("https://") {
            return Ok(target.to_string());
        }

        let base = Url::parse(base_url)
            .map_err(|e| RuntimeError::Generic(format!("Invalid base URL {}: {}", base_url, e)))?;
        let joined = base.join(target).map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to resolve follow-up target {}: {}",
                target, e
            ))
        })?;

        Ok(joined.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SseLoopOutcome {
    Stopped,
    Ended,
}

#[async_trait::async_trait]
impl StreamTransport for SseStreamTransport {
    async fn run(&self, args: StreamTransportArgs) -> RuntimeResult<()> {
        let StreamTransportArgs {
            stream_id,
            endpoint,
            mut stop_rx,
            sink,
            client_config,
        } = args;

        let max_attempts = client_config.retry_attempts.max(1);
        let mut attempt = 0u32;
        let mut backoff = Self::initial_backoff();

        loop {
            match self
                .run_loop(
                    stream_id.clone(),
                    endpoint.clone(),
                    &mut stop_rx,
                    client_config.clone(),
                    sink.clone(),
                )
                .await
            {
                Ok(SseLoopOutcome::Stopped) => return Ok(()),
                Ok(SseLoopOutcome::Ended) => return Ok(()),
                Err(err) => {
                    attempt += 1;
                    if attempt >= max_attempts {
                        return Err(err);
                    }

                    warn!(
                        "Retrying MCP SSE stream {} after attempt {} failed: {}",
                        stream_id, attempt, err
                    );

                    if Self::wait_for_retry(&mut stop_rx, backoff).await {
                        return Ok(());
                    }

                    backoff = (backoff * 2).min(Self::max_backoff());
                }
            }
        }
    }
}

#[async_trait::async_trait]
impl StreamingCapability for McpStreamingProvider {
    fn start_stream(&self, params: &Value) -> RuntimeResult<StreamHandle> {
        let map = match params {
            Value::Map(m) => m,
            _ => return Err(RuntimeError::Generic("start_stream expects a map".into())),
        };
        use rtfs::ast::{Keyword, MapKey};
        let lookup = |k: &str| -> Option<&Value> {
            let kw = MapKey::Keyword(Keyword(k.to_string()));
            map.get(&kw)
                .or_else(|| map.get(&MapKey::String(k.to_string())))
        };
        let endpoint = lookup("endpoint")
            .and_then(|v| {
                if let Value::String(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                RuntimeError::Generic("Missing required string field 'endpoint'".into())
            })?;
        let processor_fn = lookup("processor")
            .and_then(|v| {
                if let Value::String(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default();
        let initial_state = lookup("initial-state")
            .cloned()
            .unwrap_or(Value::Map(std::collections::HashMap::new()));
        let queue_capacity = lookup("queue-capacity").and_then(|v| {
            if let Value::Integer(i) = v {
                Some(*i as usize)
            } else {
                None
            }
        });
        let stream_id = format!(
            "mcp-{}-{}",
            endpoint.replace('.', "-"),
            uuid::Uuid::new_v4()
        );
        let (stop_tx, stop_rx) = mpsc::channel(1);
        let handle = StreamHandle {
            stream_id: stream_id.clone(),
            stop_tx,
        };
        self.register_processor(stream_id.clone(), processor_fn, vec![], initial_state);
        if let Some(cap) = queue_capacity {
            if let Some(reg) = self.stream_processors.lock().unwrap().get_mut(&stream_id) {
                reg.queue_capacity = cap;
            }
        }
        if self.client_config.auto_connect {
            self.spawn_transport_task(stream_id.clone(), endpoint.clone(), stop_rx);
        }
        info!("Starting MCP stream to endpoint: {}", endpoint);
        Ok(handle)
    }

    fn stop_stream(&self, handle: &StreamHandle) -> RuntimeResult<()> {
        let mut processors = self.stream_processors.lock().unwrap();
        let removed = processors.remove(&handle.stream_id);
        drop(processors);
        if removed.is_some() {
            self.remove_persisted(&handle.stream_id);
        }

        if let Ok(mut schemas) = self.stream_schemas.lock() {
            schemas.remove(&handle.stream_id);
        }

        let _ = handle.stop_tx.try_send(());
        if let Ok(mut guard) = self.stream_tasks.lock() {
            if let Some(task) = guard.remove(&handle.stream_id) {
                task.abort();
            }
        }
        info!("Stopping MCP stream: {}", handle.stream_id);
        Ok(())
    }

    async fn start_stream_with_config(
        &self,
        params: &Value,
        _config: &StreamConfig,
    ) -> RuntimeResult<StreamHandle> {
        // For MCP streams, we primarily use the basic start_stream
        // Config could be used for additional MCP-specific settings
        self.start_stream(params)
    }

    async fn send_to_stream(&self, _handle: &StreamHandle, _data: &Value) -> RuntimeResult<()> {
        // MCP streaming is typically receive-only from server to client
        Err(RuntimeError::Generic(
            "MCP streams are receive-only".to_string(),
        ))
    }

    fn start_bidirectional_stream(&self, _params: &Value) -> RuntimeResult<StreamHandle> {
        // MCP doesn't typically support bidirectional streaming in this context
        Err(RuntimeError::Generic(
            "Bidirectional MCP streaming not supported".to_string(),
        ))
    }

    async fn start_bidirectional_stream_with_config(
        &self,
        _params: &Value,
        _config: &StreamConfig,
    ) -> RuntimeResult<StreamHandle> {
        Err(RuntimeError::Generic(
            "Bidirectional MCP streaming not supported".to_string(),
        ))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
