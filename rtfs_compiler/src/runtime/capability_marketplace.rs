// ...existing code...
use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::runtime::values::Value;
use crate::runtime::capability_registry::CapabilityRegistry;
use crate::ast::MapKey;
use crate::ast::TypeExpr; // Add import for RTFS type expressions
use std::collections::HashMap;
use std::sync::Arc;
use bincode::de;
use tokio::sync::RwLock;
use tokio::sync::mpsc;
use async_trait::async_trait;
// use rmcp::client::Client;
// use rmcp::transport::HttpTransport;
use serde_json::json;
use std::any::{Any, TypeId};
use jsonschema::{JSONSchema, ValidationError};
use sha2::{Sha256, Digest};
use chrono::{DateTime, Utc};

// MCP SDK imports (disabled due to dependency issues)
// use rmcp::{
//     ServiceExt,
//     model::{CallToolRequestParam, CallToolResult, Content},
//     transport::{SseClientTransport, ConfigureCommandExt, TokioChildProcess},
// };

/// Progress token for tracking long-running operations (MCP-style)
pub type ProgressToken = String;

/// Cursor for pagination (MCP-style)
pub type Cursor = String;

/// Capability attestation information
#[derive(Debug, Clone, PartialEq)]
pub struct CapabilityAttestation {
    /// Digital signature or hash of capability
    pub signature: String,
    /// Attestation authority (e.g., organization, registry)
    pub authority: String,
    /// When the attestation was created
    pub created_at: DateTime<Utc>,
    /// When the attestation expires
    pub expires_at: Option<DateTime<Utc>>,
    /// Additional attestation metadata
    pub metadata: HashMap<String, String>,
}

/// Capability provenance tracking
#[derive(Debug, Clone, PartialEq)]
pub struct CapabilityProvenance {
    /// Source where capability was discovered/loaded
    pub source: String,
    /// Version information
    pub version: Option<String>,
    /// Hash of capability definition for integrity
    pub content_hash: String,
    /// Chain of custody information
    pub custody_chain: Vec<String>,
    /// When capability was registered
    pub registered_at: DateTime<Utc>,
}

/// Network-based capability registry configuration
#[derive(Debug, Clone)]
pub struct NetworkRegistryConfig {
    /// Registry endpoint URL
    pub endpoint: String,
    /// Authentication token for registry access
    pub auth_token: Option<String>,
    /// How often to refresh capability list (seconds)
    pub refresh_interval: u64,
    /// Whether to verify attestations
    pub verify_attestations: bool,
}

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
    pub input_schema: Option<TypeExpr>, // RTFS type expression for input validation
    pub output_schema: Option<TypeExpr>, // RTFS type expression for output validation
    pub supports_progress: bool,
    pub supports_cancellation: bool,
    /// Configuration for bidirectional streams
    pub bidirectional_config: Option<BidirectionalConfig>,
    /// Configuration for duplex streams
    pub duplex_config: Option<DuplexChannels>,
    /// Stream configuration with optional callbacks
    pub stream_config: Option<StreamConfig>,
}

impl PartialEq for StreamCapabilityImpl {
    fn eq(&self, other: &Self) -> bool {
        // Compare all fields except provider (which contains trait objects)
        self.stream_type == other.stream_type
            && self.input_schema == other.input_schema
            && self.output_schema == other.output_schema
            && self.supports_progress == other.supports_progress
            && self.supports_cancellation == other.supports_cancellation
            && self.bidirectional_config == other.bidirectional_config
            && self.duplex_config == other.duplex_config
            && self.stream_config == other.stream_config
    }
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
    pub provider: ProviderType,
    pub version: String,
    pub input_schema: Option<TypeExpr>,  // RTFS type expression for input validation
    pub output_schema: Option<TypeExpr>, // RTFS type expression for output validation
    pub attestation: Option<CapabilityAttestation>,
    pub provenance: Option<CapabilityProvenance>,
    pub permissions: Vec<String>,
    pub metadata: std::collections::HashMap<String, String>,
}

/// Different types of capability providers
#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Clone, PartialEq)]
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

impl PartialEq for LocalCapability {
    fn eq(&self, other: &Self) -> bool {
        // Compare function pointer addresses since we can't compare function content
        Arc::ptr_eq(&self.handler, &other.handler)
    }
}

