use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::runtime::values::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::mpsc;

/// Progress token for tracking long-running operations (MCP-style)
pub type ProgressToken = String;

/// Cursor for pagination (MCP-style)
pub type Cursor = String;

/// Progress notification for long-running operations
#[derive(Debug, Clone)]
pub struct ProgressNotification {
    pub progress_token: ProgressToken,
    pub progress: u64,
    pub total: Option<u64>,
    pub message: Option<String>,
}

/// Types of streaming capabilities
#[derive(Debug, Clone)]
pub enum StreamType {
    /// Produces data to consumers (unidirectional)
    Source,
    /// Consumes data from producers (unidirectional)
    Sink,
    /// Transforms data (unidirectional: input -> transform -> output)
    Transform,
    /// Bidirectional stream (can both send and receive simultaneously)
    Bidirectional,
    /// Duplex stream (separate input and output channels)
    Duplex,
}

/// Bidirectional stream configuration
#[derive(Debug, Clone)]
pub struct BidirectionalConfig {
    /// Buffer size for incoming data
    pub input_buffer_size: usize,
    /// Buffer size for outgoing data
    pub output_buffer_size: usize,
    /// Whether to enable flow control
    pub flow_control: bool,
    /// Timeout for bidirectional operations
    pub timeout_ms: u64,
}

/// Duplex stream channels
#[derive(Debug, Clone)]
pub struct DuplexChannels {
    /// Input channel configuration
    pub input_channel: StreamChannelConfig,
    /// Output channel configuration  
    pub output_channel: StreamChannelConfig,
}

/// Stream channel configuration
#[derive(Debug, Clone)]
pub struct StreamChannelConfig {
    /// Channel buffer size
    pub buffer_size: usize,
    /// Channel-specific metadata
    pub metadata: HashMap<String, String>,
}

/// Enhanced stream item for bidirectional operations
#[derive(Debug, Clone)]
pub struct StreamItem {
    pub data: Value,
    pub sequence: u64,
    pub timestamp: u64,
    pub metadata: HashMap<String, String>,
    /// Direction indicator for bidirectional streams
    pub direction: StreamDirection,
    /// Correlation ID for request/response patterns
    pub correlation_id: Option<String>,
}

/// Stream direction for bidirectional operations
#[derive(Debug, Clone)]
pub enum StreamDirection {
    /// Data flowing into the stream
    Inbound,
    /// Data flowing out of the stream
    Outbound,
    /// Bidirectional data (both directions)
    Bidirectional,
}

/// Streaming capability metadata
#[derive(Debug, Clone)]
pub struct StreamingCapabilityMetadata {
    pub id: String,
    pub name: String,
    pub description: String,
    pub stream_type: StreamType,
    pub input_schema: Option<String>, // JSON schema for input validation
    pub output_schema: Option<String>, // JSON schema for output validation
    pub supports_progress: bool,
    pub supports_cancellation: bool,
    /// Configuration for bidirectional streams
    pub bidirectional_config: Option<BidirectionalConfig>,
    /// Configuration for duplex streams
    pub duplex_config: Option<DuplexChannels>,
    /// Stream configuration with optional callbacks
    pub stream_config: Option<StreamConfig>,
}

/// Represents a capability implementation
#[derive(Debug, Clone)]
pub struct CapabilityImpl {
    pub id: String,
    pub name: String,
    pub description: String,
    pub provider: CapabilityProvider,
    pub local: bool,
    pub endpoint: Option<String>,
}

/// Different types of capability providers
#[derive(Debug, Clone)]
pub enum CapabilityProvider {
    /// Local implementation (built-in)
    Local(LocalCapability),
    /// Remote HTTP API
    Http(HttpCapability),
    /// MCP (Model Context Protocol) server
    MCP(MCPCapability),
    /// A2A (Agent-to-Agent) communication
    A2A(A2ACapability),
    /// Plugin-based capability
    Plugin(PluginCapability),
    /// Remote RTFS instance capability
    RemoteRTFS(RemoteRTFSCapability),
    /// Streaming capability
    Stream(StreamCapabilityImpl),
}

/// Remote RTFS execution capability
#[derive(Debug, Clone)]
pub struct RemoteRTFSCapability {
    pub endpoint: String,
    pub timeout_ms: u64,
    pub auth_token: Option<String>,
}

/// Local capability implementation
#[derive(Clone)]
pub struct LocalCapability {
    pub handler: Arc<dyn Fn(&Value) -> RuntimeResult<Value> + Send + Sync>,
}

impl std::fmt::Debug for LocalCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalCapability")
            .field("handler", &"<function>")
            .finish()
    }
}

/// HTTP-based remote capability
#[derive(Debug, Clone)]
pub struct HttpCapability {
    pub base_url: String,
    pub auth_token: Option<String>,
    pub timeout_ms: u64,
}

/// MCP server capability
#[derive(Debug, Clone)]
pub struct MCPCapability {
    pub server_url: String,
    pub tool_name: String,
}

/// A2A communication capability
#[derive(Debug, Clone)]
pub struct A2ACapability {
    pub agent_id: String,
    pub endpoint: String,
}

/// Plugin-based capability
#[derive(Debug, Clone)]
pub struct PluginCapability {
    pub plugin_path: String,
    pub function_name: String,
}

