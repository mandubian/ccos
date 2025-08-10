use crate::ast::{MapKey, TypeExpr};
use crate::runtime::streaming::{StreamType, BidirectionalConfig, DuplexChannels, StreamConfig, StreamingCapability, StreamHandle, StreamingProvider};
use crate::runtime::capability_registry::CapabilityRegistry;
use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::runtime::values::Value;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, PartialEq)]
pub struct CapabilityAttestation {
    pub signature: String,
    pub authority: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CapabilityProvenance {
    pub source: String,
    pub version: Option<String>,
    pub content_hash: String,
    pub custody_chain: Vec<String>,
    pub registered_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NetworkRegistryConfig {
    pub endpoint: String,
    pub callbacks: Option<crate::runtime::streaming::StreamCallbacks>,
    pub auto_reconnect: bool,
    pub max_retries: u32,
}

#[derive(Clone)]
pub struct StreamCapabilityImpl {
    pub provider: StreamingProvider,
    pub stream_type: StreamType,
    pub input_schema: Option<TypeExpr>,
    pub output_schema: Option<TypeExpr>,
    pub supports_progress: bool,
    pub supports_cancellation: bool,
    pub bidirectional_config: Option<BidirectionalConfig>,
    pub duplex_config: Option<DuplexChannels>,
    pub stream_config: Option<StreamConfig>,
}

impl PartialEq for StreamCapabilityImpl {
    fn eq(&self, other: &Self) -> bool {
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

#[derive(Debug, Clone)]
pub struct CapabilityManifest {
    pub id: String,
    pub name: String,
    pub description: String,
    pub provider: ProviderType,
    pub version: String,
    pub input_schema: Option<TypeExpr>,
    pub output_schema: Option<TypeExpr>,
    pub attestation: Option<CapabilityAttestation>,
    pub provenance: Option<CapabilityProvenance>,
    pub permissions: Vec<String>,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderType {
    Local(LocalCapability),
    Http(HttpCapability),
    MCP(MCPCapability),
    A2A(A2ACapability),
    Plugin(PluginCapability),
    RemoteRTFS(RemoteRTFSCapability),
    Stream(StreamCapabilityImpl),
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteRTFSCapability {
    pub endpoint: String,
    pub timeout_ms: u64,
    pub auth_token: Option<String>,
}

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
        Arc::ptr_eq(&self.handler, &other.handler)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HttpCapability {
    pub base_url: String,
    pub auth_token: Option<String>,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MCPCapability {
    pub server_url: String,
    pub tool_name: String,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct A2ACapability {
    pub agent_id: String,
    pub endpoint: String,
    pub protocol: String,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PluginCapability {
    pub plugin_path: String,
    pub function_name: String,
}

pub struct CapabilityMarketplace {
    pub(crate) capabilities: Arc<RwLock<HashMap<String, CapabilityManifest>>>,
    pub(crate) discovery_agents: Vec<Box<dyn CapabilityDiscovery>>,
    pub(crate) capability_registry: Arc<RwLock<CapabilityRegistry>>,
    pub(crate) network_registry: Option<NetworkRegistryConfig>,
    pub(crate) type_validator: Arc<crate::runtime::type_validator::TypeValidator>,
    pub(crate) executor_registry: std::collections::HashMap<std::any::TypeId, super::executors::ExecutorVariant>,
}

#[async_trait::async_trait]
pub trait CapabilityDiscovery: Send + Sync {
    async fn discover(&self) -> Result<Vec<CapabilityManifest>, RuntimeError>;
}

pub struct NoOpCapabilityDiscovery;

#[async_trait::async_trait]
impl CapabilityDiscovery for NoOpCapabilityDiscovery {
    async fn discover(&self) -> Result<Vec<CapabilityManifest>, RuntimeError> { Ok(vec![]) }
}
