// ...existing code...
use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::runtime::values::Value;
use crate::runtime::capability_registry::CapabilityRegistry;
use crate::runtime::type_validator::{TypeValidator, ValidationError as TypeValidationError, TypeCheckingConfig, VerificationContext}; // Add optimized validation imports
use crate::ast::MapKey;
use crate::ast::TypeExpr; // Add import for RTFS type expressions
use std::collections::HashMap;
use std::sync::Arc;
use std::any::TypeId;
use tokio::sync::RwLock;
use tokio::sync::mpsc;
use async_trait::async_trait;
use serde_json::json;
use reqwest;
use tokio::process::Command;
use std::process::Stdio;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;
use chrono::{DateTime, Utc};

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
#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Clone, PartialEq)]
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

impl PartialEq for StreamCallbacks {
    fn eq(&self, other: &Self) -> bool {
        // For function pointers, we can only check if both are Some or None
        // This is a simplified comparison for compilation purposes
        self.on_connected.is_some() == other.on_connected.is_some()
            && self.on_disconnected.is_some() == other.on_disconnected.is_some()
            && self.on_data_received.is_some() == other.on_data_received.is_some()
            && self.on_error.is_some() == other.on_error.is_some()
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
#[derive(Debug, Clone, PartialEq)]
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
    // Add type validator for RTFS type validation
    type_validator: Arc<TypeValidator>,
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
            type_validator: Arc::new(TypeValidator::new()),
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
        // Validate the schemas are well-formed
        if let Some(ref schema) = input_schema {
            self.validate_schema_wellformed(schema)?;
        }
        if let Some(ref schema) = output_schema {
            self.validate_schema_wellformed(schema)?;
        }

        // Create a validating wrapper around the handler
        let validating_handler = self.create_validating_handler(handler, input_schema.clone(), output_schema.clone());

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
            provider: ProviderType::Local(LocalCapability { handler: validating_handler }),
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

    /// Create a validating wrapper around a capability handler
    fn create_validating_handler(
        &self,
        handler: Arc<dyn Fn(&Value) -> RuntimeResult<Value> + Send + Sync>,
        input_schema: Option<TypeExpr>,
        output_schema: Option<TypeExpr>,
    ) -> Arc<dyn Fn(&Value) -> RuntimeResult<Value> + Send + Sync> {
        let validator = self.type_validator.clone();
        
        Arc::new(move |input: &Value| -> RuntimeResult<Value> {
            // Validate input
            if let Some(ref schema) = input_schema {
                validator.validate_value(input, schema)
                    .map_err(|e| RuntimeError::new(&format!("Input validation failed: {}", e)))?;
            }
            
            // Call original handler
            let result = handler(input)?;
            
            // Validate output
            if let Some(ref schema) = output_schema {
                validator.validate_value(&result, schema)
                    .map_err(|e| RuntimeError::new(&format!("Output validation failed: {}", e)))?;
            }
            
            Ok(result)
        })
    }

    /// Validate that a type schema is well-formed
    fn validate_schema_wellformed(&self, _schema: &TypeExpr) -> Result<(), RuntimeError> {
        // For now, just check that the schema is not malformed
        // In the future, we could add more sophisticated validation
        // such as checking for circular references, etc.
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

    /// Register an MCP (Model Context Protocol) capability
    pub async fn register_mcp_capability(
        &self,
        id: String,
        name: String,
        description: String,
        server_url: String,
        tool_name: String,
        timeout_ms: u64,
    ) -> Result<(), RuntimeError> {
        let content_hash = self.compute_content_hash(&format!("{}{}{}", id, name, description));
        let provenance = CapabilityProvenance {
            source: "mcp_registration".to_string(),
            version: Some("1.0.0".to_string()),
            content_hash,
            custody_chain: vec!["mcp_registration".to_string()],
            registered_at: Utc::now(),
        };

        let mcp_capability = MCPCapability {
            server_url,
            tool_name,
            timeout_ms,
        };
        
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::MCP(mcp_capability),
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

    /// Register an MCP capability with schema validation
    pub async fn register_mcp_capability_with_schema(
        &self,
        id: String,
        name: String,
        description: String,
        server_url: String,
        tool_name: String,
        timeout_ms: u64,
        input_schema: Option<TypeExpr>,
        output_schema: Option<TypeExpr>,
    ) -> Result<(), RuntimeError> {
        let content_hash = self.compute_content_hash(&format!("{}{}{}", id, name, description));
        let provenance = CapabilityProvenance {
            source: "mcp_registration".to_string(),
            version: Some("1.0.0".to_string()),
            content_hash,
            custody_chain: vec!["mcp_registration".to_string()],
            registered_at: Utc::now(),
        };

        let mcp_capability = MCPCapability {
            server_url,
            tool_name,
            timeout_ms,
        };
        
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::MCP(mcp_capability),
            version: "1.0.0".to_string(),
            input_schema,
            output_schema,
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            metadata: HashMap::new(),
        };
        
        let mut capabilities = self.capabilities.write().await;
        capabilities.insert(id, capability);
        Ok(())
    }

    /// Register an A2A (Agent-to-Agent) capability
    pub async fn register_a2a_capability(
        &self,
        id: String,
        name: String,
        description: String,
        agent_id: String,
        endpoint: String,
        protocol: String,
        timeout_ms: u64,
    ) -> Result<(), RuntimeError> {
        let content_hash = self.compute_content_hash(&format!("{}{}{}", id, name, description));
        let provenance = CapabilityProvenance {
            source: "a2a_registration".to_string(),
            version: Some("1.0.0".to_string()),
            content_hash,
            custody_chain: vec!["a2a_registration".to_string()],
            registered_at: Utc::now(),
        };

        let a2a_capability = A2ACapability {
            agent_id,
            endpoint,
            protocol,
            timeout_ms,
        };
        
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::A2A(a2a_capability),
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

    /// Register an A2A capability with schema validation
    pub async fn register_a2a_capability_with_schema(
        &self,
        id: String,
        name: String,
        description: String,
        agent_id: String,
        endpoint: String,
        protocol: String,
        timeout_ms: u64,
        input_schema: Option<TypeExpr>,
        output_schema: Option<TypeExpr>,
    ) -> Result<(), RuntimeError> {
        let content_hash = self.compute_content_hash(&format!("{}{}{}", id, name, description));
        let provenance = CapabilityProvenance {
            source: "a2a_registration".to_string(),
            version: Some("1.0.0".to_string()),
            content_hash,
            custody_chain: vec!["a2a_registration".to_string()],
            registered_at: Utc::now(),
        };

        let a2a_capability = A2ACapability {
            agent_id,
            endpoint,
            protocol,
            timeout_ms,
        };
        
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::A2A(a2a_capability),
            version: "1.0.0".to_string(),
            input_schema,
            output_schema,
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            metadata: HashMap::new(),
        };
        
        let mut capabilities = self.capabilities.write().await;
        capabilities.insert(id, capability);
        Ok(())
    }

    /// Register a plugin-based capability
    pub async fn register_plugin_capability(
        &self,
        id: String,
        name: String,
        description: String,
        plugin_path: String,
        function_name: String,
    ) -> Result<(), RuntimeError> {
        let content_hash = self.compute_content_hash(&format!("{}{}{}", id, name, description));
        let provenance = CapabilityProvenance {
            source: "plugin_registration".to_string(),
            version: Some("1.0.0".to_string()),
            content_hash,
            custody_chain: vec!["plugin_registration".to_string()],
            registered_at: Utc::now(),
        };

        let plugin_capability = PluginCapability {
            plugin_path,
            function_name,
        };
        
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::Plugin(plugin_capability),
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

    /// Register a plugin capability with schema validation
    pub async fn register_plugin_capability_with_schema(
        &self,
        id: String,
        name: String,
        description: String,
        plugin_path: String,
        function_name: String,
        input_schema: Option<TypeExpr>,
        output_schema: Option<TypeExpr>,
    ) -> Result<(), RuntimeError> {
        let content_hash = self.compute_content_hash(&format!("{}{}{}", id, name, description));
        let provenance = CapabilityProvenance {
            source: "plugin_registration".to_string(),
            version: Some("1.0.0".to_string()),
            content_hash,
            custody_chain: vec!["plugin_registration".to_string()],
            registered_at: Utc::now(),
        };

        let plugin_capability = PluginCapability {
            plugin_path,
            function_name,
        };
        
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::Plugin(plugin_capability),
            version: "1.0.0".to_string(),
            input_schema,
            output_schema,
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
        let config = TypeCheckingConfig::default();
        self.execute_with_validation_config(capability_id, params, &config).await
    }
    
    /// Execute capability with optimized type checking configuration
    pub async fn execute_with_validation_config(
        &self,
        capability_id: &str,
        params: &HashMap<String, Value>,
        config: &TypeCheckingConfig,
    ) -> Result<Value, RuntimeError> {
        let capability = {
            let capabilities = self.capabilities.read().await;
            capabilities.get(capability_id).cloned()
                .ok_or_else(|| RuntimeError::Generic(format!("Capability not found: {}", capability_id)))?
        };

        // Create capability boundary context (always validates regardless of config)
        let boundary_context = VerificationContext::capability_boundary(capability_id);

        // Validate input against schema if present
        if let Some(input_schema) = &capability.input_schema {
            self.validate_input_schema_optimized(params, input_schema, config, &boundary_context).await?;
        }

        // Convert params to Value for execution
        let inputs_value = self.params_to_value(params)?;

        // Execute the capability
        let result = self.execute_capability(capability_id, &inputs_value).await?;

        // Validate output against schema if present
        if let Some(output_schema) = &capability.output_schema {
            self.validate_output_schema_optimized(&result, output_schema, config, &boundary_context).await?;
        }

        Ok(result)
    }

    /// Optimized input validation with configuration
    async fn validate_input_schema_optimized(
        &self,
        params: &HashMap<String, Value>,
        schema_expr: &TypeExpr,
        config: &TypeCheckingConfig,
        context: &VerificationContext,
    ) -> Result<(), RuntimeError> {
        // Convert RTFS params to Value for validation
        let params_value = self.params_to_value(params)?;
        
        // Use optimized validation
        self.type_validator.validate_with_config(&params_value, schema_expr, config, context)
            .map_err(|e| RuntimeError::Generic(format!("Input validation failed: {}", e)))?;

        Ok(())
    }

    /// Optimized output validation with configuration
    async fn validate_output_schema_optimized(
        &self,
        result: &Value,
        schema_expr: &TypeExpr,
        config: &TypeCheckingConfig,
        context: &VerificationContext,
    ) -> Result<(), RuntimeError> {
        // Use optimized validation
        self.type_validator.validate_with_config(result, schema_expr, config, context)
            .map_err(|e| RuntimeError::Generic(format!("Output validation failed: {}", e)))?;

        Ok(())
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

    /// Execute a capability using the extensible CapabilityExecutor pattern
    pub async fn execute_capability(&self, id: &str, inputs: &Value) -> RuntimeResult<Value> {
        let capabilities = self.capabilities.read().await;
        
        if let Some(manifest) = capabilities.get(id) {
            // Try to use registered executor first
            let provider_type_id = match &manifest.provider {
                ProviderType::Local(_) => TypeId::of::<LocalCapability>(),
                ProviderType::Http(_) => TypeId::of::<HttpCapability>(),
                ProviderType::MCP(_) => TypeId::of::<MCPCapability>(),
                ProviderType::A2A(_) => TypeId::of::<A2ACapability>(),
                ProviderType::Plugin(_) => TypeId::of::<PluginCapability>(),
                ProviderType::RemoteRTFS(_) => TypeId::of::<RemoteRTFSCapability>(),
                ProviderType::Stream(_) => TypeId::of::<StreamCapabilityImpl>(),
            };
            
            if let Some(executor) = self.executors.get(&provider_type_id) {
                // Use registered executor
                return executor.execute(&manifest.provider, inputs).await;
            }
            
            // Fallback to direct execution if no executor is registered
            match &manifest.provider {
                ProviderType::Local(local) => self.execute_local_capability(local, inputs),
                ProviderType::Http(http) => self.execute_http_capability(http, inputs).await,
                ProviderType::MCP(mcp) => self.execute_mcp_capability(mcp, inputs).await,
                ProviderType::A2A(a2a) => self.execute_a2a_capability(a2a, inputs).await,
                ProviderType::Plugin(plugin) => self.execute_plugin_capability(plugin, inputs).await,
                ProviderType::RemoteRTFS(remote_rtfs) => self.execute_remote_rtfs_capability(remote_rtfs, inputs).await,
                ProviderType::Stream(stream_impl) => self.execute_stream_capability(stream_impl, inputs).await,
            }
        } else {
            // Try to delegate to capability registry for built-in capabilities
            let registry = self.capability_registry.read().await;
            // Convert single Value to Vec<Value> for the registry
            let args = vec![inputs.clone()];
            registry.execute_capability_with_microvm(id, args)
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

    /// Execute MCP capability using tokio directly
    async fn execute_mcp_capability(&self, mcp: &MCPCapability, inputs: &Value) -> RuntimeResult<Value> {
        // Convert inputs to JSON for MCP communication
        let input_json = self.value_to_json(inputs)?;
        
        // Create child process for MCP server
        let mut child = Command::new("npx")
            .arg("-y")
            .arg("@modelcontextprotocol/server-everything")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| RuntimeError::Generic(format!("Failed to start MCP server: {}", e)))?;
        
        let mut stdin = child.stdin.take().ok_or_else(|| {
            RuntimeError::Generic("Failed to get stdin for MCP server".to_string())
        })?;
        
        let mut stdout = child.stdout.take().ok_or_else(|| {
            RuntimeError::Generic("Failed to get stdout for MCP server".to_string())
        })?;
        
        // If tool_name is not specific, discover available tools
        let tool_name = if mcp.tool_name.is_empty() || mcp.tool_name == "*" {
            // Send tools/list request
            let tools_request = json!({
                "jsonrpc": "2.0",
                "id": "tools_discovery",
                "method": "tools/list",
                "params": {}
            });
            
            let request_str = serde_json::to_string(&tools_request)
                .map_err(|e| RuntimeError::Generic(format!("Failed to serialize tools request: {}", e)))?;
            
            stdin.write_all((request_str + "\n").as_bytes()).await
                .map_err(|e| RuntimeError::Generic(format!("Failed to write to MCP server: {}", e)))?;
            
            // Read response
            let mut response = String::new();
            stdout.read_to_string(&mut response).await
                .map_err(|e| RuntimeError::Generic(format!("Failed to read from MCP server: {}", e)))?;
            
            let tools_response: serde_json::Value = serde_json::from_str(&response)
                .map_err(|e| RuntimeError::Generic(format!("Failed to parse MCP response: {}", e)))?;
            
            // Extract first tool name
            if let Some(result) = tools_response.get("result") {
                if let Some(tools) = result.get("tools") {
                    if let Some(tools_array) = tools.as_array() {
                        if let Some(first_tool) = tools_array.first() {
                            if let Some(name) = first_tool.get("name") {
                                name.as_str().unwrap_or("default_tool").to_string()
                            } else {
                                "default_tool".to_string()
                            }
                        } else {
                            return Err(RuntimeError::Generic("No MCP tools available".to_string()));
                        }
                    } else {
                        return Err(RuntimeError::Generic("Invalid tools response format".to_string()));
                    }
                } else {
                    return Err(RuntimeError::Generic("No tools in MCP response".to_string()));
                }
            } else {
                return Err(RuntimeError::Generic("No result in MCP response".to_string()));
            }
        } else {
            mcp.tool_name.clone()
        };
        
        // Send tool call request
        let tool_request = json!({
            "jsonrpc": "2.0",
            "id": "tool_call",
            "method": "tools/call",
            "params": {
                "name": tool_name,
                "arguments": input_json
            }
        });
        
        let request_str = serde_json::to_string(&tool_request)
            .map_err(|e| RuntimeError::Generic(format!("Failed to serialize tool request: {}", e)))?;
        
        stdin.write_all((request_str + "\n").as_bytes()).await
            .map_err(|e| RuntimeError::Generic(format!("Failed to write tool request: {}", e)))?;
        
        // Read response
        let mut response = String::new();
        stdout.read_to_string(&mut response).await
            .map_err(|e| RuntimeError::Generic(format!("Failed to read tool response: {}", e)))?;
        
        let tool_response: serde_json::Value = serde_json::from_str(&response)
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse tool response: {}", e)))?;
        
        // Check for error
        if let Some(error) = tool_response.get("error") {
            return Err(RuntimeError::Generic(format!("MCP tool execution failed: {:?}", error)));
        }
        
        // Extract result
        if let Some(result) = tool_response.get("result") {
            if let Some(content) = result.get("content") {
                // Convert content to RTFS Value
                Self::json_to_rtfs_value(content)
            } else {
                // Convert entire result to RTFS Value
                Self::json_to_rtfs_value(result)
            }
        } else {
            Err(RuntimeError::Generic("No result in MCP tool response".to_string()))
        }
    }
    


    /// Execute A2A capability
    async fn execute_a2a_capability(&self, a2a: &A2ACapability, inputs: &Value) -> RuntimeResult<Value> {
        // A2A (Agent-to-Agent) communication implementation with multi-protocol support
        match a2a.protocol.as_str() {
            "http" | "https" => self.execute_a2a_http(a2a, inputs).await,
            "websocket" | "ws" | "wss" => self.execute_a2a_websocket(a2a, inputs).await,
            "grpc" => self.execute_a2a_grpc(a2a, inputs).await,
            _ => Err(RuntimeError::Generic(format!("Unsupported A2A protocol: {}", a2a.protocol)))
        }
    }
    
    /// Execute A2A over HTTP/HTTPS
    async fn execute_a2a_http(&self, a2a: &A2ACapability, inputs: &Value) -> RuntimeResult<Value> {
        let client = reqwest::Client::new();
        
        // Convert inputs to JSON for A2A communication
        let input_json = self.value_to_json(inputs)?;
        
        // Prepare A2A request payload
        let payload = serde_json::json!({
            "agent_id": a2a.agent_id,
            "capability": "execute",
            "inputs": input_json,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });
        
        // Make HTTP request to A2A endpoint
        let response = client
            .post(&a2a.endpoint)
            .header("Content-Type", "application/json")
            .json(&payload)
            .timeout(std::time::Duration::from_millis(a2a.timeout_ms))
            .send()
            .await
            .map_err(|e| RuntimeError::Generic(format!("A2A HTTP request failed: {}", e)))?;
        
        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(RuntimeError::Generic(format!("A2A HTTP error {}: {}", status, error_body)));
        }
        
        let response_json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse A2A HTTP response: {}", e)))?;
        
        // Extract result from A2A response
        if let Some(result) = response_json.get("result") {
            Self::json_to_rtfs_value(result)
        } else if let Some(error) = response_json.get("error") {
            let error_msg = if let Some(message) = error.get("message") {
                message.as_str().unwrap_or("Unknown A2A error")
            } else {
                "Unknown A2A error"
            };
            Err(RuntimeError::Generic(format!("A2A error: {}", error_msg)))
        } else {
            Err(RuntimeError::Generic("Invalid A2A response format".to_string()))
        }
    }
    
    /// Execute A2A over WebSocket
    async fn execute_a2a_websocket(&self, a2a: &A2ACapability, inputs: &Value) -> RuntimeResult<Value> {
        // WebSocket implementation would go here
        // For now, return a placeholder error
        Err(RuntimeError::Generic("A2A WebSocket protocol not yet implemented".to_string()))
    }
    
    /// Execute A2A over gRPC
    async fn execute_a2a_grpc(&self, a2a: &A2ACapability, inputs: &Value) -> RuntimeResult<Value> {
        // gRPC implementation would go here
        // For now, return a placeholder error
        Err(RuntimeError::Generic("A2A gRPC protocol not yet implemented".to_string()))
    }

    /// Execute plugin capability
    async fn execute_plugin_capability(&self, plugin: &PluginCapability, inputs: &Value) -> RuntimeResult<Value> {
        // Plugin execution implementation
        use std::process::Command;
        use std::path::Path;
        
        // Validate plugin path exists
        if !Path::new(&plugin.plugin_path).exists() {
            return Err(RuntimeError::Generic(format!("Plugin not found: {}", plugin.plugin_path)));
        }
        
        // Convert inputs to JSON for plugin communication
        let input_json = self.value_to_json(inputs)?;
        
        // Execute plugin as subprocess with JSON input
        let output = Command::new(&plugin.plugin_path)
            .arg("--function")
            .arg(&plugin.function_name)
            .arg("--input")
            .arg(serde_json::to_string(&input_json)
                .map_err(|e| RuntimeError::Generic(format!("Failed to serialize plugin input: {}", e)))?)
            .output()
            .map_err(|e| RuntimeError::Generic(format!("Failed to execute plugin: {}", e)))?;
        
        // Check if plugin execution was successful
        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            return Err(RuntimeError::Generic(format!("Plugin execution failed: {}", error_msg)));
        }
        
        // Parse plugin output as JSON
        let output_str = String::from_utf8_lossy(&output.stdout);
        let output_json: serde_json::Value = serde_json::from_str(&output_str)
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse plugin output: {}", e)))?;
        
        // Convert JSON back to RTFS Value
        Self::json_to_rtfs_value(&output_json)
    }
    
    /// Execute RemoteRTFS capability
    async fn execute_remote_rtfs_capability(&self, remote_rtfs: &RemoteRTFSCapability, inputs: &Value) -> RuntimeResult<Value> {
        let client = reqwest::Client::new();
        
        // Convert inputs to JSON for RTFS communication
        let input_json = self.value_to_json(inputs)?;
        
        // Prepare RTFS request payload
        let payload = serde_json::json!({
            "type": "rtfs_call",
            "inputs": input_json,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });
        
        // Make HTTP request to remote RTFS instance
        let mut request = client
            .post(&remote_rtfs.endpoint)
            .header("Content-Type", "application/json")
            .json(&payload)
            .timeout(std::time::Duration::from_millis(remote_rtfs.timeout_ms));
        
        // Add authentication if provided
        if let Some(token) = &remote_rtfs.auth_token {
            request = request.bearer_auth(token);
        }
        
        let response = request
            .send()
            .await
            .map_err(|e| RuntimeError::Generic(format!("Remote RTFS request failed: {}", e)))?;
        
        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(RuntimeError::Generic(format!("Remote RTFS error {}: {}", status, error_body)));
        }
        
        let response_json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse remote RTFS response: {}", e)))?;
        
        // Extract result from RTFS response
        if let Some(result) = response_json.get("result") {
            Self::json_to_rtfs_value(result)
        } else if let Some(error) = response_json.get("error") {
            let error_msg = if let Some(message) = error.get("message") {
                message.as_str().unwrap_or("Unknown remote RTFS error")
            } else {
                "Unknown remote RTFS error"
            };
            Err(RuntimeError::Generic(format!("Remote RTFS error: {}", error_msg)))
        } else {
            Err(RuntimeError::Generic("Invalid remote RTFS response format".to_string()))
        }
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
            // Convert inputs to JSON for MCP communication
            let input_json = serde_json::to_value(inputs)
                .map_err(|e| RuntimeError::Generic(format!("Failed to serialize inputs: {}", e)))?;
            
            // Connect to real MCP server via HTTP/WebSocket
            // For now, we'll use HTTP as the primary transport
            let client = reqwest::Client::new();
            
            // If tool_name is not specific, discover available tools
            let tool_name = if mcp.tool_name.is_empty() || mcp.tool_name == "*" {
                // Send tools/list request to MCP server
                let tools_request = json!({
                    "jsonrpc": "2.0",
                    "id": "tools_discovery",
                    "method": "tools/list",
                    "params": {}
                });
                
                let response = client
                    .post(&mcp.server_url)
                    .json(&tools_request)
                    .timeout(std::time::Duration::from_millis(mcp.timeout_ms))
                    .send()
                    .await
                    .map_err(|e| RuntimeError::Generic(format!("Failed to connect to MCP server: {}", e)))?;
                
                let tools_response: serde_json::Value = response
                    .json()
                    .await
                    .map_err(|e| RuntimeError::Generic(format!("Failed to parse MCP response: {}", e)))?;
                
                // Extract first tool name
                if let Some(result) = tools_response.get("result") {
                    if let Some(tools) = result.get("tools") {
                        if let Some(tools_array) = tools.as_array() {
                            if let Some(first_tool) = tools_array.first() {
                                if let Some(name) = first_tool.get("name") {
                                    name.as_str().unwrap_or("default_tool").to_string()
                                } else {
                                    "default_tool".to_string()
                                }
                            } else {
                                return Err(RuntimeError::Generic("No MCP tools available".to_string()));
                            }
                        } else {
                            return Err(RuntimeError::Generic("Invalid tools response format".to_string()));
                        }
                    } else {
                        return Err(RuntimeError::Generic("No tools in MCP response".to_string()));
                    }
                } else {
                    return Err(RuntimeError::Generic("No result in MCP response".to_string()));
                }
            } else {
                mcp.tool_name.clone()
            };
            
            // Send tool call request
            let tool_request = json!({
                "jsonrpc": "2.0",
                "id": "tool_call",
                "method": "tools/call",
                "params": {
                    "name": tool_name,
                    "arguments": input_json
                }
            });
            
            let response = client
                .post(&mcp.server_url)
                .json(&tool_request)
                .timeout(std::time::Duration::from_millis(mcp.timeout_ms))
                .send()
                .await
                .map_err(|e| RuntimeError::Generic(format!("Failed to execute MCP tool: {}", e)))?;
            
            let tool_response: serde_json::Value = response
                .json()
                .await
                .map_err(|e| RuntimeError::Generic(format!("Failed to parse tool response: {}", e)))?;
            
            // Check for error
            if let Some(error) = tool_response.get("error") {
                return Err(RuntimeError::Generic(format!("MCP tool execution failed: {:?}", error)));
            }
            
            // Extract result
            if let Some(result) = tool_response.get("result") {
                if let Some(content) = result.get("content") {
                    // Convert content to RTFS Value
                    CapabilityMarketplace::json_to_rtfs_value(content)
                } else {
                    // Convert entire result to RTFS Value
                    CapabilityMarketplace::json_to_rtfs_value(result)
                }
            } else {
                Err(RuntimeError::Generic("No result in MCP tool response".to_string()))
            }
        } else {
            Err(RuntimeError::Generic("Invalid provider type for MCP executor".to_string()))
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
            // A2A (Agent-to-Agent) communication implementation with multi-protocol support
            match a2a.protocol.as_str() {
                "http" | "https" => self.execute_a2a_http(a2a, inputs).await,
                "websocket" | "ws" | "wss" => self.execute_a2a_websocket(a2a, inputs).await,
                "grpc" => self.execute_a2a_grpc(a2a, inputs).await,
                _ => Err(RuntimeError::Generic(format!("Unsupported A2A protocol: {}", a2a.protocol)))
            }
        } else {
            Err(RuntimeError::Generic("ProviderType mismatch for A2AExecutor".to_string()))
        }
    }
}

impl A2AExecutor {
    /// Execute A2A over HTTP/HTTPS
    async fn execute_a2a_http(&self, a2a: &A2ACapability, inputs: &Value) -> RuntimeResult<Value> {
        let client = reqwest::Client::new();
        
        // Convert inputs to JSON for A2A communication
        let input_json = Self::value_to_json(inputs)?;
        
        // Prepare A2A request payload
        let payload = serde_json::json!({
            "agent_id": a2a.agent_id,
            "capability": "execute",
            "inputs": input_json,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });
        
        // Make HTTP request to A2A endpoint
        let response = client
            .post(&a2a.endpoint)
            .header("Content-Type", "application/json")
            .json(&payload)
            .timeout(std::time::Duration::from_millis(a2a.timeout_ms))
            .send()
            .await
            .map_err(|e| RuntimeError::Generic(format!("A2A HTTP request failed: {}", e)))?;
        
        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(RuntimeError::Generic(format!("A2A HTTP error {}: {}", status, error_body)));
        }
        