/// Streaming providers for different transport mechanisms
#[derive(Debug, Clone)]
pub enum StreamingProvider {
    /// Local in-memory streams using channels
    Local {
        buffer_size: usize,
    },
    /// WebSocket-based streaming
    WebSocket {
        url: String,
        protocols: Vec<String>,
    },
    /// Server-Sent Events (SSE) streaming
    ServerSentEvents {
        url: String,
        headers: HashMap<String, String>,
    },
    /// HTTP chunked transfer streaming
    Http {
        url: String,
        method: String,
        headers: HashMap<String, String>,
    },
}

/// Unified stream capability trait - the core abstraction
#[async_trait::async_trait]
pub trait StreamCapability: Send + Sync {
    /// Start streaming - returns a receiver for stream items (channel-based, default)
    async fn start_stream(&self, params: &Value) -> RuntimeResult<mpsc::Receiver<StreamItem>>;
    
    /// Start streaming with enhanced configuration and optional callbacks
    async fn start_stream_with_config(&self, params: &Value, config: &StreamConfig) -> RuntimeResult<StreamHandle>;
    
    /// Send item to stream (for sinks, transforms, and bidirectional streams)
    async fn send_item(&self, item: &StreamItem) -> RuntimeResult<()>;
    
    /// Start bidirectional stream - returns both sender and receiver (channel-based, default)
    async fn start_bidirectional_stream(&self, params: &Value) -> RuntimeResult<(mpsc::Sender<StreamItem>, mpsc::Receiver<StreamItem>)>;
    
    /// Start bidirectional stream with enhanced configuration and optional callbacks
    async fn start_bidirectional_stream_with_config(&self, params: &Value, config: &StreamConfig) -> RuntimeResult<StreamHandle>;
    
    /// Start duplex stream - returns separate channels for input and output
    async fn start_duplex_stream(&self, params: &Value) -> RuntimeResult<DuplexStreamChannels>;
    
    /// Get current stream progress (optional)
    async fn get_progress(&self, token: &ProgressToken) -> RuntimeResult<Option<ProgressNotification>>;
    
    /// Cancel stream operation (optional)
    async fn cancel(&self, token: &ProgressToken) -> RuntimeResult<()>;
}

/// Duplex stream channels result
#[derive(Debug)]
pub struct DuplexStreamChannels {
    /// Input channel (for sending data to the stream)
    pub input_sender: mpsc::Sender<StreamItem>,
    /// Output channel (for receiving data from the stream)
    pub output_receiver: mpsc::Receiver<StreamItem>,
    /// Optional feedback channel (for flow control or acknowledgments)
    pub feedback_receiver: Option<mpsc::Receiver<StreamItem>>,
}

/// Stream capability implementation
#[derive(Debug, Clone)]
pub struct StreamCapabilityImpl {
    pub metadata: StreamingCapabilityMetadata,
    pub provider: StreamingProvider,
}

impl StreamCapabilityImpl {
    pub fn new(metadata: StreamingCapabilityMetadata, provider: StreamingProvider) -> Self {
        Self { metadata, provider }
    }
}

/// The capability marketplace that manages all available capabilities
pub struct CapabilityMarketplace {
    capabilities: Arc<RwLock<HashMap<String, CapabilityImpl>>>,
    discovery_agents: Vec<Box<dyn CapabilityDiscovery>>,
}

impl std::fmt::Debug for CapabilityMarketplace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CapabilityMarketplace")
            .field("capabilities", &"<HashMap<String, CapabilityImpl>>")
            .field("discovery_agents", &format!("{} discovery agents", self.discovery_agents.len()))
            .finish()
    }
}

impl CapabilityMarketplace {
    /// Create a new capability marketplace
    pub fn new() -> Self {
        Self {
            capabilities: Arc::new(RwLock::new(HashMap::new())),
            discovery_agents: Vec::new(),
        }
    }

    /// Register a local capability
    pub async fn register_local_capability(
        &self,
        id: String,
        name: String,
        description: String,
        handler: Arc<dyn Fn(&Value) -> RuntimeResult<Value> + Send + Sync>,
    ) -> Result<(), RuntimeError> {
        let capability = CapabilityImpl {
            id: id.clone(),
            name,
            description,
            provider: CapabilityProvider::Local(LocalCapability { handler }),
            local: true,
            endpoint: None,
        };

        let mut capabilities = self.capabilities.write().await;
        capabilities.insert(id, capability);
        Ok(())
    }

    /// Register a remote HTTP capability
    pub async fn register_http_capability(
        &self,
        id: String,
        name: String,
        description: String,
        base_url: String,
        auth_token: Option<String>,
    ) -> Result<(), RuntimeError> {
        let capability = CapabilityImpl {
            id: id.clone(),
            name,
            description,
            provider: CapabilityProvider::Http(HttpCapability {
                base_url,
                auth_token,
                timeout_ms: 5000,
            }),
            local: false,
            endpoint: None,
        };

        let mut capabilities = self.capabilities.write().await;
        capabilities.insert(id, capability);
        Ok(())
    }

    /// Register a remote RTFS capability
    pub async fn register_remote_rtfs_capability(
        &self,
        id: String,
        name: String,
        description: String,
        endpoint: String,
        auth_token: Option<String>,
        timeout_ms: u64,
    ) -> Result<(), RuntimeError> {
        let endpoint_clone = endpoint.clone();
        let capability = CapabilityImpl {
            id: id.clone(),
            name,
            description,
            provider: CapabilityProvider::RemoteRTFS(RemoteRTFSCapability {
                endpoint,
                timeout_ms,
                auth_token,
            }),
            local: false,
            endpoint: Some(endpoint_clone),
        };

        let mut capabilities = self.capabilities.write().await;
        capabilities.insert(id, capability);
        Ok(())
    }