/// HTTP-based remote capability
#[derive(Debug, Clone, PartialEq)]
pub struct HttpCapability {
    pub base_url: String,
    pub auth_token: Option<String>,
    pub timeout_ms: u64,
}

/// MCP server capability
#[derive(Debug, Clone, PartialEq)]
pub struct MCPCapability {
    pub server_url: String,
    pub tool_name: String,
    pub timeout_ms: u64,
}

/// A2A communication capability
#[derive(Debug, Clone, PartialEq)]
pub struct A2ACapability {
    pub agent_id: String,
    pub endpoint: String,
    pub protocol: String, // "http", "websocket", "grpc"
    pub timeout_ms: u64,
}

/// Plugin-based capability
#[derive(Debug, Clone, PartialEq)]
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
    executors: HashMap<TypeId, Arc<dyn CapabilityExecutor>>,
    network_registry: Option<NetworkRegistryConfig>,
}

impl CapabilityMarketplace {
    /// Create a new capability marketplace
    pub fn new(capability_registry: Arc<RwLock<CapabilityRegistry>>) -> Self {
        let mut marketplace = Self {
            capabilities: Arc::new(RwLock::new(HashMap::new())),
            discovery_agents: Vec::new(),
            capability_registry,
            executors: HashMap::new(),
            network_registry: None,
        };
        marketplace.register_executor(Arc::new(MCPExecutor));
        marketplace.register_executor(Arc::new(A2AExecutor));
        marketplace.register_executor(Arc::new(LocalExecutor));
        marketplace.register_executor(Arc::new(HttpExecutor));
        marketplace
    }

    /// Compute SHA-256 hash of capability content for integrity verification
    fn compute_content_hash(&self, content: &str) -> String {
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Register a local capability
    pub async fn register_local_capability(
        &self,
        id: String,
        name: String,
        description: String,
        handler: Arc<dyn Fn(&Value) -> RuntimeResult<Value> + Send + Sync>,
    ) -> Result<(), RuntimeError> {
        self.register_local_capability_with_schema(
            id, name, description, handler, None, None
        ).await
    }

    /// Register a local capability with schema validation
    pub async fn register_local_capability_with_schema(
        &self,
        id: String,
        name: String,
        description: String,
        handler: Arc<dyn Fn(&Value) -> RuntimeResult<Value> + Send + Sync>,
        input_schema: Option<TypeExpr>,
        output_schema: Option<TypeExpr>,
    ) -> Result<(), RuntimeError> {
        let content_hash = self.compute_content_hash(&format!("{}{}{}", id, name, description));
        let provenance = CapabilityProvenance {
            source: "local".to_string(),
            version: Some("1.0.0".to_string()),
            content_hash,
            custody_chain: vec!["local_registration".to_string()],
            registered_at: Utc::now(),
        };

        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::Local(LocalCapability { handler }),
            version: "1.0.0".to_string(),
            input_schema,
            output_schema,
            attestation: None, // Local capabilities don't need external attestation
            provenance: Some(provenance),
            permissions: vec![],
            metadata: HashMap::new(),
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
        self.register_http_capability_with_schema(
            id, name, description, base_url, auth_token, None, None
        ).await
    }

    /// Register a remote HTTP capability with schema validation
    pub async fn register_http_capability_with_schema(
        &self,
        id: String,
        name: String,
        description: String,
        base_url: String,
        auth_token: Option<String>,
        input_schema: Option<TypeExpr>,
        output_schema: Option<TypeExpr>,
    ) -> Result<(), RuntimeError> {
        let content_hash = self.compute_content_hash(&format!("{}{}{}{}", id, name, description, base_url));
        let provenance = CapabilityProvenance {
            source: format!("http:{}", base_url),
            version: Some("1.0.0".to_string()),
            content_hash,
            custody_chain: vec!["http_registration".to_string()],
            registered_at: Utc::now(),
        };

        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::Http(HttpCapability {
                base_url,
                auth_token,
                timeout_ms: 5000,
            }),
            version: "1.0.0".to_string(),
            input_schema,
            output_schema,
            attestation: None, // Could be added for verified HTTP endpoints
            provenance: Some(provenance),
            permissions: vec![],
            metadata: HashMap::new(),
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
        let content_hash = self.compute_content_hash(&format!("{}{}{}{}", id, name, description, endpoint));
        let provenance = CapabilityProvenance {
            source: format!("remote_rtfs:{}", endpoint),
            version: Some("1.0.0".to_string()),
            content_hash,
            custody_chain: vec!["remote_rtfs_registration".to_string()],
            registered_at: Utc::now(),
        };

        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::RemoteRTFS(RemoteRTFSCapability {
                endpoint,
                timeout_ms,
                auth_token,
            }),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            metadata: HashMap::new(),
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
        let content_hash = self.compute_content_hash(&format!("{}{}{}", id, name, description));
        let provenance = CapabilityProvenance {
            source: "streaming".to_string(),
            version: Some("1.0.0".to_string()),
            content_hash,
            custody_chain: vec!["streaming_registration".to_string()],
            registered_at: Utc::now(),
        };

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
            provider: ProviderType::Stream(stream_impl),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            metadata: HashMap::new(),
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
        let content_hash = self.compute_content_hash(&format!("{}{}{}", id, name, description));
        let provenance = CapabilityProvenance {
            source: "bidirectional_stream".to_string(),
            version: Some("1.0.0".to_string()),
            content_hash,
            custody_chain: vec!["bidirectional_stream_registration".to_string()],
            registered_at: Utc::now(),
        };

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
            provider: ProviderType::Stream(stream_impl),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            metadata: HashMap::new(),
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
        let content_hash = self.compute_content_hash(&format!("{}{}{}", id, name, description));
        let provenance = CapabilityProvenance {
            source: "duplex_stream".to_string(),
            version: Some("1.0.0".to_string()),
            content_hash,
            custody_chain: vec!["duplex_stream_registration".to_string()],
            registered_at: Utc::now(),
        };

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
            provider: ProviderType::Stream(stream_impl),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            metadata: HashMap::new(),
        };
        
        let mut capabilities = self.capabilities.write().await;
        capabilities.insert(id, capability);
        Ok(())
    }