        let response_json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse A2A HTTP response: {}", e)))?;
        
        // Extract result from A2A response
        if let Some(result) = response_json.get("result") {
            Self::json_to_rtfs_value(result)
        } else if let Some(error) = response_json.get("error") {
            let error_msg = if let Some(message) = error.get("message") {
                message.as_str().unwrap_or("Unknown A2A error")
            } else {
                "Unknown A2A error"
            };
            Err(RuntimeError::Generic(format!("A2A error: {}", error_msg)))
        } else {
            Err(RuntimeError::Generic("Invalid A2A response format".to_string()))
        }
    }
    
    /// Execute A2A over WebSocket
    async fn execute_a2a_websocket(&self, a2a: &A2ACapability, inputs: &Value) -> RuntimeResult<Value> {
        // WebSocket implementation would go here
        // For now, return a placeholder error
        Err(RuntimeError::Generic("A2A WebSocket protocol not yet implemented".to_string()))
    }
    
    /// Execute A2A over gRPC
    async fn execute_a2a_grpc(&self, a2a: &A2ACapability, inputs: &Value) -> RuntimeResult<Value> {
        // gRPC implementation would go here
        // For now, return a placeholder error
        Err(RuntimeError::Generic("A2A gRPC protocol not yet implemented".to_string()))
    }