    /// Register a streaming capability with simplified interface
    pub async fn register_streaming_capability(
        &self,
        id: String,
        name: String,
        description: String,
        stream_type: StreamType,
        provider: StreamingProvider,
    ) -> Result<(), RuntimeError> {
        let metadata = StreamingCapabilityMetadata {
            id: id.clone(),
            name,
            description,
            stream_type,
            input_schema: None,
            output_schema: None,
            supports_progress: true,
            supports_cancellation: true,
            bidirectional_config: None,
            duplex_config: None,
            stream_config: None,
        };
        
        // Create unified capability that handles streaming
        let capability = CapabilityImpl {
            id: id.clone(),
            name: metadata.name.clone(),
            description: metadata.description.clone(),
            provider: CapabilityProvider::Stream(StreamCapabilityImpl::new(metadata, provider)),
            local: false,
            endpoint: None,
        };
        
        let mut capabilities = self.capabilities.write().await;
        capabilities.insert(id, capability);
        Ok(())
    }

    /// Register a bidirectional streaming capability
    pub async fn register_bidirectional_stream_capability(
        &self,
        id: String,
        name: String,
        description: String,
        provider: StreamingProvider,
        config: BidirectionalConfig,
    ) -> Result<(), RuntimeError> {
        let metadata = StreamingCapabilityMetadata {
            id: id.clone(),
            name,
            description,
            stream_type: StreamType::Bidirectional,
            input_schema: None,
            output_schema: None,
            supports_progress: true,
            supports_cancellation: true,
            bidirectional_config: Some(config),
            duplex_config: None,
            stream_config: None,
        };
        
        let capability = CapabilityImpl {
            id: id.clone(),
            name: metadata.name.clone(),
            description: metadata.description.clone(),
            provider: CapabilityProvider::Stream(StreamCapabilityImpl::new(metadata, provider)),
            local: false,
            endpoint: None,
        };
        
        let mut capabilities = self.capabilities.write().await;
        capabilities.insert(id, capability);
        Ok(())
    }

    /// Register a duplex streaming capability
    pub async fn register_duplex_stream_capability(
        &self,
        id: String,
        name: String,
        description: String,
        provider: StreamingProvider,
        duplex_config: DuplexChannels,
    ) -> Result<(), RuntimeError> {
        let metadata = StreamingCapabilityMetadata {
            id: id.clone(),
            name,
            description,
            stream_type: StreamType::Duplex,
            input_schema: None,
            output_schema: None,
            supports_progress: true,
            supports_cancellation: true,
            bidirectional_config: None,
            duplex_config: Some(duplex_config),
            stream_config: None,
        };
        
        let capability = CapabilityImpl {
            id: id.clone(),
            name: metadata.name.clone(),
            description: metadata.description.clone(),
            provider: CapabilityProvider::Stream(StreamCapabilityImpl::new(metadata, provider)),
            local: false,
            endpoint: None,
        };
        
        let mut capabilities = self.capabilities.write().await;
        capabilities.insert(id, capability);
        Ok(())
    }

    /// Start a bidirectional stream - convenience method
    pub async fn start_bidirectional_stream(
        &self,
        capability_id: &str,
        _params: &Value,
    ) -> RuntimeResult<(mpsc::Sender<StreamItem>, mpsc::Receiver<StreamItem>)> {
        let capability = self.get_capability(capability_id).await
            .ok_or_else(|| RuntimeError::Generic(format!("Capability '{}' not found", capability_id)))?;
        
        if let CapabilityProvider::Stream(stream_impl) = &capability.provider {
            if !matches!(stream_impl.metadata.stream_type, StreamType::Bidirectional) {
                return Err(RuntimeError::Generic(format!("Capability '{}' is not bidirectional", capability_id)));
            }
            
            // Create bidirectional channels
            let (input_tx, _input_rx) = mpsc::channel::<StreamItem>(100);
            let (_output_tx, output_rx) = mpsc::channel::<StreamItem>(100);
            
            // TODO: In a real implementation, this would start the actual stream processing
            // For now, we return the channels
            Ok((input_tx, output_rx))
        } else {
            Err(RuntimeError::Generic(format!("Capability '{}' is not a stream capability", capability_id)))
        }
    }

    /// Start a duplex stream - convenience method
    pub async fn start_duplex_stream(
        &self,
        capability_id: &str,
        _params: &Value,
    ) -> RuntimeResult<DuplexStreamChannels> {
        let capability = self.get_capability(capability_id).await
            .ok_or_else(|| RuntimeError::Generic(format!("Capability '{}' not found", capability_id)))?;
        
        if let CapabilityProvider::Stream(stream_impl) = &capability.provider {
            if !matches!(stream_impl.metadata.stream_type, StreamType::Duplex) {
                return Err(RuntimeError::Generic(format!("Capability '{}' is not duplex", capability_id)));
            }
            
            // Create duplex channels
            let (input_tx, _input_rx) = mpsc::channel::<StreamItem>(100);
            let (_output_tx, output_rx) = mpsc::channel::<StreamItem>(100);
            let (_feedback_tx, feedback_rx) = mpsc::channel::<StreamItem>(50);
            
            // TODO: In a real implementation, this would start the actual stream processing
            Ok(DuplexStreamChannels {
                input_sender: input_tx,
                output_receiver: output_rx,
                feedback_receiver: Some(feedback_rx),
            })
        } else {
            Err(RuntimeError::Generic(format!("Capability '{}' is not a stream capability", capability_id)))
        }
    }

