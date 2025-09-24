// streaming.rs
// All stream-related types, traits, and aliases for CCOS/RTFS

use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use tokio::sync::mpsc;
use crate::runtime::{values::Value, error::{RuntimeError, RuntimeResult}};

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
    fn start_stream(&self, params: &crate::runtime::values::Value) -> crate::runtime::error::RuntimeResult<StreamHandle>;
    /// Stop a stream
    fn stop_stream(&self, handle: &StreamHandle) -> crate::runtime::error::RuntimeResult<()>;
    /// Start a stream with extended configuration
    async fn start_stream_with_config(&self, params: &crate::runtime::values::Value, config: &StreamConfig) -> crate::runtime::error::RuntimeResult<StreamHandle>;
    /// Send data to a stream
    async fn send_to_stream(&self, handle: &StreamHandle, data: &crate::runtime::values::Value) -> crate::runtime::error::RuntimeResult<()>;
    /// Start a bidirectional stream
    fn start_bidirectional_stream(&self, params: &crate::runtime::values::Value) -> crate::runtime::error::RuntimeResult<StreamHandle>;
    /// Start a bidirectional stream with extended configuration
    async fn start_bidirectional_stream_with_config(&self, params: &crate::runtime::values::Value, config: &StreamConfig) -> crate::runtime::error::RuntimeResult<StreamHandle>;
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
        let mut guard = self.inner.lock().map_err(|_| "stream persistence poisoned".to_string())?;
        guard.insert(snapshot.stream_id.clone(), snapshot.clone());
        Ok(())
    }

    fn load_snapshot(&self, stream_id: &str) -> Result<Option<StreamSnapshot>, String> {
        let guard = self.inner.lock().map_err(|_| "stream persistence poisoned".to_string())?;
        Ok(guard.get(stream_id).cloned())
    }

    fn remove_snapshot(&self, stream_id: &str) -> Result<(), String> {
        let mut guard = self.inner.lock().map_err(|_| "stream persistence poisoned".to_string())?;
        guard.remove(stream_id);
        Ok(())
    }
}

/// MCP-specific streaming provider for Model Context Protocol endpoints
pub struct McpStreamingProvider {
    /// Base MCP client configuration
    pub client_config: McpClientConfig,
    /// Stream processor registry for continuation management
    pub stream_processors: Arc<Mutex<HashMap<String, StreamProcessorRegistration>>>,
    /// Optional processor invoker hook (Phase 3) allowing real RTFS function invocation
    /// Signature: (processor_fn, state, chunk, metadata) -> result map
    processor_invoker: Option<Arc<dyn Fn(&str, &Value, &Value, &Value) -> RuntimeResult<Value> + Send + Sync>>,
    /// Optional persistence backend (Phase 4) for continuation snapshots
    persistence: Option<Arc<dyn StreamPersistence>>, 
}

#[derive(Debug, Clone)]
pub struct McpClientConfig {
    pub server_url: String,
    pub timeout_ms: u64,
    pub retry_attempts: u32,
}

#[derive(Clone)]
pub struct StreamProcessorRegistration {
    pub processor_fn: String,      // Function name to call (placeholder until real invocation)
    pub continuation: Vec<u8>,     // Serialized continuation data (future)
    pub initial_state: Value,      // Original starting state
    pub current_state: Value,      // Mutable logical state updated per chunk (Phase 1 prototype)
    pub status: StreamStatus,      // Directive/status lifecycle tracking (Phase 2 prototype)
}

/// Lifecycle status for a stream processor registration.
/// This will expand as richer directives are supported (e.g., backpressure, inject, error details).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamStatus {
    Active,
    Completed,
    Stopped,
    Error(String),
}

impl McpStreamingProvider {
    pub fn new(server_url: String) -> Self {
        Self {
            client_config: McpClientConfig {
                server_url,
                timeout_ms: 30000,
                retry_attempts: 3,
            },
            stream_processors: Arc::new(Mutex::new(HashMap::new())),
            processor_invoker: None,
            persistence: None,
        }
    }

    /// Construct with a custom processor invoker (used by Phase 3 tests to supply evaluator)
    pub fn new_with_invoker(server_url: String, invoker: Arc<dyn Fn(&str, &Value, &Value, &Value) -> RuntimeResult<Value> + Send + Sync>) -> Self {
        let mut s = Self::new(server_url);
        s.processor_invoker = Some(invoker);
        s
    }

    /// Construct with explicit persistence backend and optional processor invoker.
    pub fn new_with_persistence(server_url: String, persistence: Arc<dyn StreamPersistence>, invoker: Option<Arc<dyn Fn(&str, &Value, &Value, &Value) -> RuntimeResult<Value> + Send + Sync>>) -> Self {
        let mut s = Self::new(server_url);
        s.persistence = Some(persistence);
        s.processor_invoker = invoker;
        s
    }