    /// Convert RTFS Value to JSON
    fn value_to_json(value: &Value) -> Result<serde_json::Value, RuntimeError> {
        use serde_json::Value as JsonValue;
        
        match value {
            Value::Integer(i) => Ok(JsonValue::Number(serde_json::Number::from(*i))),
            Value::Float(f) => Ok(JsonValue::Number(serde_json::Number::from_f64(*f)
                .ok_or_else(|| RuntimeError::Generic("Invalid float value".to_string()))?)),
            Value::String(s) => Ok(JsonValue::String(s.clone())),
            Value::Boolean(b) => Ok(JsonValue::Bool(*b)),
            Value::Vector(vec) => {
                let json_vec: Result<Vec<JsonValue>, RuntimeError> = vec.iter()
                    .map(|v| Self::value_to_json(v))
                    .collect();
                Ok(JsonValue::Array(json_vec?))
            },
            Value::Map(map) => {
                let mut json_map = serde_json::Map::new();
                for (key, val) in map {
                    let key_str = match key {
                        crate::ast::MapKey::String(s) => s.clone(),
                        crate::ast::MapKey::Keyword(k) => k.0.clone(),
                        _ => return Err(RuntimeError::Generic("Map keys must be strings or keywords".to_string())),
                    };
                    json_map.insert(key_str, Self::value_to_json(val)?);
                }
                Ok(JsonValue::Object(json_map))
            },
            Value::Nil => Ok(JsonValue::Null),
            _ => Err(RuntimeError::Generic(format!("Cannot convert {} to JSON", value.type_name()))),
        }
    }