    /// Get a capability by ID
    pub async fn get_capability(&self, id: &str) -> Option<CapabilityImpl> {
        let capabilities = self.capabilities.read().await;
        capabilities.get(id).cloned()
    }

    /// List all available capabilities
    pub async fn list_capabilities(&self) -> Vec<CapabilityImpl> {
        let capabilities = self.capabilities.read().await;
        capabilities.values().cloned().collect()
    }

    /// Execute a capability
    pub async fn execute_capability(&self, id: &str, inputs: &Value) -> RuntimeResult<Value> {
        let capability = self.get_capability(id).await
            .ok_or_else(|| RuntimeError::Generic(format!("Capability '{}' not found", id)))?;

        match &capability.provider {
            CapabilityProvider::Local(local) => {
                // Execute local capability synchronously
                return (local.handler)(inputs);
            }
            CapabilityProvider::Http(http) => {
                // Execute HTTP capability asynchronously
                return self.execute_http_capability(http, inputs).await;
            }
            CapabilityProvider::MCP(mcp) => {
                // Execute MCP capability asynchronously
                return self.execute_mcp_capability(mcp, inputs).await;
            }
            CapabilityProvider::A2A(a2a) => {
                // Execute A2A capability asynchronously
                return self.execute_a2a_capability(a2a, inputs).await;
            }
            CapabilityProvider::Plugin(plugin) => {
                // Execute plugin capability
                return self.execute_plugin_capability(plugin, inputs).await;
            }
            CapabilityProvider::RemoteRTFS(remote) => {
                // Execute remote RTFS capability asynchronously
                return execute_remote_rtfs_capability(remote, inputs).await;
            }
            CapabilityProvider::Stream(stream) => {
                // Execute streaming capability
                return self.execute_stream_capability(stream, inputs).await;
            }
        }
    }

    /// Execute HTTP capability
    async fn execute_http_capability(&self, http: &HttpCapability, inputs: &Value) -> RuntimeResult<Value> {
        // Convert RTFS Value to JSON
        let json_inputs = serde_json::to_value(inputs)
            .map_err(|e| RuntimeError::Generic(format!("Failed to serialize inputs: {}", e)))?;

        // Make HTTP request
        let client = reqwest::Client::new();
        let response = client
            .post(&http.base_url)
            .header("Content-Type", "application/json")
            .json(&json_inputs)
            .timeout(std::time::Duration::from_millis(http.timeout_ms))
            .send()
            .await
            .map_err(|e| RuntimeError::Generic(format!("HTTP request failed: {}", e)))?;

        let json_response = response.json::<serde_json::Value>().await
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse response: {}", e)))?;

        // Convert JSON back to RTFS Value
        Self::json_to_rtfs_value(&json_response)
    }

    /// Execute MCP capability
    async fn execute_mcp_capability(&self, _mcp: &MCPCapability, _inputs: &Value) -> RuntimeResult<Value> {
        // TODO: Implement MCP client
        Err(RuntimeError::Generic("MCP capabilities not yet implemented".to_string()))
    }

    /// Execute A2A capability
    async fn execute_a2a_capability(&self, _a2a: &A2ACapability, _inputs: &Value) -> RuntimeResult<Value> {
        // TODO: Implement A2A client
        Err(RuntimeError::Generic("A2A capabilities not yet implemented".to_string()))
    }

    /// Execute plugin capability
    async fn execute_plugin_capability(&self, _plugin: &PluginCapability, _inputs: &Value) -> RuntimeResult<Value> {
        // TODO: Implement plugin execution
        Err(RuntimeError::Generic("Plugin capabilities not yet implemented".to_string()))
    }

    /// Execute streaming capability
    async fn execute_stream_capability(&self, stream: &StreamCapabilityImpl, inputs: &Value) -> RuntimeResult<Value> {
        // For now, return a simple acknowledgment that streaming was initiated
        // In a full implementation, this would start the stream and return a stream handle
        // The actual streaming would be handled through the start_stream_consumption method
        match stream.metadata.stream_type {
            StreamType::Source => {
                // For source streams, we could return stream metadata or a handle
                Ok(Value::Map(HashMap::from([
                    (crate::ast::MapKey::Keyword(crate::ast::Keyword::new("stream_type")), Value::String("source".to_string())),
                    (crate::ast::MapKey::Keyword(crate::ast::Keyword::new("stream_id")), Value::String(stream.metadata.id.clone())),
                    (crate::ast::MapKey::Keyword(crate::ast::Keyword::new("status")), Value::String("ready".to_string())),
                ])))
            }
            StreamType::Sink => {
                // For sink streams, we could return acknowledgment of data receipt
                Ok(Value::Map(HashMap::from([
                    (crate::ast::MapKey::Keyword(crate::ast::Keyword::new("stream_type")), Value::String("sink".to_string())),
                    (crate::ast::MapKey::Keyword(crate::ast::Keyword::new("stream_id")), Value::String(stream.metadata.id.clone())),
                    (crate::ast::MapKey::Keyword(crate::ast::Keyword::new("status")), Value::String("received".to_string())),
                ])))
            }
            StreamType::Transform => {
                // For transform streams, we could return the transformed data
                Ok(Value::Map(HashMap::from([
                    (crate::ast::MapKey::Keyword(crate::ast::Keyword::new("stream_type")), Value::String("transform".to_string())),
                    (crate::ast::MapKey::Keyword(crate::ast::Keyword::new("stream_id")), Value::String(stream.metadata.id.clone())),
                    (crate::ast::MapKey::Keyword(crate::ast::Keyword::new("status")), Value::String("transformed".to_string())),
                    (crate::ast::MapKey::Keyword(crate::ast::Keyword::new("output")), inputs.clone()),
                ])))
            }
            StreamType::Bidirectional => {
                // For bidirectional streams, we could return a configuration or handle
                Ok(Value::Map(HashMap::from([
                    (crate::ast::MapKey::Keyword(crate::ast::Keyword::new("stream_type")), Value::String("bidirectional".to_string())),
                    (crate::ast::MapKey::Keyword(crate::ast::Keyword::new("stream_id")), Value::String(stream.metadata.id.clone())),
                    (crate::ast::MapKey::Keyword(crate::ast::Keyword::new("status")), Value::String("initialized".to_string())),
                ])))
            }
            StreamType::Duplex => {
                // For duplex streams, we could return separate handles for input and output
                Ok(Value::Map(HashMap::from([
                    (crate::ast::MapKey::Keyword(crate::ast::Keyword::new("stream_type")), Value::String("duplex".to_string())),
                    (crate::ast::MapKey::Keyword(crate::ast::Keyword::new("stream_id")), Value::String(stream.metadata.id.clone())),
                    (crate::ast::MapKey::Keyword(crate::ast::Keyword::new("status")), Value::String("ready".to_string())),
                ])))
            }
        }
    }