    /// Execute capability with input/output schema validation
    pub async fn execute_with_validation(
        &self,
        capability_id: &str,
        params: &HashMap<String, Value>,
    ) -> Result<Value, RuntimeError> {
        let capability = {
            let capabilities = self.capabilities.read().await;
            capabilities.get(capability_id).cloned()
                .ok_or_else(|| RuntimeError::Generic(format!("Capability not found: {}", capability_id)))?
        };

        // Validate input against schema if present
        if let Some(input_schema) = &capability.input_schema {
            self.validate_input_schema(params, input_schema).await?;
        }

        // Convert params to Value for execution
        let inputs_value = self.params_to_value(params)?;

        // Execute the capability
        let result = self.execute_capability(capability_id, &inputs_value).await?;

        // Validate output against schema if present
        if let Some(output_schema) = &capability.output_schema {
            self.validate_output_schema(&result, output_schema).await?;
        }

        Ok(result)
    }

    /// Validate input parameters against JSON schema
    async fn validate_input_schema(
        &self,
        params: &HashMap<String, Value>,
        schema_expr: &TypeExpr,
    ) -> Result<(), RuntimeError> {
        use jsonschema::JSONSchema;
        use serde_json::{Map, Value as JsonValue};

        let schema_json = schema_expr.to_json()
            .map_err(|e| RuntimeError::Generic(format!("Schema conversion failed: {}", e)))?;
        
        let compiled = JSONSchema::compile(&schema_json)
            .map_err(|e| RuntimeError::Generic(format!("Schema compilation failed: {}", e)))?;

        // Convert RTFS params to JSON for validation
        let json_params = self.params_to_json(params)?;
        
        let validation_result = compiled.validate(&json_params);
        if let Err(errors) = validation_result {
            let error_msgs: Vec<String> = errors.map(|e| e.to_string()).collect();
            return Err(RuntimeError::Generic(format!("Input validation failed: {}", error_msgs.join(", "))));
        }

        Ok(())
    }

    /// Validate output against JSON schema
    async fn validate_output_schema(
        &self,
        result: &Value,
        schema_expr: &TypeExpr,
    ) -> Result<(), RuntimeError> {
        use jsonschema::JSONSchema;
        use serde_json::Value as JsonValue;

        let schema_json = schema_expr.to_json()
            .map_err(|e| RuntimeError::Generic(format!("Schema conversion failed: {}", e)))?;
        
        let compiled = JSONSchema::compile(&schema_json)
            .map_err(|e| RuntimeError::Generic(format!("Schema compilation failed: {}", e)))?;

        // Convert RTFS result to JSON for validation
        let json_result = self.value_to_json(result)?;
        
        let validation_result = compiled.validate(&json_result);
        if let Err(errors) = validation_result {
            let error_msgs: Vec<String> = errors.map(|e| e.to_string()).collect();
            return Err(RuntimeError::Generic(format!("Output validation failed: {}", error_msgs.join(", "))));
        }

        Ok(())
    }