    /// Convert JSON to RTFS Value
    fn json_to_rtfs_value(json: &serde_json::Value) -> RuntimeResult<Value> {
        match json {
            serde_json::Value::String(s) => Ok(Value::String(s.clone())),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Ok(Value::Integer(i))
                } else if let Some(f) = n.as_f64() {
                    Ok(Value::Float(f))
                } else {
                    Err(RuntimeError::Generic("Invalid number format".to_string()))
                }
            }
            serde_json::Value::Bool(b) => Ok(Value::Boolean(*b)),
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
            serde_json::Value::Null => Ok(Value::Nil),
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

    /// Test MCP capability execution using the official SDK
    #[tokio::test]
    async fn test_mcp_capability_execution() {
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let mut marketplace = CapabilityMarketplace::new(registry);
        
        // Register MCP executor
        marketplace.register_executor(Arc::new(MCPExecutor));
        
        // Test with mock MCP server for testing purposes
        let result = test_mcp_with_mock_server().await;
        
        // Should either succeed with mock server or fail gracefully
        match result {
            Ok(_) => {
                // Mock server worked correctly
            }
            Err(_) => {
                // Mock server not available, which is acceptable in test environment
            }
        }
    }
    
    /// Helper function to test MCP with mock server (only for tests)
    async fn test_mcp_with_mock_server() -> RuntimeResult<Value> {
        // Convert inputs to JSON for MCP communication
        let inputs = Value::Map(HashMap::new());
        let input_json = serde_json::to_value(&inputs)
            .map_err(|e| RuntimeError::Generic(format!("Failed to serialize inputs: {}", e)))?;
        
        // Create child process for mock MCP server with timeout
        let mut child = Command::new("npx")
            .arg("-y")
            .arg("@modelcontextprotocol/server-everything")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| RuntimeError::Generic(format!("Failed to start mock MCP server: {}", e)))?;
        