    /// Convert JSON value to RTFS Value
    fn json_to_rtfs_value(json: &serde_json::Value) -> RuntimeResult<Value> {
        match json {
            serde_json::Value::Null => Ok(Value::Nil),
            serde_json::Value::Bool(b) => Ok(Value::Boolean(*b)),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Ok(Value::Integer(i))
                } else if let Some(f) = n.as_f64() {
                    Ok(Value::Float(f))
                } else {
                    Err(RuntimeError::Generic("Invalid number format".to_string()))
                }
            }
            serde_json::Value::String(s) => Ok(Value::String(s.clone())),
            serde_json::Value::Array(arr) => {
                let values: Result<Vec<Value>, RuntimeError> = arr.iter()
                    .map(Self::json_to_rtfs_value)
                    .collect();
                Ok(Value::Vector(values?))
            }
            serde_json::Value::Object(obj) => {
                let mut map = HashMap::new();
                for (key, value) in obj {
                    let rtfs_key = crate::ast::MapKey::String(key.clone());
                    let rtfs_value = Self::json_to_rtfs_value(value)?;
                    map.insert(rtfs_key, rtfs_value);
                }
                Ok(Value::Map(map))
            }
        }
    }

    /// Add a discovery agent for automatic capability discovery
    pub fn add_discovery_agent(&mut self, agent: Box<dyn CapabilityDiscovery>) {
        self.discovery_agents.push(agent);
    }

    /// Discover capabilities from all registered discovery agents
    pub async fn discover_capabilities(&self) -> Result<usize, RuntimeError> {
        let mut discovered_count = 0;
        
        for agent in &self.discovery_agents {
            match agent.discover().await {
                Ok(capabilities) => {
                    let mut marketplace_capabilities = self.capabilities.write().await;
                    for capability in capabilities {
                        marketplace_capabilities.insert(capability.id.clone(), capability);
                        discovered_count += 1;
                    }
                }
                Err(e) => {
                    eprintln!("Discovery agent failed: {}", e);
                }
            }
        }
        
        Ok(discovered_count)
    }

    /// Start streaming with enhanced configuration and optional callbacks
    pub async fn start_stream_with_config(
        &self,
        capability_id: &str,
        params: &Value,
        config: &StreamConfig,
    ) -> RuntimeResult<StreamHandle> {
        let capability = self.get_capability(capability_id).await
            .ok_or_else(|| RuntimeError::Generic(format!("Capability '{}' not found", capability_id)))?;

        if let CapabilityProvider::Stream(stream_impl) = &capability.provider {
            if config.enable_callbacks {
                stream_impl.start_stream_with_config(params, config).await
            } else {
                // Fall back to channel-only mode
                let receiver = stream_impl.start_stream(params).await?;
                Ok(StreamHandle::new_channel_only(capability_id.to_string(), receiver))
            }
        } else {
            Err(RuntimeError::Generic(format!("Capability '{}' is not a stream capability", capability_id)))
        }
    }

    /// Start bidirectional stream with enhanced configuration and optional callbacks
    pub async fn start_bidirectional_stream_with_config(
        &self,
        capability_id: &str,
        params: &Value,
        config: &StreamConfig,
    ) -> RuntimeResult<StreamHandle> {
        let capability = self.get_capability(capability_id).await
            .ok_or_else(|| RuntimeError::Generic(format!("Capability '{}' not found", capability_id)))?;

        if let CapabilityProvider::Stream(stream_impl) = &capability.provider {
            if !matches!(stream_impl.metadata.stream_type, StreamType::Bidirectional) {
                return Err(RuntimeError::Generic(format!("Capability '{}' is not bidirectional", capability_id)));
            }
            
            if config.enable_callbacks {
                stream_impl.start_bidirectional_stream_with_config(params, config).await
            } else {
                // Fall back to channel-only mode
                let (sender, receiver) = stream_impl.start_bidirectional_stream(params).await?;
                Ok(StreamHandle::new_bidirectional_channel_only(capability_id.to_string(), sender, receiver))
            }
        } else {
            Err(RuntimeError::Generic(format!("Capability '{}' is not a stream capability", capability_id)))
        }
    }
}

