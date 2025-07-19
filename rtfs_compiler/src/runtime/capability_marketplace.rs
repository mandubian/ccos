// ...existing code...
// ...existing code...
use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::runtime::values::Value;
use crate::runtime::capability_registry::CapabilityRegistry;
use std::collections::HashMap;
use std::sync::Arc;
use bincode::de;
use tokio::sync::RwLock;
use tokio::sync::mpsc;
use async_trait::async_trait;

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
    pub input_channel: String,
    pub output_channel: String,
}

/// Callback function for stream events
pub type StreamCallback = Arc<dyn Fn(Value) -> RuntimeResult<()> + Send + Sync>;

/// Callbacks for stream events
#[derive(Clone)]
pub struct StreamCallbacks {
    pub on_connected: Option<StreamCallback>,
    pub on_disconnected: Option<StreamCallback>,
    pub on_data_received: Option<StreamCallback>,
    pub on_error: Option<StreamCallback>,
}

impl std::fmt::Debug for StreamCallbacks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamCallbacks")
         .field("on_connected", &self.on_connected.is_some())
         .field("on_disconnected", &self.on_disconnected.is_some())
         .field("on_data_received", &self.on_data_received.is_some())
         .field("on_error", &self.on_error.is_some())
         .finish()
    }
}

impl Default for StreamCallbacks {
    fn default() -> Self {
        Self {
            on_connected: None,
            on_disconnected: None,
            on_data_received: None,
            on_error: None,
        }
    }
}


/// Configuration for streaming capabilities
#[derive(Debug, Clone)]
pub struct StreamConfig {
    pub callbacks: Option<StreamCallbacks>,
    pub auto_reconnect: bool,
    pub max_retries: u32,
}

/// Streaming capability implementation details
#[derive(Clone)]
pub struct StreamCapabilityImpl {
    pub provider: StreamingProvider,
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

impl std::fmt::Debug for StreamCapabilityImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamCapabilityImpl")
         .field("stream_type", &self.stream_type)
         .field("input_schema", &self.input_schema)
         .field("output_schema", &self.output_schema)
         .field("supports_progress", &self.supports_progress)
         .field("supports_cancellation", &self.supports_cancellation)
         .field("bidirectional_config", &self.bidirectional_config)
         .field("duplex_config", &self.duplex_config)
         .field("stream_config", &self.stream_config)
         .finish()
    }
}

/// Represents a capability implementation
#[derive(Debug, Clone)]
pub struct CapabilityManifest {
    pub id: String,
    pub name: String,
    pub description: String,
    pub provider_type: ProviderType,
    pub local: bool,
    pub endpoint: Option<String>,
}