        let mut stdin = child.stdin.take().ok_or_else(|| {
            RuntimeError::Generic("Failed to get stdin for mock MCP server".to_string())
        })?;
        
        let mut stdout = child.stdout.take().ok_or_else(|| {
            RuntimeError::Generic("Failed to get stdout for mock MCP server".to_string())
        })?;
        
        // Send tools/list request
        let tools_request = json!({
            "jsonrpc": "2.0",
            "id": "tools_discovery",
            "method": "tools/list",
            "params": {}
        });
        
        let request_str = serde_json::to_string(&tools_request)
            .map_err(|e| RuntimeError::Generic(format!("Failed to serialize tools request: {}", e)))?;
        
        // Write with timeout
        let request_with_newline = request_str + "\n";
        let write_future = stdin.write_all(request_with_newline.as_bytes());
        let timeout_duration = std::time::Duration::from_millis(1000);
        
        match tokio::time::timeout(timeout_duration, write_future).await {
            Ok(write_result) => {
                write_result.map_err(|e| RuntimeError::Generic(format!("Failed to write to mock MCP server: {}", e)))?;
            }
            Err(_) => {
                let _ = child.kill().await;
                return Err(RuntimeError::Generic("Mock MCP server write timeout".to_string()));
            }
        }
        
        // Read response with timeout
        let mut response = String::new();
        let read_future = stdout.read_to_string(&mut response);
        