/// Trait for capability discovery agents
#[async_trait::async_trait]
pub trait CapabilityDiscovery: Send + Sync {
    async fn discover(&self) -> Result<Vec<CapabilityImpl>, RuntimeError>;
}

/// Default implementation with common local capabilities
impl Default for CapabilityMarketplace {
    fn default() -> Self {
        let marketplace = Self::new();
        
        // For now, return an empty marketplace to avoid async issues
        // Capabilities will be registered when needed
        marketplace
    }
}

impl Clone for CapabilityMarketplace {
    fn clone(&self) -> Self {
        Self {
            capabilities: Arc::clone(&self.capabilities),
            discovery_agents: Vec::new(), // Discovery agents are not cloned
        }
    }
}

// Free async function for remote RTFS execution
pub async fn execute_remote_rtfs_capability(remote: &RemoteRTFSCapability, inputs: &Value) -> RuntimeResult<Value> {
    // Convert RTFS Value to JSON
    let json_inputs = serde_json::to_value(inputs)
        .map_err(|e| RuntimeError::Generic(format!("Failed to serialize inputs: {}", e)))?;

    // Make HTTP request to remote RTFS endpoint
    let client = reqwest::Client::new();
    let mut req = client
        .post(&remote.endpoint)
        .header("Content-Type", "application/json")
        .json(&json_inputs)
        .timeout(std::time::Duration::from_millis(remote.timeout_ms));
    if let Some(token) = &remote.auth_token {
        req = req.bearer_auth(token);
    }
    let response = req
        .send()
        .await
        .map_err(|e| RuntimeError::Generic(format!("Remote RTFS request failed: {}", e)))?;

    let json_response = response.json::<serde_json::Value>().await
        .map_err(|e| RuntimeError::Generic(format!("Failed to parse remote RTFS response: {}", e)))?;

    // Convert JSON back to RTFS Value
    CapabilityMarketplace::json_to_rtfs_value(&json_response)
}

/// Callback function type for streaming events
pub type StreamCallback = Arc<dyn Fn(StreamEvent) -> Result<(), RuntimeError> + Send + Sync>;

/// Async callback function type for streaming events
pub type AsyncStreamCallback = Arc<dyn Fn(StreamEvent) -> futures::future::BoxFuture<'static, Result<(), RuntimeError>> + Send + Sync>;

/// Stream event types that can trigger callbacks
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Stream connection established
    Connected { stream_id: String, metadata: HashMap<String, String> },
    /// Stream disconnected
    Disconnected { stream_id: String, reason: String },
    /// Data item received
    DataReceived { stream_id: String, item: StreamItem },
    /// Data item sent
    DataSent { stream_id: String, item: StreamItem },
    /// Stream error occurred
    Error { stream_id: String, error: String },
    /// Stream progress update
    Progress { stream_id: String, progress: ProgressNotification },
    /// Stream buffer full (backpressure)
    BackpressureTriggered { stream_id: String, buffer_size: usize },
    /// Stream buffer available again
    BackpressureRelieved { stream_id: String, buffer_size: usize },
}

/// Callback registration for stream events
#[derive(Clone)]
pub struct StreamCallbacks {
    /// Callback for connection events
    pub on_connected: Option<StreamCallback>,
    /// Callback for disconnection events
    pub on_disconnected: Option<StreamCallback>,
    /// Callback for data received events
    pub on_data_received: Option<StreamCallback>,
    /// Callback for data sent events
    pub on_data_sent: Option<StreamCallback>,
    /// Callback for error events
    pub on_error: Option<StreamCallback>,
    /// Callback for progress events
    pub on_progress: Option<StreamCallback>,
    /// Callback for backpressure events
    pub on_backpressure: Option<StreamCallback>,
}

impl std::fmt::Debug for StreamCallbacks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamCallbacks")
            .field("on_connected", &self.on_connected.as_ref().map(|_| "Some(callback)"))
            .field("on_disconnected", &self.on_disconnected.as_ref().map(|_| "Some(callback)"))
            .field("on_data_received", &self.on_data_received.as_ref().map(|_| "Some(callback)"))
            .field("on_data_sent", &self.on_data_sent.as_ref().map(|_| "Some(callback)"))
            .field("on_error", &self.on_error.as_ref().map(|_| "Some(callback)"))
            .field("on_progress", &self.on_progress.as_ref().map(|_| "Some(callback)"))
            .field("on_backpressure", &self.on_backpressure.as_ref().map(|_| "Some(callback)"))
            .finish()
    }
}

impl Default for StreamCallbacks {
    fn default() -> Self {
        Self {
            on_connected: None,
            on_disconnected: None,
            on_data_received: None,
            on_data_sent: None,
            on_error: None,
            on_progress: None,
            on_backpressure: None,
        }
    }
}

/// Stream configuration with optional callbacks
#[derive(Clone)]
pub struct StreamConfig {
    /// Buffer size for the stream
    pub buffer_size: usize,
    /// Enable/disable callbacks (default: false for channels-only mode)
    pub enable_callbacks: bool,
    /// Callback handlers (only used if enable_callbacks is true)
    pub callbacks: StreamCallbacks,
    /// Custom metadata for the stream
    pub metadata: HashMap<String, String>,
}

