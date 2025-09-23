// streaming.rs
// All stream-related types, traits, and aliases for CCOS/RTFS

use std::sync::{Arc, Mutex};
use std::collections::HashMap;
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

/// MCP-specific streaming provider for Model Context Protocol endpoints
pub struct McpStreamingProvider {
    /// Base MCP client configuration
    pub client_config: McpClientConfig,
    /// Stream processor registry for continuation management
    pub stream_processors: Arc<Mutex<HashMap<String, StreamProcessorRegistration>>>,
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
#[derive(Clone, Debug, PartialEq, Eq)]
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
        }
    }

    /// Register a stream processor for continuation-based processing
    pub fn register_processor(&self, stream_id: String, processor_fn: String, continuation: Vec<u8>, initial_state: Value) {
        let mut processors = self.stream_processors.lock().unwrap();
        processors.insert(stream_id, StreamProcessorRegistration {
            processor_fn,
            continuation,
            current_state: initial_state.clone(),
            initial_state,
            status: StreamStatus::Active,
        });
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

            // Placeholder processor behavior: increment :count integer per chunk
            let mut new_state = registration.current_state.clone();
            if let Value::Map(m) = &mut new_state {
                use crate::ast::{MapKey, Keyword};
                let key = MapKey::Keyword(Keyword("count".to_string()));
                let current = m.get(&key).and_then(|v| if let Value::Integer(i)=v {Some(*i)} else {None}).unwrap_or(0);
                m.insert(key, Value::Integer(current + 1));
            }
            registration.current_state = new_state;

            // Directive parsing: look for :action keyword in chunk map
            if let Value::Map(m) = &chunk {
                use crate::ast::{MapKey, Keyword};
                let action_key = MapKey::Keyword(Keyword("action".to_string()));
                if let Some(Value::Keyword(Keyword(action_kw))) = m.get(&action_key) {
                    match action_kw.as_str() {
                        "complete" => {
                            registration.status = StreamStatus::Completed;
                        }
                        "stop" => {
                            registration.status = StreamStatus::Stopped;
                        }
                        other => {
                            registration.status = StreamStatus::Error(format!("Unknown action directive: {}", other));
                        }
                    }
                }
            }

            println!("Processing chunk for stream {}: {:?} (state/status: {:?})", stream_id, chunk, registration.status);
            let _ = metadata; // currently unused
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
        self.register_processor(stream_id, processor_fn, vec![], initial_state);
        println!("Starting MCP stream to endpoint: {}", endpoint);
        Ok(handle)
    }

    fn stop_stream(&self, handle: &StreamHandle) -> RuntimeResult<()> {
        let mut processors = self.stream_processors.lock().unwrap();
        processors.remove(&handle.stream_id);

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