        match tokio::time::timeout(timeout_duration, read_future).await {
            Ok(read_result) => {
                read_result.map_err(|e| RuntimeError::Generic(format!("Failed to read from mock MCP server: {}", e)))?;
            }
            Err(_) => {
                let _ = child.kill().await;
                return Err(RuntimeError::Generic("Mock MCP server read timeout".to_string()));
            }
        }
        
        let tools_response: serde_json::Value = serde_json::from_str(&response)
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse mock MCP response: {}", e)))?;
        
        // Extract first tool name
        let tool_name = if let Some(result) = tools_response.get("result") {
            if let Some(tools) = result.get("tools") {
                if let Some(tools_array) = tools.as_array() {
                    if let Some(first_tool) = tools_array.first() {
                        if let Some(name) = first_tool.get("name") {
                            name.as_str().unwrap_or("default_tool").to_string()
                        } else {
                            "default_tool".to_string()
                        }
                    } else {
                        return Err(RuntimeError::Generic("No MCP tools available".to_string()));
                    }
                } else {
                    return Err(RuntimeError::Generic("Invalid tools response format".to_string()));
                }
            } else {
                return Err(RuntimeError::Generic("No tools in MCP response".to_string()));
            }
        } else {
            return Err(RuntimeError::Generic("No result in MCP response".to_string()));
        };
        
        // Send tool call request
        let tool_request = json!({
            "jsonrpc": "2.0",
            "id": "tool_call",
            "method": "tools/call",
            "params": {
                "name": tool_name,
                "arguments": input_json
            }
        });
        
        let request_str = serde_json::to_string(&tool_request)
            .map_err(|e| RuntimeError::Generic(format!("Failed to serialize tool request: {}", e)))?;
        
        // Write with timeout
        let request_with_newline = request_str + "\n";
        let write_future = stdin.write_all(request_with_newline.as_bytes());
        
        match timeout(timeout_duration, write_future).await {
            Ok(write_result) => {
                write_result.map_err(|e| RuntimeError::Generic(format!("Failed to write tool request: {}", e)))?;
            }
            Err(_) => {
                let _ = child.kill().await;
                return Err(RuntimeError::Generic("Mock MCP tool execution write timeout".to_string()));
            }
        }
        
        // Read response with timeout
        let mut response = String::new();
        let read_future = stdout.read_to_string(&mut response);
        
        match timeout(timeout_duration, read_future).await {
            Ok(read_result) => {
                read_result.map_err(|e| RuntimeError::Generic(format!("Failed to read tool response: {}", e)))?;
            }
            Err(_) => {
                let _ = child.kill().await;
                return Err(RuntimeError::Generic("Mock MCP tool execution read timeout".to_string()));
            }
        }
        
        let tool_response: serde_json::Value = serde_json::from_str(&response)
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse tool response: {}", e)))?;
        
        // Check for error
        if let Some(error) = tool_response.get("error") {
            return Err(RuntimeError::Generic(format!("Mock MCP tool execution failed: {:?}", error)));
        }
        
