// streaming.rs
// All stream-related types, traits, and aliases for CCOS/RTFS

use std::sync::Arc;
use tokio::sync::mpsc;

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