    /// Convert RTFS parameters to JSON Value for schema validation
    fn params_to_json(&self, params: &HashMap<String, Value>) -> Result<serde_json::Value, RuntimeError> {
        use serde_json::{Map, Value as JsonValue};
        
        let mut json_map = Map::new();
        for (key, value) in params {
            json_map.insert(key.clone(), self.value_to_json(value)?);
        }
        Ok(JsonValue::Object(json_map))
    }

    /// Convert HashMap params to RTFS Value for execution
    fn params_to_value(&self, params: &HashMap<String, Value>) -> Result<Value, RuntimeError> {
        let mut map = HashMap::new();
        for (key, value) in params {
            map.insert(MapKey::String(key.clone()), value.clone());
        }
        Ok(Value::Map(map))
    }

    /// Convert RTFS Value to JSON Value for schema validation
    fn value_to_json(&self, value: &Value) -> Result<serde_json::Value, RuntimeError> {
        use serde_json::Value as JsonValue;
        
        match value {
            Value::Integer(i) => Ok(JsonValue::Number(serde_json::Number::from(*i))),
            Value::Float(f) => Ok(JsonValue::Number(serde_json::Number::from_f64(*f)
                .ok_or_else(|| RuntimeError::Generic("Invalid float value".to_string()))?)),
            Value::String(s) => Ok(JsonValue::String(s.clone())),
            Value::Boolean(b) => Ok(JsonValue::Bool(*b)),
            Value::Vector(vec) => {
                let json_vec: Result<Vec<JsonValue>, RuntimeError> = vec.iter()
                    .map(|v| self.value_to_json(v))
                    .collect();
                Ok(JsonValue::Array(json_vec?))
            },
            Value::Map(map) => {
                let mut json_map = serde_json::Map::new();
                for (key, val) in map {
                    let key_str = match key {
                        MapKey::String(s) => s.clone(),
                        MapKey::Keyword(k) => k.0.clone(),
                        _ => return Err(RuntimeError::Generic("Map keys must be strings or keywords".to_string())),
                    };
                    json_map.insert(key_str, self.value_to_json(val)?);
                }
                Ok(JsonValue::Object(json_map))
            },
            Value::Nil => Ok(JsonValue::Null),
            _ => Err(RuntimeError::Generic(format!("Cannot convert {} to JSON", value.type_name()))),
        }
    }

    /// Execute a capability by its ID
    pub async fn execute_capability(&self, id: &str, inputs: &Value) -> RuntimeResult<Value> {
        let capabilities = self.capabilities.read().await;
        let capability = match capabilities.get(id) {
            Some(c) => c,
            None => return Err(RuntimeError::Generic(format!("Capability '{}' not found", id))),
        };
        let provider = &capability.provider;
        let type_id = match provider {
            ProviderType::Local(_) => TypeId::of::<LocalCapability>(),
            ProviderType::Http(_) => TypeId::of::<HttpCapability>(),
            ProviderType::MCP(_) => TypeId::of::<MCPCapability>(),
            ProviderType::A2A(_) => TypeId::of::<A2ACapability>(),
            ProviderType::Plugin(_) => TypeId::of::<PluginCapability>(),
            ProviderType::RemoteRTFS(_) => TypeId::of::<RemoteRTFSCapability>(),
            ProviderType::Stream(_) => TypeId::of::<StreamCapabilityImpl>(),
        };
        if let Some(executor) = self.executors.get(&type_id) {
            executor.execute(provider, inputs).await
        } else {
            Err(RuntimeError::Generic("No executor registered for this provider type".to_string()))
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

    /// Execute MCP capability using JSON-RPC protocol
    async fn execute_mcp_capability(&self, mcp: &MCPCapability, inputs: &Value) -> RuntimeResult<Value> {
        let client = reqwest::Client::new();
        
        // Prepare JSON-RPC MCP request payload according to MCP specification
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": mcp.tool_name,
                "arguments": self.value_to_json(inputs)?
            }
        });