impl std::fmt::Debug for StreamConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamConfig")
            .field("buffer_size", &self.buffer_size)
            .field("enable_callbacks", &self.enable_callbacks)
            .field("callbacks", &self.callbacks)
            .field("metadata", &self.metadata)
            .finish()
    }
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            buffer_size: 100,
            enable_callbacks: false,
            callbacks: StreamCallbacks::default(),
            metadata: HashMap::new(),
        }
    }
}

/// Stream handle that provides both channel access and callback management
#[derive(Debug)]
pub struct StreamHandle {
    /// Stream identifier
    pub stream_id: String,
    /// Channel receiver (always available)
    pub receiver: Option<mpsc::Receiver<StreamItem>>,
    /// Channel sender (for bidirectional streams)
    pub sender: Option<mpsc::Sender<StreamItem>>,
    /// Callback configuration
    pub callbacks: StreamCallbacks,
    /// Whether callbacks are enabled
    pub callbacks_enabled: bool,
}

impl StreamHandle {
    /// Create a new stream handle with channels only
    pub fn new_channel_only(stream_id: String, receiver: mpsc::Receiver<StreamItem>) -> Self {
        Self {
            stream_id,
            receiver: Some(receiver),
            sender: None,
            callbacks: StreamCallbacks::default(),
            callbacks_enabled: false,
        }
    }

    /// Create a new bidirectional stream handle with channels only
    pub fn new_bidirectional_channel_only(
        stream_id: String,
        sender: mpsc::Sender<StreamItem>,
        receiver: mpsc::Receiver<StreamItem>,
    ) -> Self {
        Self {
            stream_id,
            receiver: Some(receiver),
            sender: Some(sender),
            callbacks: StreamCallbacks::default(),
            callbacks_enabled: false,
        }
    }

    /// Create a new stream handle with callbacks enabled
    pub fn new_with_callbacks(
        stream_id: String,
        receiver: Option<mpsc::Receiver<StreamItem>>,
        sender: Option<mpsc::Sender<StreamItem>>,
        callbacks: StreamCallbacks,
    ) -> Self {
        Self {
            stream_id,
            receiver,
            sender,
            callbacks,
            callbacks_enabled: true,
        }
    }

    /// Send a stream item (for bidirectional streams)
    pub async fn send(&self, item: StreamItem) -> Result<(), RuntimeError> {
        if let Some(sender) = &self.sender {
            // Send through channel
            sender.send(item.clone()).await.map_err(|e| {
                RuntimeError::Generic(format!("Failed to send stream item: {}", e))
            })?;

            // Trigger callback if enabled
            if self.callbacks_enabled {
                if let Some(callback) = &self.callbacks.on_data_sent {
                    callback(StreamEvent::DataSent {
                        stream_id: self.stream_id.clone(),
                        item,
                    })?;
                }
            }

            Ok(())
        } else {
            Err(RuntimeError::Generic("Stream is not bidirectional".to_string()))
        }
    }