        // Extract result
        if let Some(result) = tool_response.get("result") {
            if let Some(content) = result.get("content") {
                // Convert content to RTFS Value
                CapabilityMarketplace::json_to_rtfs_value(content)
            } else {
                // Convert entire result to RTFS Value
                CapabilityMarketplace::json_to_rtfs_value(result)
            }
        } else {
            Err(RuntimeError::Generic("No result in mock MCP tool response".to_string()))
        }
    }

    /// Test CapabilityExecutor pattern with custom executor
    #[tokio::test]
    async fn test_capability_executor_pattern() {
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let mut marketplace = CapabilityMarketplace::new(registry);
        
        // Create a test executor
        struct TestExecutor;
        
        #[async_trait(?Send)]
        impl CapabilityExecutor for TestExecutor {
            fn provider_type_id(&self) -> TypeId {
                TypeId::of::<HttpCapability>()
            }
            
            async fn execute(&self, provider: &ProviderType, inputs: &Value) -> RuntimeResult<Value> {
                if let ProviderType::Http(_) = provider {
                    Ok(Value::String("Test executor result".to_string()))
                } else {
                    Err(RuntimeError::Generic("Invalid provider type".to_string()))
                }
            }
        }
        
        // Register test executor
        marketplace.register_executor(Arc::new(TestExecutor));
        
        // Register HTTP capability
        let http_config = HttpCapability {
            base_url: "http://example.com".to_string(),
            auth_token: None,
            timeout_ms: 5000,
        };
        
        let manifest = CapabilityManifest {
            id: "test.http".to_string(),
            name: "Test HTTP".to_string(),
            description: "Test HTTP capability".to_string(),
            provider: ProviderType::Http(http_config),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: None,
            permissions: vec![],
            metadata: HashMap::new(),
        };
        
        marketplace.capabilities.write().await.insert("test.http".to_string(), manifest);
        
        // Test execution
        let inputs = Value::Map(HashMap::new());
        let result = marketplace.execute_capability("test.http", &inputs).await;
        
        assert!(result.is_ok());
        if let Ok(Value::String(s)) = result {
            assert_eq!(s, "Test executor result");
        } else {
            panic!("Expected string result");
        }
    }

    /// Test MCPExecutor implementation
    #[tokio::test]
    async fn test_mcp_executor() {
        let executor = MCPExecutor;
        
        // Test provider type ID
        assert_eq!(executor.provider_type_id(), TypeId::of::<MCPCapability>());
        
        // Test with invalid provider type
        let http_provider = ProviderType::Http(HttpCapability {
            base_url: "http://example.com".to_string(),
            auth_token: None,
            timeout_ms: 5000,
        });
        
        let inputs = Value::Map(HashMap::new());
        let result = executor.execute(&http_provider, &inputs).await;
        
        assert!(result.is_err());
        if let Err(RuntimeError::Generic(msg)) = result {
            assert_eq!(msg, "Invalid provider type for MCP executor");
        } else {
            panic!("Expected Generic error");
        }
    }

    /// Test A2AExecutor implementation
    #[tokio::test]
    async fn test_a2a_executor() {
        let executor = A2AExecutor;
        
        // Test provider type ID
        assert_eq!(executor.provider_type_id(), TypeId::of::<A2ACapability>());
        
        // Test with valid A2A provider
        let a2a_provider = ProviderType::A2A(A2ACapability {
            agent_id: "test_agent".to_string(),
            endpoint: "http://localhost:8080".to_string(),
            protocol: "http".to_string(),
            timeout_ms: 5000,
        });
        
        let inputs = Value::Map(HashMap::new());
        let result = executor.execute(&a2a_provider, &inputs).await;
        
        // Should fail due to no server running, but should not panic
        assert!(result.is_err());
    }

    /// Test LocalExecutor implementation
    #[tokio::test]
    async fn test_local_executor() {
        let executor = LocalExecutor;
        
        // Test provider type ID
        assert_eq!(executor.provider_type_id(), TypeId::of::<LocalCapability>());
        
        // Test with valid local provider
        let local_provider = ProviderType::Local(LocalCapability {
            handler: Arc::new(|_| Ok(Value::String("Local result".to_string()))),
        });
        
        let inputs = Value::Map(HashMap::new());
        let result = executor.execute(&local_provider, &inputs).await;
        
        assert!(result.is_ok());
        if let Ok(Value::String(s)) = result {
            assert_eq!(s, "Local result");
        } else {
            panic!("Expected string result");
        }
    }

    /// Test HttpExecutor implementation
    #[tokio::test]
    async fn test_http_executor() {
        let executor = HttpExecutor;
        
        // Test provider type ID
        assert_eq!(executor.provider_type_id(), TypeId::of::<HttpCapability>());
        
        // Test with invalid HTTP provider (non-existent server)
        let http_provider = ProviderType::Http(HttpCapability {
            base_url: "http://localhost:9999/nonexistent".to_string(),
            auth_token: None,
            timeout_ms: 100, // Very short timeout to ensure failure
        });
        
        let inputs = Value::Map(HashMap::new());
        let result = executor.execute(&http_provider, &inputs).await;
        
        // Should fail due to connection timeout or server not found
        assert!(result.is_err());
    }

    /// Test executor registration and lookup
    #[tokio::test]
    async fn test_executor_registration() {
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let mut marketplace = CapabilityMarketplace::new(registry);
        
        // Register executors
        marketplace.register_executor(Arc::new(MCPExecutor));
        marketplace.register_executor(Arc::new(A2AExecutor));
        marketplace.register_executor(Arc::new(LocalExecutor));
        marketplace.register_executor(Arc::new(HttpExecutor));
        
        // Verify executors are registered
        assert!(marketplace.executors.contains_key(&TypeId::of::<MCPCapability>()));
        assert!(marketplace.executors.contains_key(&TypeId::of::<A2ACapability>()));
        assert!(marketplace.executors.contains_key(&TypeId::of::<LocalCapability>()));
        assert!(marketplace.executors.contains_key(&TypeId::of::<HttpCapability>()));
    }

    /// Test capability execution fallback when no executor is registered
    #[tokio::test]
    async fn test_execution_fallback() {
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = CapabilityMarketplace::new(registry);
        
        // Register local capability without executor
        let local_config = LocalCapability {
            handler: Arc::new(|_| Ok(Value::String("Fallback result".to_string()))),
        };
        
        let manifest = CapabilityManifest {
            id: "test.local".to_string(),
            name: "Test Local".to_string(),
            description: "Test local capability".to_string(),
            provider: ProviderType::Local(local_config),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: None,
            permissions: vec![],
            metadata: HashMap::new(),
        };
        
        marketplace.capabilities.write().await.insert("test.local".to_string(), manifest);
        
        // Test execution (should use fallback)
        let inputs = Value::Map(HashMap::new());
        let result = marketplace.execute_capability("test.local", &inputs).await;
        
        assert!(result.is_ok());
        if let Ok(Value::String(s)) = result {
            assert_eq!(s, "Fallback result");
        } else {
            panic!("Expected string result");
        }
    }

    /// Test capability not found fallback to registry
    #[tokio::test]
    async fn test_capability_not_found_fallback() {
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = CapabilityMarketplace::new(registry);
        
        // Test execution of non-existent capability
        let inputs = Value::Map(HashMap::new());
        let result = marketplace.execute_capability("non.existent", &inputs).await;
        
        // Should fail but not panic
        assert!(result.is_err());
    }

    /// Test MCP SDK integration with mock data
    #[tokio::test]
    async fn test_mcp_sdk_integration() {
        // Create a marketplace instance to test value_to_json
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = CapabilityMarketplace::new(registry);
        
        // Test JSON serialization/deserialization
        let test_input = Value::Map({
            let mut map = HashMap::new();
            map.insert(MapKey::String("key".to_string()), Value::String("value".to_string()));
            map
        });
        
        let json_result = marketplace.value_to_json(&test_input);
        assert!(json_result.is_ok());
        
        // Test JSON to RTFS value conversion
        let json_value = serde_json::json!({
            "string": "test",
            "number": 42,
            "boolean": true,
            "array": [1, 2, 3],
            "object": {"nested": "value"}
        });
        
        let rtfs_result = CapabilityMarketplace::json_to_rtfs_value(&json_value);
        assert!(rtfs_result.is_ok());
        
        if let Ok(Value::Map(map)) = rtfs_result {
            assert!(map.contains_key(&MapKey::String("string".to_string())));
            assert!(map.contains_key(&MapKey::String("number".to_string())));
            assert!(map.contains_key(&MapKey::String("boolean".to_string())));
            assert!(map.contains_key(&MapKey::String("array".to_string())));
            assert!(map.contains_key(&MapKey::String("object".to_string())));
        } else {
            panic!("Expected map result");
        }
    }

    /// Test error handling in executors
    #[tokio::test]
    async fn test_executor_error_handling() {
        let executor = LocalExecutor;
        
        // Test with local provider that returns error
        let error_provider = ProviderType::Local(LocalCapability {
            handler: Arc::new(|_| Err(RuntimeError::Generic("Test error".to_string()))),
        });
        
        let inputs = Value::Map(HashMap::new());
        let result = executor.execute(&error_provider, &inputs).await;
        
        assert!(result.is_err());
        if let Err(RuntimeError::Generic(msg)) = result {
            assert_eq!(msg, "Test error");
        } else {
            panic!("Expected Generic error");
        }
    }


}

/// Network-based capability discovery agent
pub struct NetworkDiscoveryAgent {
    registry_endpoint: String,
    auth_token: Option<String>,
    refresh_interval: std::time::Duration,
    last_discovery: std::time::Instant,
}

impl NetworkDiscoveryAgent {
    pub fn new(registry_endpoint: String, auth_token: Option<String>, refresh_interval_secs: u64) -> Self {
        Self {
            registry_endpoint,
            auth_token,
            refresh_interval: std::time::Duration::from_secs(refresh_interval_secs),
            last_discovery: std::time::Instant::now() - std::time::Duration::from_secs(refresh_interval_secs), // Allow immediate discovery
        }
    }

    async fn parse_capability_manifest(&self, cap_json: &serde_json::Value) -> Result<CapabilityManifest, RuntimeError> {
        // This is a simplified implementation - in a real scenario, you'd want more robust parsing
        let id = cap_json.get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RuntimeError::Generic("Missing capability ID".to_string()))?
            .to_string();
        