    /// Register a stream processor for continuation-based processing
    pub fn register_processor(&self, stream_id: String, processor_fn: String, continuation: Vec<u8>, initial_state: Value) {
        let mut processors = self.stream_processors.lock().unwrap();
        processors.insert(stream_id.clone(), StreamProcessorRegistration {
            processor_fn,
            continuation,
            current_state: initial_state.clone(),
            initial_state,
            status: StreamStatus::Active,
        });
        if let Some(registration) = processors.get(&stream_id) {
            self.persist_registration(&stream_id, registration);
        }
    }

    fn snapshot_for(&self, stream_id: &str, registration: &StreamProcessorRegistration) -> StreamSnapshot {
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
                eprintln!("Failed to persist snapshot for stream {}: {}", stream_id, err);
            }
        }
    }

    fn remove_persisted(&self, stream_id: &str) {
        if let Some(persistence) = &self.persistence {
            if let Err(err) = persistence.remove_snapshot(stream_id) {
                eprintln!("Failed to remove persisted snapshot for stream {}: {}", stream_id, err);
            }
        }
    }

    /// Process a stream chunk by resuming RTFS execution
    pub async fn process_chunk(&self, stream_id: &str, chunk: Value, metadata: Value) -> RuntimeResult<()> {
        let mut processors = self.stream_processors.lock().unwrap();
        if let Some(registration) = processors.get_mut(stream_id) {
            // If stream already terminal, ignore further chunks (idempotent no-op)
            match &registration.status {
                StreamStatus::Completed | StreamStatus::Stopped | StreamStatus::Error(_) => {
                    return Ok(()); // Silently ignore for now; could log.
                }
                StreamStatus::Active => {}
            }
            // Phase 3: Real processor invocation if invoker + processor_fn set; else fallback
            let mut invoked = false;
            if let Some(invoker) = &self.processor_invoker {
                if !registration.processor_fn.is_empty() {
                    match invoker(&registration.processor_fn, &registration.current_state, &chunk, &metadata) {
                        Ok(result_val) => {
                            invoked = true;
                            // Interpret return shape
                            use crate::ast::{MapKey, Keyword};
                            match result_val.clone() {
                                Value::Map(m) => {
                                    // Recognized keys
                                    let state_key = MapKey::Keyword(Keyword("state".to_string()));
                                    let action_key = MapKey::Keyword(Keyword("action".to_string()));
                                    let output_key = MapKey::Keyword(Keyword("output".to_string()));
                                    let mut recognized = false;
                                    if let Some(new_state) = m.get(&state_key) {
                                        registration.current_state = new_state.clone();
                                        recognized = true;
                                    }
                                    if let Some(Value::Keyword(Keyword(action_kw))) = m.get(&action_key) {
                                        match action_kw.as_str() {
                                            "complete" => registration.status = StreamStatus::Completed,
                                            "stop" => registration.status = StreamStatus::Stopped,
                                            other => registration.status = StreamStatus::Error(format!("Unknown action directive: {}", other)),
                                        }
                                    }
                                    if m.get(&output_key).is_some() {
                                        // Future: emit event/log. For now, ignore but mark recognized.
                                        recognized = true;
                                    }
                                    if !recognized {
                                        // Backward compat: treat entire map as new state
                                        registration.current_state = Value::Map(m.clone());
                                    }
                                }
                                other => {
                                    // Mark status error then return error
                                    registration.status = StreamStatus::Error(format!(
                                        "Processor '{}' returned invalid shape (expected map)",
                                        registration.processor_fn
                                    ));
                                    return Err(RuntimeError::Generic(format!(
                                        "Processor '{}' returned invalid shape (expected map), got: {:?}",
                                        registration.processor_fn, other
                                    )));
                                }
                            }
                        }
                        Err(e) => {
                            registration.status = StreamStatus::Error(format!("Processor invocation error: {}", e));
                            return Err(e);
                        }
                    }
                }
            }

            if !invoked {
                // Fallback placeholder behavior (Phase 1/2) to maintain backward compatibility
                let mut new_state = registration.current_state.clone();
                if let Value::Map(m) = &mut new_state {
                    use crate::ast::{MapKey, Keyword};
                    let key = MapKey::Keyword(Keyword("count".to_string()));
                    let current = m.get(&key).and_then(|v| if let Value::Integer(i)=v {Some(*i)} else {None}).unwrap_or(0);
                    m.insert(key, Value::Integer(current + 1));
                }
                registration.current_state = new_state;

                // Directive parsing from chunk (legacy path)
                if let Value::Map(m) = &chunk {
                    use crate::ast::{MapKey, Keyword};
                    let action_key = MapKey::Keyword(Keyword("action".to_string()));
                    if let Some(Value::Keyword(Keyword(action_kw))) = m.get(&action_key) {
                        match action_kw.as_str() {
                            "complete" => registration.status = StreamStatus::Completed,
                            "stop" => registration.status = StreamStatus::Stopped,
                            other => registration.status = StreamStatus::Error(format!("Unknown action directive: {}", other)),
                        }
                    }
                }
            }

            self.persist_registration(stream_id, registration);
            println!("Processing chunk for stream {}: {:?} (state/status: {:?})", stream_id, chunk, registration.status);
            Ok(())
        } else {
            Err(RuntimeError::Generic(format!("No processor registered for stream: {}", stream_id)))
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
        let persistence = self.persistence.clone().ok_or_else(|| RuntimeError::Generic("Stream persistence backend not configured".into()))?;
        let snapshot = persistence.load_snapshot(stream_id)
            .map_err(|e| RuntimeError::Generic(format!("Failed to load snapshot: {}", e)))?
            .ok_or_else(|| RuntimeError::Generic(format!("No persisted snapshot for stream: {}", stream_id)))?;

        let mut processors = self.stream_processors.lock().unwrap();
        processors.insert(stream_id.to_string(), StreamProcessorRegistration {
            processor_fn: snapshot.processor_fn,
            continuation: snapshot.continuation,
            initial_state: snapshot.current_state.clone(),
            current_state: snapshot.current_state,
            status: snapshot.status,
        });
        if let Some(registration) = processors.get(stream_id) {
            self.persist_registration(stream_id, registration);
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl StreamingCapability for McpStreamingProvider {
    fn start_stream(&self, params: &Value) -> RuntimeResult<StreamHandle> {
        let map = match params { Value::Map(m) => m, _ => return Err(RuntimeError::Generic("start_stream expects a map".into())) };
        use crate::ast::{MapKey, Keyword};
        let lookup = |k: &str| -> Option<&Value> {
            let kw = MapKey::Keyword(Keyword(k.to_string()));
            map.get(&kw).or_else(|| map.get(&MapKey::String(k.to_string())))
        };
        let endpoint = lookup("endpoint").and_then(|v| if let Value::String(s)=v {Some(s.clone())} else {None})
            .ok_or_else(|| RuntimeError::Generic("Missing required string field 'endpoint'".into()))?;
        let processor_fn = lookup("processor").and_then(|v| if let Value::String(s)=v {Some(s.clone())} else {None}).unwrap_or_default();
        let initial_state = lookup("initial-state").cloned().unwrap_or(Value::Map(std::collections::HashMap::new()));
        let stream_id = format!("mcp-{}-{}", endpoint.replace('.', "-"), uuid::Uuid::new_v4());
        let (stop_tx, _stop_rx) = mpsc::channel(1);
        let handle = StreamHandle { stream_id: stream_id.clone(), stop_tx };
        self.register_processor(stream_id.clone(), processor_fn, vec![], initial_state);
        println!("Starting MCP stream to endpoint: {}", endpoint);
        Ok(handle)
    }

    fn stop_stream(&self, handle: &StreamHandle) -> RuntimeResult<()> {
        let mut processors = self.stream_processors.lock().unwrap();
        let removed = processors.remove(&handle.stream_id);
        drop(processors);
        if removed.is_some() {
            self.remove_persisted(&handle.stream_id);
        }

        // TODO: Close MCP connection
        println!("Stopping MCP stream: {}", handle.stream_id);
        Ok(())
    }

    async fn start_stream_with_config(&self, params: &Value, config: &StreamConfig) -> RuntimeResult<StreamHandle> {
        // For MCP streams, we primarily use the basic start_stream
        // Config could be used for additional MCP-specific settings
        self.start_stream(params)
    }

    async fn send_to_stream(&self, _handle: &StreamHandle, _data: &Value) -> RuntimeResult<()> {
        // MCP streaming is typically receive-only from server to client
        Err(RuntimeError::Generic("MCP streams are receive-only".to_string()))
    }

    fn start_bidirectional_stream(&self, _params: &Value) -> RuntimeResult<StreamHandle> {
        // MCP doesn't typically support bidirectional streaming in this context
        Err(RuntimeError::Generic("Bidirectional MCP streaming not supported".to_string()))
    }

    async fn start_bidirectional_stream_with_config(&self, _params: &Value, _config: &StreamConfig) -> RuntimeResult<StreamHandle> {
        Err(RuntimeError::Generic("Bidirectional MCP streaming not supported".to_string()))
    }
}