        // Make MCP server request
        let response = client
            .post(&mcp.server_url)
            .json(&payload)
            .timeout(std::time::Duration::from_millis(mcp.timeout_ms))
            .send()
            .await
            .map_err(|e| RuntimeError::Generic(format!("MCP request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(RuntimeError::Generic(format!("MCP server error: {}", response.status())));
        }

        let response_json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse MCP response: {}", e)))?;

        // Extract result from MCP JSON-RPC response
        if let Some(result) = response_json.get("result") {
            Self::json_to_rtfs_value(result)
        } else if let Some(error) = response_json.get("error") {
            Err(RuntimeError::Generic(format!("MCP server error: {}", error)))
        } else {
            Err(RuntimeError::Generic("Invalid MCP response format".to_string()))
        }
    }

    /// Execute A2A capability
    async fn execute_a2a_capability(&self, a2a: &A2ACapability, inputs: &Value) -> RuntimeResult<Value> {
        // A2A (Agent-to-Agent) communication implementation
        let client = reqwest::Client::new();
        
        // Prepare A2A request payload
        let payload = serde_json::json!({
            "source_agent": "rtfs_capability_marketplace",
            "target_agent": a2a.agent_id,
            "protocol": a2a.protocol,
            "action": "execute",
            "payload": self.value_to_json(inputs)?
        });

        // Make A2A communication request  
        let response = client
            .post(&a2a.endpoint)
            .json(&payload)
            .timeout(std::time::Duration::from_millis(a2a.timeout_ms))
            .send()
            .await
            .map_err(|e| RuntimeError::Generic(format!("A2A request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(RuntimeError::Generic(format!("A2A agent error: {}", response.status())));
        }

        let response_json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse A2A response: {}", e)))?;

        // Extract result from A2A response
        if let Some(result) = response_json.get("result") {
            Self::json_to_rtfs_value(result)
        } else if let Some(error) = response_json.get("error") {
            Err(RuntimeError::Generic(format!("A2A agent error: {}", error)))
        } else {
            // Return the full response if no specific result field
            Self::json_to_rtfs_value(&response_json)
        }
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
    pub async fn discover_from_agents(&self) -> Result<usize, RuntimeError> {
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

        if let ProviderType::Stream(stream_impl) = &capability.provider {
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

        if let ProviderType::Stream(stream_impl) = &capability.provider {
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

    pub fn register_executor<T: CapabilityExecutor + 'static>(&mut self, executor: Arc<T>) {
        self.executors.insert(executor.provider_type_id(), executor);
    }

    pub async fn execute_mcp_capability_static(mcp: &MCPCapability, inputs: &Value) -> RuntimeResult<Value> {
        // TODO: Implement MCP client integration
        Err(RuntimeError::Generic("MCP client integration not yet implemented (static stub)".to_string()))
    }
    pub async fn execute_a2a_capability_static(a2a: &A2ACapability, inputs: &Value) -> RuntimeResult<Value> {
        Err(RuntimeError::Generic("A2A capabilities not yet implemented (static stub)".to_string()))
    }
    pub async fn execute_plugin_capability_static(plugin: &PluginCapability, inputs: &Value) -> RuntimeResult<Value> {
        Err(RuntimeError::Generic("Plugin capabilities not yet implemented (static stub)".to_string()))
    }
    pub async fn execute_remote_rtfs_capability_static(remote_rtfs: &RemoteRTFSCapability, inputs: &Value) -> RuntimeResult<Value> {
        Err(RuntimeError::Generic("Remote RTFS capabilities not yet implemented (static stub)".to_string()))
    }
    pub async fn execute_stream_capability_static(
        stream_impl: &StreamCapabilityImpl,
        inputs: &Value,
    ) -> RuntimeResult<Value> {
        // For now, we just start the stream. The handle would need to be managed.
        // This is a simplification. A real implementation would need to return the handle
        // or manage the stream lifecycle.
        let handle = stream_impl.provider.start_stream(inputs)?;
        Ok(Value::String(format!("Stream started with ID: {}", handle.stream_id)))
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

#[async_trait(?Send)]
pub trait CapabilityExecutor: Send + Sync {
    fn provider_type_id(&self) -> TypeId;
    async fn execute(&self, provider: &ProviderType, inputs: &Value) -> RuntimeResult<Value>;
}

// Example executor for MCP
pub struct MCPExecutor;

#[async_trait(?Send)]
impl CapabilityExecutor for MCPExecutor {
    fn provider_type_id(&self) -> TypeId {
        TypeId::of::<MCPCapability>()
    }
    async fn execute(&self, provider: &ProviderType, inputs: &Value) -> RuntimeResult<Value> {
        if let ProviderType::MCP(mcp) = provider {
            // --- MCP Rust SDK Integration ---
            // Intended logic:
            // 1. Connect to MCP server at mcp.server_url (e.g., using WebSocket or stdio transport)
            // 2. Create an MCP client (using mcp_rust_sdk::client::Client or similar)
            // 3. Build a request to invoke the tool named mcp.tool_name with the provided inputs
            // 4. Send the request and await the response
            // 5. Parse the response and convert it to RTFS Value
            // 6. Return the result or error

            // Example (pseudo-code, replace with actual SDK usage):
            // use mcp_rust_sdk::client::Client;
            // let client = Client::connect(&mcp.server_url).await?;
            // let response = client.call_tool(&mcp.tool_name, Self::rtfs_value_to_json(inputs)?).await?;
            // let value = Self::json_to_rtfs_value(&response)?;
            // Ok(value)

            Err(RuntimeError::Generic("MCP capability execution not yet implemented. Integrate MCP Rust SDK here (see comments for intended logic).".to_string()))
        } else {
            Err(RuntimeError::Generic("ProviderType mismatch for MCPExecutor".to_string()))
        }
    }
}

// Repeat for A2A, Plugin, etc. (stubs for now)
pub struct A2AExecutor;
#[async_trait(?Send)]
impl CapabilityExecutor for A2AExecutor {
    fn provider_type_id(&self) -> TypeId {
        TypeId::of::<A2ACapability>()
    }
    async fn execute(&self, provider: &ProviderType, inputs: &Value) -> RuntimeResult<Value> {
        if let ProviderType::A2A(a2a) = provider {
            Err(RuntimeError::Generic("A2A capabilities not yet implemented (stub)".to_string()))
        } else {
            Err(RuntimeError::Generic("ProviderType mismatch for A2AExecutor".to_string()))
        }
    }
}

// Local capability executor
pub struct LocalExecutor;

#[async_trait(?Send)]
impl CapabilityExecutor for LocalExecutor {
    fn provider_type_id(&self) -> TypeId {
        TypeId::of::<LocalCapability>()
    }
    
    async fn execute(&self, provider: &ProviderType, inputs: &Value) -> RuntimeResult<Value> {
        if let ProviderType::Local(local) = provider {
            // Execute the local capability handler directly
            (local.handler)(inputs)
        } else {
            Err(RuntimeError::Generic("ProviderType mismatch for LocalExecutor".to_string()))
        }
    }
}

// HTTP capability executor  
pub struct HttpExecutor;

#[async_trait(?Send)]
impl CapabilityExecutor for HttpExecutor {
    fn provider_type_id(&self) -> TypeId {
        TypeId::of::<HttpCapability>()
    }
    
    async fn execute(&self, provider: &ProviderType, inputs: &Value) -> RuntimeResult<Value> {
        if let ProviderType::Http(http) = provider {
            // Execute HTTP capability using the existing logic
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
        } else {
            Err(RuntimeError::Generic("ProviderType mismatch for HttpExecutor".to_string()))
        }
    }
}

impl CapabilityMarketplace {
    /// Discover capabilities from network registry
    pub async fn discover_capabilities(
        &self,
        query: &str,
        limit: Option<usize>,
    ) -> Result<Vec<CapabilityManifest>, RuntimeError> {
        if let Some(registry_config) = &self.network_registry {
            self.discover_from_network(registry_config, query, limit).await
        } else {
            // Fallback to local discovery
            self.discover_local_capabilities(query, limit).await
        }
    }

    /// Discover capabilities from network registry
    async fn discover_from_network(
        &self,
        config: &NetworkRegistryConfig,
        query: &str,
        limit: Option<usize>,
    ) -> Result<Vec<CapabilityManifest>, RuntimeError> {
        use serde_json::json;

        let discovery_payload = json!({
            "query": query,
            "limit": limit.unwrap_or(10),
            "timestamp": Utc::now().to_rfc3339()
        });

        let client = reqwest::Client::new();
        let response = client
            .post(&config.endpoint)
            .json(&discovery_payload)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| RuntimeError::Generic(format!("Network discovery failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(RuntimeError::Generic(format!(
                "Discovery request failed with status: {}",
                response.status()
            )));
        }

        let discovery_response: serde_json::Value = response
            .json()
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse discovery response: {}", e)))?;

        // Parse and validate discovered capabilities
        self.parse_discovery_response(&discovery_response).await
    }

    /// Parse discovery response and validate capabilities
    async fn parse_discovery_response(
        &self,
        response: &serde_json::Value,
    ) -> Result<Vec<CapabilityManifest>, RuntimeError> {
        let capabilities_json = response
            .get("capabilities")
            .ok_or_else(|| RuntimeError::Generic("Missing 'capabilities' field in discovery response".to_string()))?;

        let mut capabilities = Vec::new();

        if let serde_json::Value::Array(cap_array) = capabilities_json {
            for cap_json in cap_array {
                match self.parse_capability_manifest(cap_json).await {
                    Ok(manifest) => {
                        // Verify attestation if present
                        if let Some(attestation) = &manifest.attestation {
                            if self.verify_capability_attestation(attestation, &manifest).await? {
                                capabilities.push(manifest);
                            }
                        } else {
                            capabilities.push(manifest);
                        }
                    }
                    Err(e) => {
                        // Log parsing error but continue with other capabilities
                        eprintln!("Failed to parse capability manifest: {}", e);
                    }
                }
            }
        }

        Ok(capabilities)
    }

    /// Parse individual capability manifest from JSON
    async fn parse_capability_manifest(
        &self,
        cap_json: &serde_json::Value,
    ) -> Result<CapabilityManifest, RuntimeError> {
        let id = cap_json
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RuntimeError::Generic("Missing capability id".to_string()))?
            .to_string();

        let name = cap_json
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RuntimeError::Generic("Missing capability name".to_string()))?
            .to_string();

        let description = cap_json
            .get("description")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RuntimeError::Generic("Missing capability description".to_string()))?
            .to_string();

        // For network-discovered capabilities, create a basic HTTP provider
        let endpoint = cap_json
            .get("endpoint")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let provider_type = if let Some(endpoint) = &endpoint {
            ProviderType::Http(HttpCapability {
                base_url: endpoint.clone(),
                auth_token: None,
                timeout_ms: 30000,
            })
        } else {
            return Err(RuntimeError::Generic("Network capabilities must have an endpoint".to_string()));
        };

        // Parse optional schema fields
        let input_schema = cap_json
            .get("input_schema")
            .and_then(|v| v.as_str())
            .map(|s| TypeExpr::from_str(s).unwrap());

        let output_schema = cap_json
            .get("output_schema")
            .and_then(|v| v.as_str())
            .map(|s| TypeExpr::from_str(s).unwrap());

        // Parse attestation if present
        let attestation = if let Some(att_json) = cap_json.get("attestation") {
            Some(self.parse_capability_attestation(att_json)?)
        } else {
            None
        };

        // Parse provenance if present
        let provenance = if let Some(prov_json) = cap_json.get("provenance") {
            Some(self.parse_capability_provenance(prov_json)?)
        } else {
            None
        };

        Ok(CapabilityManifest {
            id,
            name,
            description,
            provider: provider_type,
            version: "1.0.0".to_string(),
            input_schema,
            output_schema,
            attestation,
            provenance,
            permissions: vec![],
            metadata: HashMap::new(),
        })
    }

    /// Parse capability attestation from JSON
    fn parse_capability_attestation(
        &self,
        att_json: &serde_json::Value,
    ) -> Result<CapabilityAttestation, RuntimeError> {
        let authority = att_json
            .get("authority")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RuntimeError::Generic("Missing attestation authority".to_string()))?
            .to_string();

        let signature = att_json
            .get("signature")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RuntimeError::Generic("Missing attestation signature".to_string()))?
            .to_string();

        let timestamp_str = att_json
            .get("timestamp")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RuntimeError::Generic("Missing attestation timestamp".to_string()))?;

        let timestamp = DateTime::parse_from_rfc3339(timestamp_str)
            .map_err(|e| RuntimeError::Generic(format!("Invalid timestamp format: {}", e)))?
            .with_timezone(&Utc);

        let metadata = if let Some(claims_obj) = att_json.get("claims") {
            if let serde_json::Value::Object(claims_map) = claims_obj {
                claims_map
                    .iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            } else {
                HashMap::new()
            }
        } else {
            HashMap::new()
        };

        Ok(CapabilityAttestation {
            signature,
            authority,
            created_at: timestamp,
            expires_at: None,  // Could parse from JSON if present
            metadata,
        })
    }