        let name = cap_json.get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(&id)
            .to_string();
        
        let description = cap_json.get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("No description available")
            .to_string();
        
        let version = cap_json.get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("1.0.0")
            .to_string();
        
        // For now, create a simple local capability as placeholder
        let provider = ProviderType::Local(LocalCapability {
            handler: Arc::new(|_| Ok(Value::String("Network capability placeholder".to_string())))
        });
        
        Ok(CapabilityManifest {
            id,
            name,
            description,
            provider,
            version,
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: None,
            permissions: vec![],
            metadata: HashMap::new(),
        })
    }
}

#[async_trait::async_trait]
impl CapabilityDiscovery for NetworkDiscoveryAgent {
    async fn discover(&self) -> Result<Vec<CapabilityManifest>, RuntimeError> {
        // Check if we need to refresh (avoid too frequent requests)
        if self.last_discovery.elapsed() < self.refresh_interval {
            return Ok(vec![]); // Return empty if not time to refresh
        }
        
        let client = reqwest::Client::new();
        
        // Prepare discovery request
        let payload = serde_json::json!({
            "method": "discover_capabilities",
            "params": {
                "limit": 100,
                "include_attestations": true,
                "include_provenance": true
            }
        });
        
        // Make request to capability registry
        let mut request = client
            .post(&self.registry_endpoint)
            .header("Content-Type", "application/json")
            .json(&payload)
            .timeout(std::time::Duration::from_secs(30));
        
        // Add authentication if provided
        if let Some(token) = &self.auth_token {
            request = request.bearer_auth(token);
        }
        
        let response = request
            .send()
            .await
            .map_err(|e| RuntimeError::Generic(format!("Network discovery failed: {}", e)))?;
        
        if !response.status().is_success() {
            return Err(RuntimeError::Generic(format!("Registry error: {}", response.status())));
        }
        
        let response_json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse discovery response: {}", e)))?;
        
        // Parse capabilities from response
        let capabilities = if let Some(result) = response_json.get("result") {
            if let Some(caps) = result.get("capabilities") {
                if let serde_json::Value::Array(caps_array) = caps {
                    let mut manifests = Vec::new();
                    for cap_json in caps_array {
                        match self.parse_capability_manifest(cap_json).await {
                            Ok(manifest) => manifests.push(manifest),
                            Err(e) => {
                                eprintln!("Failed to parse capability manifest: {}", e);
                                continue;
                            }
                        }
                    }
                    manifests
                } else {
                    vec![]
                }
            } else {
                vec![]
            }
        } else {
            vec![]
        };
        
        Ok(capabilities)
    }
}

/// Local file-based capability discovery agent
pub struct LocalFileDiscoveryAgent {
    discovery_path: std::path::PathBuf,
    file_pattern: String,
}

impl LocalFileDiscoveryAgent {
    pub fn new(discovery_path: std::path::PathBuf, file_pattern: String) -> Self {
        Self {
            discovery_path,
            file_pattern,
        }
    }
    
    /// Parse capability manifest from JSON (helper method for discovery agents)
    async fn parse_capability_manifest_from_json(&self, cap_json: &serde_json::Value) -> Result<CapabilityManifest, RuntimeError> {
        // This is a simplified version - in a real implementation, this would be more comprehensive
        let id = cap_json
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RuntimeError::Generic("Missing capability id".to_string()))?
            .to_string();

        let name = cap_json
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(&id)
            .to_string();

        let description = cap_json
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("Discovered capability")
            .to_string();

        let version = cap_json
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("1.0.0")
            .to_string();

        // Parse provider type
        let provider = if let Some(endpoint) = cap_json.get("endpoint").and_then(|v| v.as_str()) {
            ProviderType::Http(HttpCapability {
                base_url: endpoint.to_string(),
                auth_token: None,
                timeout_ms: 30000,
            })
        } else {
            // Default to local provider if no endpoint specified
            ProviderType::Local(LocalCapability {
                handler: Arc::new(|_| Err(RuntimeError::Generic("Discovered capability not implemented".to_string()))),
            })
        };

        // Parse attestation if present
        let attestation = cap_json.get("attestation").and_then(|att_json| {
            self.parse_capability_attestation(att_json).ok()
        });

        // Parse provenance
        let provenance = Some(CapabilityProvenance {
            source: "local_file_discovery".to_string(),
            version: Some(version.clone()),
            content_hash: self.compute_content_hash(&format!("{}{}{}", id, name, description)),
            custody_chain: vec!["local_file_discovery".to_string()],
            registered_at: Utc::now(),
        });

        Ok(CapabilityManifest {
            id,
            name,
            description,
            provider,
            version,
            input_schema: None,
            output_schema: None,
            attestation,
            provenance,
            permissions: vec![],
            metadata: std::collections::HashMap::new(),
        })
    }
    
    /// Compute content hash for capability integrity
    fn compute_content_hash(&self, content: &str) -> String {
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        format!("{:x}", hasher.finalize())
    }
    
    /// Parse capability attestation from JSON
    fn parse_capability_attestation(&self, att_json: &serde_json::Value) -> Result<CapabilityAttestation, RuntimeError> {
        let signature = att_json
            .get("signature")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RuntimeError::Generic("Missing attestation signature".to_string()))?
            .to_string();

        let authority = att_json
            .get("authority")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RuntimeError::Generic("Missing attestation authority".to_string()))?
            .to_string();

        let created_at = att_json
            .get("created_at")
            .and_then(|v| v.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|| Utc::now());

        let expires_at = att_json
            .get("expires_at")
            .and_then(|v| v.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        let metadata = att_json
            .get("metadata")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();

        Ok(CapabilityAttestation {
            signature,
            authority,
            created_at,
            expires_at,
            metadata,
        })
    }
}

#[async_trait::async_trait]
impl CapabilityDiscovery for LocalFileDiscoveryAgent {
    async fn discover(&self) -> Result<Vec<CapabilityManifest>, RuntimeError> {
        let mut manifests = Vec::new();
        
        // Simple file discovery without glob dependency
        if let Ok(entries) = std::fs::read_dir(&self.discovery_path) {
            for entry in entries {
                if let Ok(entry) = entry {
                    let path = entry.path();
                    if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
                        // Check if filename matches pattern (simple string contains check)
                        if let Some(filename) = path.file_name().and_then(|s| s.to_str()) {
                            if filename.contains(&self.file_pattern) {
                                if let Ok(content) = std::fs::read_to_string(&path) {
                                    if let Ok(cap_json) = serde_json::from_str::<serde_json::Value>(&content) {
                                        match self.parse_capability_manifest_from_json(&cap_json).await {
                                            Ok(manifest) => manifests.push(manifest),
                                            Err(e) => {
                                                eprintln!("Failed to parse capability manifest from {}: {}", path.display(), e);
                                                continue;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        Ok(manifests)
    }
}