/// Different types of capability providers
#[derive(Debug, Clone)]
pub enum ProviderType {
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
        f.debug_struct("LocalCapability").finish()
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

/// Handle for managing a stream
#[derive(Debug, Clone)]
pub struct StreamHandle {
    pub stream_id: String,
    pub stop_tx: mpsc::Sender<()>,
}

/// Trait for streaming capability providers
#[async_trait]
pub trait StreamingCapability {
    /// Start a stream
    fn start_stream(&self, params: &Value) -> RuntimeResult<StreamHandle>;
    /// Stop a stream
    fn stop_stream(&self, handle: &StreamHandle) -> RuntimeResult<()>;
    /// Start a stream with extended configuration
    async fn start_stream_with_config(&self, params: &Value, config: &StreamConfig) -> RuntimeResult<StreamHandle>;
    /// Send data to a stream
    async fn send_to_stream(&self, handle: &StreamHandle, data: &Value) -> RuntimeResult<()>;
    /// Start a bidirectional stream
    fn start_bidirectional_stream(&self, params: &Value) -> RuntimeResult<StreamHandle>;
    /// Start a bidirectional stream with extended configuration
    async fn start_bidirectional_stream_with_config(&self, params: &Value, config: &StreamConfig) -> RuntimeResult<StreamHandle>;
}

/// Type alias for a thread-safe, shareable streaming capability provider
pub type StreamingProvider = Arc<dyn StreamingCapability + Send + Sync>;

/// The capability marketplace that manages all available capabilities
pub struct CapabilityMarketplace {
    capabilities: Arc<RwLock<HashMap<String, CapabilityManifest>>>,
    discovery_agents: Vec<Box<dyn CapabilityDiscovery>>,
    // Add a field for the capability registry
    capability_registry: Arc<RwLock<CapabilityRegistry>>,
}

impl CapabilityMarketplace {
    /// Create a new capability marketplace
    pub fn new(capability_registry: Arc<RwLock<CapabilityRegistry>>) -> Self {
        Self {
            capabilities: Arc::new(RwLock::new(HashMap::new())),
            discovery_agents: Vec::new(),
            capability_registry,
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
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider_type: ProviderType::Local(LocalCapability { handler }),
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
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider_type: ProviderType::Http(HttpCapability {
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
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider_type: ProviderType::RemoteRTFS(RemoteRTFSCapability {
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
        let stream_impl = StreamCapabilityImpl {
            provider,
            stream_type,
            input_schema: None,
            output_schema: None,
            supports_progress: true,
            supports_cancellation: true,
            bidirectional_config: None,
            duplex_config: None,
            stream_config: None,
        };
        
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider_type: ProviderType::Stream(stream_impl),
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
        let stream_impl = StreamCapabilityImpl {
            provider,
            stream_type: StreamType::Bidirectional,
            input_schema: None,
            output_schema: None,
            supports_progress: true,
            supports_cancellation: true,
            bidirectional_config: Some(config),
            duplex_config: None,
            stream_config: None,
        };
        
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider_type: ProviderType::Stream(stream_impl),
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
        let stream_impl = StreamCapabilityImpl {
            provider,
            stream_type: StreamType::Duplex,
            input_schema: None,
            output_schema: None,
            supports_progress: true,
            supports_cancellation: true,
            bidirectional_config: None,
            duplex_config: Some(duplex_config),
            stream_config: None,
        };
        
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider_type: ProviderType::Stream(stream_impl),
            local: false,
            endpoint: None,
        };
        
        let mut capabilities = self.capabilities.write().await;
        capabilities.insert(id, capability);
        Ok(())
    }

    /// Execute a capability by its ID
    pub async fn execute_capability(&self, id: &str, inputs: &Value) -> RuntimeResult<Value> {
        let capability = self.get_capability(id).await
            .ok_or_else(|| RuntimeError::Generic(format!("Capability '{}' not found", id)))?;

        match &capability.provider_type {
            ProviderType::Local(local) => {
                // Execute local capability synchronously
                (local.handler)(inputs)
            }
            ProviderType::Http(http) => {
                // Execute HTTP capability asynchronously
                self.execute_http_capability(http, inputs).await
            }
            ProviderType::MCP(mcp) => {
                // Execute MCP capability asynchronously
                self.execute_mcp_capability(mcp, inputs).await
            }
            ProviderType::A2A(a2a) => {
                // Execute A2A capability asynchronously
                self.execute_a2a_capability(a2a, inputs).await
            }
            ProviderType::Plugin(plugin) => {
                // Execute plugin capability
                self.execute_plugin_capability(plugin, inputs).await
            }
            ProviderType::RemoteRTFS(remote_rtfs) => {
                // Execute remote RTFS capability
                self.execute_remote_rtfs_capability(remote_rtfs, inputs).await
            }
            ProviderType::Stream(stream) => {
                // Execute streaming capability
                self.execute_stream_capability(stream, inputs).await
            }
        }
    }

    /// Get a capability by its ID
    pub async fn get_capability(&self, id: &str) -> Option<CapabilityManifest> {
        let capabilities = self.capabilities.read().await;
        capabilities.get(id).cloned()
    }

    /// List all available capabilities
    pub async fn list_capabilities(&self) -> Vec<CapabilityManifest> {
        let capabilities = self.capabilities.read().await;
        capabilities.values().cloned().collect()
    }

    /// Execute HTTP capability
    async fn execute_http_capability(&self, http: &HttpCapability, inputs: &Value) -> RuntimeResult<Value> {
        // For HTTP capabilities, we expect the inputs to be in RTFS format: [url, method, headers, body]
        // Convert inputs to args format if needed
        let args = match inputs {
            Value::List(list) => list.clone(),
            Value::Vector(vec) => vec.clone(),
            single_value => vec![single_value.clone()],
        };
        
        // Extract HTTP parameters from args
        let url = args.get(0).and_then(|v| v.as_string()).unwrap_or(&http.base_url);
        let method = args.get(1).and_then(|v| v.as_string()).unwrap_or("GET");
        let default_headers = std::collections::HashMap::new();
        let headers = args.get(2).and_then(|v| match v {
            Value::Map(m) => Some(m),
            _ => None,
        }).unwrap_or(&default_headers);
        let body = args.get(3).and_then(|v| v.as_string()).unwrap_or("").to_string();

        // Make HTTP request
        let client = reqwest::Client::new();
        let method_enum = reqwest::Method::from_bytes(method.as_bytes()).unwrap_or(reqwest::Method::GET);
        let mut req = client.request(method_enum, url);
        
        // Add authentication if provided
        if let Some(token) = &http.auth_token {
            req = req.bearer_auth(token);
        }
        
        // Add custom headers
        for (k, v) in headers.iter() {
            if let crate::ast::MapKey::String(ref key) = k {
                if let Value::String(ref val) = v {
                    req = req.header(key, val);
                }
            }
        }
        
        // Add body if provided
        if !body.is_empty() {
            req = req.body(body);
        }
        
        // Execute request with timeout
        let response = req
            .timeout(std::time::Duration::from_millis(http.timeout_ms))
            .send()
            .await
            .map_err(|e| RuntimeError::Generic(format!("HTTP request failed: {}", e)))?;

        // Extract response details before consuming
        let status = response.status().as_u16() as i64;
        let response_headers = response.headers().clone();
        let resp_body = response.text().await.unwrap_or_default();

        // Build response map
        let mut response_map = std::collections::HashMap::new();
        response_map.insert(
            crate::ast::MapKey::String("status".to_string()),
            Value::Integer(status),
        );
        
        response_map.insert(
            crate::ast::MapKey::String("body".to_string()),
            Value::String(resp_body),
        );
        
        let mut headers_map = std::collections::HashMap::new();
        for (key, value) in response_headers.iter() {
            headers_map.insert(
                crate::ast::MapKey::String(key.to_string()),
                Value::String(value.to_str().unwrap_or("").to_string()),
            );
        }
        response_map.insert(
            crate::ast::MapKey::String("headers".to_string()),
            Value::Map(headers_map),
        );

        Ok(Value::Map(response_map))
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

    /// Execute remote RTFS capability
    async fn execute_remote_rtfs_capability(&self, _remote_rtfs: &RemoteRTFSCapability, _inputs: &Value) -> RuntimeResult<Value> {
        // TODO: Implement remote RTFS execution
        Err(RuntimeError::Generic("Remote RTFS capabilities not yet implemented".to_string()))
    }

    /// Execute streaming capability
    async fn execute_stream_capability(
        &self,
        stream_impl: &StreamCapabilityImpl,
        inputs: &Value,
    ) -> RuntimeResult<Value> {
        // For now, we just start the stream. The handle would need to be managed.
        // This is a simplification. A real implementation would need to return the handle
        // or manage the stream lifecycle.
        let handle = stream_impl.provider.start_stream(inputs)?;
        Ok(Value::String(format!("Stream started with ID: {}", handle.stream_id)))
    }

    /// Execute a local capability
    fn execute_local_capability(&self, local: &LocalCapability, inputs: &Value) -> RuntimeResult<Value> {
        (local.handler)(inputs)
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

        if let ProviderType::Stream(stream_impl) = &capability.provider_type {
            if config.callbacks.is_some() {
                stream_impl.provider.start_stream_with_config(params, config).await
            } else {
                let handle = stream_impl.provider.start_stream(params)?;
                Ok(handle)
            }
        } else {
            Err(RuntimeError::Generic(format!("Capability '{}' is not a stream capability", capability_id)))
        }
    }

    /// Start a bidirectional stream with enhanced configuration and optional callbacks
    pub async fn start_bidirectional_stream_with_config(
        &self,
        capability_id: &str,
        params: &Value,
        config: &StreamConfig,
    ) -> RuntimeResult<StreamHandle> {
        let capability = self.get_capability(capability_id).await
            .ok_or_else(|| RuntimeError::Generic(format!("Capability '{}' not found", capability_id)))?;

        if let ProviderType::Stream(stream_impl) = &capability.provider_type {
            if !matches!(stream_impl.stream_type, StreamType::Bidirectional) {
                return Err(RuntimeError::Generic(format!("Capability '{}' is not bidirectional", capability_id)));
            }
            if config.callbacks.is_some() {
                stream_impl.provider.start_bidirectional_stream_with_config(params, config).await
            } else {
                let handle = stream_impl.provider.start_bidirectional_stream(params)?;
                Ok(handle)
            }
        } else {
            Err(RuntimeError::Generic(format!("Capability '{}' is not a stream capability", capability_id)))
        }
    }
}

/// Trait for capability discovery agents
#[async_trait::async_trait]
pub trait CapabilityDiscovery: Send + Sync {
    /// Discover capabilities from a source
    async fn discover(&self) -> Result<Vec<CapabilityManifest>, RuntimeError>;
}

/// Example: a simple discovery agent that returns a fixed list of capabilities
pub struct NoOpCapabilityDiscovery;

#[async_trait::async_trait]
impl CapabilityDiscovery for NoOpCapabilityDiscovery {
    async fn discover(&self) -> Result<Vec<CapabilityManifest>, RuntimeError> {
        Ok(vec![])
    }
}

/// Test suite for capability marketplace
#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::values::Value;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_register_and_execute_local_capability() {
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = CapabilityMarketplace::new(registry.clone());

        let handler = Arc::new(|inputs: &Value| {
            Ok(inputs.clone())
        });

        marketplace.register_local_capability(
            "test.echo".to_string(),
            "Test Echo".to_string(),
            "A simple echo capability".to_string(),
            handler,
        ).await.unwrap();

        let inputs = Value::String("Hello".to_string());
        let result = marketplace.execute_capability("test.echo", &inputs).await.unwrap();

        assert_eq!(result, inputs);
    }
}