    /// Parse capability provenance from JSON
    fn parse_capability_provenance(
        &self,
        prov_json: &serde_json::Value,
    ) -> Result<CapabilityProvenance, RuntimeError> {
        let source = prov_json
            .get("source")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RuntimeError::Generic("Missing provenance source".to_string()))?
            .to_string();

        let version = prov_json
            .get("version")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let content_hash = prov_json
            .get("content_hash")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RuntimeError::Generic("Missing provenance content_hash".to_string()))?
            .to_string();

        let custody_chain = if let Some(chain_json) = prov_json.get("custody_chain") {
            if let serde_json::Value::Array(chain_array) = chain_json {
                chain_array
                    .iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        let timestamp_str = prov_json
            .get("registered_at")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RuntimeError::Generic("Missing provenance timestamp".to_string()))?;

        let registered_at = DateTime::parse_from_rfc3339(timestamp_str)
            .map_err(|e| RuntimeError::Generic(format!("Invalid timestamp format: {}", e)))?
            .with_timezone(&Utc);

        Ok(CapabilityProvenance {
            source,
            version,
            content_hash,
            custody_chain,
            registered_at,
        })
    }

    /// Verify capability attestation
    async fn verify_capability_attestation(
        &self,
        attestation: &CapabilityAttestation,
        manifest: &CapabilityManifest,
    ) -> Result<bool, RuntimeError> {
        // Basic attestation verification - in production this would use cryptographic verification
        // For now, just verify the timestamp is recent and authority is not empty
        let now = Utc::now();
        let age = now.signed_duration_since(attestation.created_at);
        
        // Reject attestations older than 30 days
        if age.num_days() > 30 {
            return Ok(false);
        }

        // Verify authority is not empty
        if attestation.authority.is_empty() {
            return Ok(false);
        }

        // Verify content hash matches if present in provenance
        if let Some(provenance) = &manifest.provenance {
            let expected_hash = self.compute_content_hash(&format!("{}{}{}", 
                manifest.id, manifest.name, manifest.description));
            if provenance.content_hash != expected_hash {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Discover capabilities locally (fallback)
    async fn discover_local_capabilities(
        &self,
        query: &str,
        limit: Option<usize>,
    ) -> Result<Vec<CapabilityManifest>, RuntimeError> {
        let capabilities = self.capabilities.read().await;
        let query_lower = query.to_lowercase();
        
        let mut matches: Vec<CapabilityManifest> = capabilities
            .values()
            .filter(|cap| {
                cap.name.to_lowercase().contains(&query_lower) ||
                cap.description.to_lowercase().contains(&query_lower) ||
                cap.id.to_lowercase().contains(&query_lower)
            })
            .cloned()
            .collect();

        if let Some(limit) = limit {
            matches.truncate(limit);
        }

        Ok(matches)
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

    #[tokio::test]
    async fn test_register_and_execute_http_capability() {
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = CapabilityMarketplace::new(registry.clone());

        // Register a test HTTP capability (using httpbin.org for testing)
        marketplace.register_http_capability(
            "test.http_get".to_string(),
            "Test HTTP GET".to_string(),
            "A simple HTTP GET capability".to_string(),
            "https://httpbin.org/get".to_string(),
            None, // auth_token
        ).await.unwrap();

        let inputs = Value::String("test".to_string());
        let result = marketplace.execute_capability("test.http_get", &inputs).await;

        // The test should succeed (return Ok) even if we can't connect to httpbin.org
        // We're just testing that the execution doesn't panic or have compilation errors
        match result {
            Ok(_) => {
                // HTTP call succeeded - great!
                println!("HTTP capability executed successfully");
            }
            Err(_) => {
                // HTTP call failed (possibly network issues) - that's okay for this test
                // We're mainly testing that the code compiles and executes without panicking
                println!("HTTP capability failed (expected in some environments)");
            }
        }
    }
}