    /// Receive a stream item (non-blocking)
    pub async fn try_recv(&mut self) -> Result<Option<StreamItem>, RuntimeError> {
        if let Some(receiver) = &mut self.receiver {
            match receiver.try_recv() {
                Ok(item) => {
                    // Trigger callback if enabled
                    if self.callbacks_enabled {
                        if let Some(callback) = &self.callbacks.on_data_received {
                            callback(StreamEvent::DataReceived {
                                stream_id: self.stream_id.clone(),
                                item: item.clone(),
                            })?;
                        }
                    }
                    Ok(Some(item))
                }
                Err(mpsc::error::TryRecvError::Empty) => Ok(None),
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    // Trigger disconnection callback if enabled
                    if self.callbacks_enabled {
                        if let Some(callback) = &self.callbacks.on_disconnected {
                            callback(StreamEvent::Disconnected {
                                stream_id: self.stream_id.clone(),
                                reason: "Channel disconnected".to_string(),
                            })?;
                        }
                    }
                    Err(RuntimeError::Generic("Stream channel disconnected".to_string()))
                }
            }
        } else {
            Err(RuntimeError::Generic("No receiver available".to_string()))
        }
    }

    /// Receive a stream item (blocking)
    pub async fn recv(&mut self) -> Result<StreamItem, RuntimeError> {
        if let Some(receiver) = &mut self.receiver {
            match receiver.recv().await {
                Some(item) => {
                    // Trigger callback if enabled
                    if self.callbacks_enabled {
                        if let Some(callback) = &self.callbacks.on_data_received {
                            callback(StreamEvent::DataReceived {
                                stream_id: self.stream_id.clone(),
                                item: item.clone(),
                            })?;
                        }
                    }
                    Ok(item)
                }
                None => {
                    // Trigger disconnection callback if enabled
                    if self.callbacks_enabled {
                        if let Some(callback) = &self.callbacks.on_disconnected {
                            callback(StreamEvent::Disconnected {
                                stream_id: self.stream_id.clone(),
                                reason: "Channel closed".to_string(),
                            })?;
                        }
                    }
                    Err(RuntimeError::Generic("Stream channel closed".to_string()))
                }
            }
        } else {
            Err(RuntimeError::Generic("No receiver available".to_string()))
        }
    }

    /// Trigger a custom stream event
    pub fn trigger_event(&self, event: StreamEvent) -> Result<(), RuntimeError> {
        if !self.callbacks_enabled {
            return Ok(());
        }

        match &event {
            StreamEvent::Connected { .. } => {
                if let Some(callback) = &self.callbacks.on_connected {
                    callback(event)?;
                }
            }
            StreamEvent::Disconnected { .. } => {
                if let Some(callback) = &self.callbacks.on_disconnected {
                    callback(event)?;
                }
            }
            StreamEvent::DataReceived { .. } => {
                if let Some(callback) = &self.callbacks.on_data_received {
                    callback(event)?;
                }
            }
            StreamEvent::DataSent { .. } => {
                if let Some(callback) = &self.callbacks.on_data_sent {
                    callback(event)?;
                }
            }
            StreamEvent::Error { .. } => {
                if let Some(callback) = &self.callbacks.on_error {
                    callback(event)?;
                }
            }
            StreamEvent::Progress { .. } => {
                if let Some(callback) = &self.callbacks.on_progress {
                    callback(event)?;
                }
            }
            StreamEvent::BackpressureTriggered { .. } | StreamEvent::BackpressureRelieved { .. } => {
                if let Some(callback) = &self.callbacks.on_backpressure {
                    callback(event)?;
                }
            }
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl StreamCapability for StreamCapabilityImpl {
    /// Start streaming - returns a receiver for stream items (channel-based, default)
    async fn start_stream(&self, _params: &Value) -> RuntimeResult<mpsc::Receiver<StreamItem>> {
        // Create a channel for stream items
        let (sender, receiver) = mpsc::channel::<StreamItem>(self.metadata.stream_config.as_ref().map(|c| c.buffer_size).unwrap_or(100));
        
        // TODO: In a real implementation, this would start the actual stream based on the provider
        // For now, we just return the receiver
        drop(sender); // Close the sender to indicate no more items will be sent
        
        Ok(receiver)
    }
    
    /// Start streaming with enhanced configuration and optional callbacks
    async fn start_stream_with_config(&self, _params: &Value, config: &StreamConfig) -> RuntimeResult<StreamHandle> {
        // Create a channel for stream items
        let (sender, receiver) = mpsc::channel::<StreamItem>(config.buffer_size);
        
        // TODO: In a real implementation, this would start the actual stream based on the provider
        // For now, we just return the handle
        drop(sender); // Close the sender to indicate no more items will be sent
        
        if config.enable_callbacks {
            Ok(StreamHandle::new_with_callbacks(
                self.metadata.id.clone(),
                Some(receiver),
                None,
                config.callbacks.clone(),
            ))
        } else {
            Ok(StreamHandle::new_channel_only(self.metadata.id.clone(), receiver))
        }
    }
    
    /// Send item to stream (for sinks, transforms, and bidirectional streams)
    async fn send_item(&self, _item: &StreamItem) -> RuntimeResult<()> {
        // TODO: In a real implementation, this would send the item based on the provider
        Ok(())
    }
    
    /// Start bidirectional stream - returns both sender and receiver (channel-based, default)
    async fn start_bidirectional_stream(&self, _params: &Value) -> RuntimeResult<(mpsc::Sender<StreamItem>, mpsc::Receiver<StreamItem>)> {
        let buffer_size = self.metadata.bidirectional_config.as_ref().map(|c| c.input_buffer_size).unwrap_or(100);
        
        // Create bidirectional channels
        let (sender, receiver) = mpsc::channel::<StreamItem>(buffer_size);
        
        // TODO: In a real implementation, this would start the actual bidirectional stream based on the provider
        
        Ok((sender, receiver))
    }
    
    /// Start bidirectional stream with enhanced configuration and optional callbacks
    async fn start_bidirectional_stream_with_config(&self, _params: &Value, config: &StreamConfig) -> RuntimeResult<StreamHandle> {
        // Create bidirectional channels
        let (sender, receiver) = mpsc::channel::<StreamItem>(config.buffer_size);
        
        // TODO: In a real implementation, this would start the actual bidirectional stream based on the provider
        
        if config.enable_callbacks {
            Ok(StreamHandle::new_with_callbacks(
                self.metadata.id.clone(),
                Some(receiver),
                Some(sender),
                config.callbacks.clone(),
            ))
        } else {
            Ok(StreamHandle::new_bidirectional_channel_only(self.metadata.id.clone(), sender, receiver))
        }
    }
    
    /// Start duplex stream - returns separate channels for input and output
    async fn start_duplex_stream(&self, _params: &Value) -> RuntimeResult<DuplexStreamChannels> {
        let buffer_size = self.metadata.duplex_config.as_ref()
            .map(|c| c.input_channel.buffer_size)
            .unwrap_or(100);
        
        // Create duplex channels
        let (input_tx, _input_rx) = mpsc::channel::<StreamItem>(buffer_size);
        let (_output_tx, output_rx) = mpsc::channel::<StreamItem>(buffer_size);
        let (_feedback_tx, feedback_rx) = mpsc::channel::<StreamItem>(50);
        
        // TODO: In a real implementation, this would start the actual duplex stream based on the provider
        
        Ok(DuplexStreamChannels {
            input_sender: input_tx,
            output_receiver: output_rx,
            feedback_receiver: Some(feedback_rx),
        })
    }
    
    /// Get current stream progress (optional)
    async fn get_progress(&self, _token: &ProgressToken) -> RuntimeResult<Option<ProgressNotification>> {
        // TODO: In a real implementation, this would return actual progress
        Ok(None)
    }
    
    /// Cancel stream operation (optional)
    async fn cancel(&self, _token: &ProgressToken) -> RuntimeResult<()> {
        // TODO: In a real implementation, this would cancel the stream
        Ok(())
    }
}