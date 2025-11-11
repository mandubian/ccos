use crate::streaming::{
    BidirectionalConfig, DuplexChannels, StreamCallbacks, StreamConfig, StreamType,
    StreamingProvider,
};
use chrono::{DateTime, Datelike, Timelike, Utc};
use rtfs::ast::TypeExpr;
use rtfs::runtime::error::RuntimeResult;
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::values::Value;
use serde::{Deserialize, Serialize};
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
    pub callbacks: Option<StreamCallbacks>,
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
    pub effects: Vec<String>,
    pub metadata: HashMap<String, String>,
    /// Agent-specific metadata flags for unified capability/agent model
    pub agent_metadata: Option<AgentMetadata>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenApiCapability {
    pub base_url: String,
    pub spec_url: Option<String>,
    pub operations: Vec<OpenApiOperation>,
    pub auth: Option<OpenApiAuth>,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenApiOperation {
    pub operation_id: Option<String>,
    pub method: String,
    pub path: String,
    pub summary: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenApiAuth {
    pub auth_type: String,
    pub location: String,
    pub parameter_name: String,
    pub env_var_name: Option<String>,
    pub required: bool,
}

/// Metadata flags to distinguish agents from capabilities in the unified model
#[derive(Debug, Clone, PartialEq)]
pub struct AgentMetadata {
    /// Kind of artifact: :primitive, :composite, or :agent
    pub kind: CapabilityKind,
    /// Whether this artifact can plan and select capabilities dynamically
    pub planning: bool,
    /// Whether this artifact maintains state across invocations
    pub stateful: bool,
    /// Whether this artifact can interact with humans (ask questions, get feedback)
    pub interactive: bool,
    /// Additional agent-specific configuration
    pub config: HashMap<String, String>,
}

/// Types of capabilities in the unified model
#[derive(Debug, Clone, PartialEq)]
pub enum CapabilityKind {
    /// Single-shot, stateless capability (default)
    Primitive,
    /// Fixed pipeline of capabilities
    Composite,
    /// Goal-directed controller with autonomy
    Agent,
}

impl CapabilityManifest {
    /// Create a new capability manifest with default (primitive) kind
    pub fn new(
        id: String,
        name: String,
        description: String,
        provider: ProviderType,
        version: String,
    ) -> Self {
        Self {
            id,
            name,
            description,
            provider,
            version,
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: None,
            permissions: Vec::new(),
            effects: Vec::new(),
            metadata: HashMap::new(),
            agent_metadata: None,
        }
    }

    /// Create a new agent manifest with agent metadata
    pub fn new_agent(
        id: String,
        name: String,
        description: String,
        provider: ProviderType,
        version: String,
        planning: bool,
        stateful: bool,
        interactive: bool,
    ) -> Self {
        Self {
            id,
            name,
            description,
            provider,
            version,
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: None,
            permissions: Vec::new(),
            effects: Vec::new(),
            metadata: HashMap::new(),
            agent_metadata: Some(AgentMetadata {
                kind: CapabilityKind::Agent,
                planning,
                stateful,
                interactive,
                config: HashMap::new(),
            }),
        }
    }

    /// Get the capability kind, defaulting to Primitive if no agent metadata
    pub fn kind(&self) -> CapabilityKind {
        self.agent_metadata
            .as_ref()
            .map(|m| m.kind.clone())
            .unwrap_or(CapabilityKind::Primitive)
    }

    /// Check if this is an agent (has planning, stateful, or interactive capabilities)
    pub fn is_agent(&self) -> bool {
        matches!(self.kind(), CapabilityKind::Agent)
    }

    /// Check if this capability can plan and select other capabilities
    pub fn can_plan(&self) -> bool {
        self.agent_metadata
            .as_ref()
            .map(|m| m.planning)
            .unwrap_or(false)
    }

    /// Check if this capability maintains state
    pub fn is_stateful(&self) -> bool {
        self.agent_metadata
            .as_ref()
            .map(|m| m.stateful)
            .unwrap_or(false)
    }

    /// Check if this capability can interact with humans
    pub fn is_interactive(&self) -> bool {
        self.agent_metadata
            .as_ref()
            .map(|m| m.interactive)
            .unwrap_or(false)
    }

    /// Set agent metadata for this capability
    pub fn with_agent_metadata(mut self, metadata: AgentMetadata) -> Self {
        self.agent_metadata = Some(metadata);
        self
    }
}

/// Isolation policy for capability execution
#[derive(Debug, Clone)]
pub struct CapabilityIsolationPolicy {
    pub allowed_capabilities: Vec<String>,
    pub denied_capabilities: Vec<String>,
    pub namespace_policies: HashMap<String, NamespacePolicy>,
    pub resource_constraints: Option<ResourceConstraints>,
    pub time_constraints: Option<TimeConstraints>,
}

#[derive(Debug, Clone)]
pub struct NamespacePolicy {
    pub allowed_patterns: Vec<String>,
    pub denied_patterns: Vec<String>,
    pub resource_limits: Option<ResourceConstraints>,
}

/// Flexible resource constraint system that can handle any resource type
/// without breaking existing events when new resources are added
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceConstraints {
    /// Core resource limits (backward compatible)
    pub core_limits: CoreResourceLimits,
    /// Extended resource limits for new resource types
    pub extended_limits: HashMap<String, ResourceLimit>,
    /// Resource monitoring configuration
    pub monitoring_config: ResourceMonitoringConfig,
}

/// Core resource limits that are always available
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreResourceLimits {
    pub max_memory_mb: Option<u64>,
    pub max_cpu_percent: Option<f64>,
    pub max_execution_time_seconds: Option<u64>,
    pub max_concurrent_calls: Option<u32>,
}

/// Generic resource limit that can represent any resource type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimit {
    pub value: f64,
    pub unit: String,
    pub resource_type: ResourceType,
    pub enforcement_level: EnforcementLevel,
}

/// Supported resource types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ResourceType {
    // Core resources
    Memory,
    Cpu,
    ExecutionTime,
    ConcurrentCalls,

    // AI/ML specific resources
    GpuMemory,
    GpuUtilization,
    GpuComputeUnits,

    // Environmental resources
    Co2Emissions,
    EnergyConsumption,

    // Network resources
    NetworkBandwidth,
    NetworkLatency,

    // Storage resources
    DiskSpace,
    DiskIO,

    // Custom resource type
    Custom(String),
}

/// How strictly a resource limit should be enforced
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EnforcementLevel {
    /// Soft limit - log warning but allow execution
    Warning,
    /// Hard limit - prevent execution if exceeded
    Hard,
    /// Adaptive limit - dynamically adjust based on system load
    Adaptive,
}

/// Configuration for resource monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceMonitoringConfig {
    /// Whether to enable real-time monitoring
    pub enabled: bool,
    /// Monitoring interval in milliseconds
    pub monitoring_interval_ms: u64,
    /// Whether to collect historical data
    pub collect_history: bool,
    /// Maximum history retention period in seconds
    pub history_retention_seconds: Option<u64>,
    /// Resource-specific monitoring settings
    pub resource_settings: HashMap<ResourceType, ResourceMonitoringSettings>,
}

/// Settings for monitoring a specific resource type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceMonitoringSettings {
    /// Whether to monitor this resource type
    pub enabled: bool,
    /// Sampling rate (how often to check)
    pub sampling_rate_ms: u64,
    /// Alert threshold percentage
    pub alert_threshold_percent: f64,
    /// Whether to log detailed metrics
    pub detailed_logging: bool,
}

/// Current resource usage snapshot
#[derive(Debug, Clone)]
pub struct ResourceUsage {
    pub timestamp: DateTime<Utc>,
    pub capability_id: String,
    pub resources: HashMap<ResourceType, ResourceMeasurement>,
}

/// Measurement of a specific resource
#[derive(Debug, Clone)]
pub struct ResourceMeasurement {
    pub value: f64,
    pub unit: String,
    pub resource_type: ResourceType,
    pub is_limit_exceeded: bool,
    pub limit_value: Option<f64>,
}

impl Default for ResourceConstraints {
    fn default() -> Self {
        Self {
            core_limits: CoreResourceLimits {
                max_memory_mb: None,
                max_cpu_percent: None,
                max_execution_time_seconds: None,
                max_concurrent_calls: None,
            },
            extended_limits: HashMap::new(),
            monitoring_config: ResourceMonitoringConfig {
                enabled: false,
                monitoring_interval_ms: 1000,
                collect_history: false,
                history_retention_seconds: None,
                resource_settings: HashMap::new(),
            },
        }
    }
}

impl ResourceConstraints {
    /// Create constraints with GPU limits
    pub fn with_gpu_limits(
        mut self,
        gpu_memory_mb: Option<u64>,
        gpu_utilization_percent: Option<f64>,
    ) -> Self {
        if let Some(memory) = gpu_memory_mb {
            self.extended_limits.insert(
                "gpu_memory".to_string(),
                ResourceLimit {
                    value: memory as f64,
                    unit: "MB".to_string(),
                    resource_type: ResourceType::GpuMemory,
                    enforcement_level: EnforcementLevel::Hard,
                },
            );
        }

        if let Some(utilization) = gpu_utilization_percent {
            self.extended_limits.insert(
                "gpu_utilization".to_string(),
                ResourceLimit {
                    value: utilization,
                    unit: "%".to_string(),
                    resource_type: ResourceType::GpuUtilization,
                    enforcement_level: EnforcementLevel::Hard,
                },
            );
        }

        self
    }

    /// Create constraints with environmental limits
    pub fn with_environmental_limits(
        mut self,
        co2_emissions_g: Option<f64>,
        energy_consumption_kwh: Option<f64>,
    ) -> Self {
        if let Some(co2) = co2_emissions_g {
            self.extended_limits.insert(
                "co2_emissions".to_string(),
                ResourceLimit {
                    value: co2,
                    unit: "g".to_string(),
                    resource_type: ResourceType::Co2Emissions,
                    enforcement_level: EnforcementLevel::Warning, // Usually warning for environmental
                },
            );
        }

        if let Some(energy) = energy_consumption_kwh {
            self.extended_limits.insert(
                "energy_consumption".to_string(),
                ResourceLimit {
                    value: energy,
                    unit: "kWh".to_string(),
                    resource_type: ResourceType::EnergyConsumption,
                    enforcement_level: EnforcementLevel::Warning,
                },
            );
        }

        self
    }

    /// Add a custom resource limit
    pub fn with_custom_limit(
        mut self,
        name: &str,
        value: f64,
        unit: &str,
        enforcement_level: EnforcementLevel,
    ) -> Self {
        self.extended_limits.insert(
            name.to_string(),
            ResourceLimit {
                value,
                unit: unit.to_string(),
                resource_type: ResourceType::Custom(name.to_string()),
                enforcement_level,
            },
        );
        self
    }

    /// Check if a resource usage exceeds limits
    pub fn check_resource_limits(&self, usage: &ResourceUsage) -> Vec<ResourceViolation> {
        let mut violations = Vec::new();

        // Check core resource limits
        if let Some(memory_limit) = self.core_limits.max_memory_mb {
            if let Some(memory_usage) = usage.resources.get(&ResourceType::Memory) {
                if memory_usage.value > memory_limit as f64 {
                    violations.push(ResourceViolation {
                        resource_type: ResourceType::Memory,
                        current_value: memory_usage.value,
                        limit_value: memory_limit as f64,
                        unit: memory_usage.unit.clone(),
                        enforcement_level: EnforcementLevel::Hard,
                    });
                }
            }
        }

        // Check extended resource limits
        for (_name, limit) in &self.extended_limits {
            if let Some(measurement) = usage.resources.get(&limit.resource_type) {
                if measurement.value > limit.value {
                    violations.push(ResourceViolation {
                        resource_type: limit.resource_type.clone(),
                        current_value: measurement.value,
                        limit_value: limit.value,
                        unit: measurement.unit.clone(),
                        enforcement_level: limit.enforcement_level.clone(),
                    });
                }
            }
        }

        violations
    }

    /// Get all resource types that should be monitored
    pub fn get_monitored_resources(&self) -> Vec<ResourceType> {
        let mut resources = Vec::new();

        // Add core resources if limits are set
        if self.core_limits.max_memory_mb.is_some() {
            resources.push(ResourceType::Memory);
        }
        if self.core_limits.max_cpu_percent.is_some() {
            resources.push(ResourceType::Cpu);
        }
        if self.core_limits.max_execution_time_seconds.is_some() {
            resources.push(ResourceType::ExecutionTime);
        }

        // Add extended resources
        for limit in self.extended_limits.values() {
            resources.push(limit.resource_type.clone());
        }

        resources
    }
}

/// Resource limit violation
#[derive(Debug, Clone)]
pub struct ResourceViolation {
    pub resource_type: ResourceType,
    pub current_value: f64,
    pub limit_value: f64,
    pub unit: String,
    pub enforcement_level: EnforcementLevel,
}

impl ResourceViolation {
    pub fn is_hard_violation(&self) -> bool {
        matches!(self.enforcement_level, EnforcementLevel::Hard)
    }

    pub fn to_string(&self) -> String {
        format!(
            "Resource limit exceeded: {} {} (limit: {} {})",
            self.current_value, self.unit, self.limit_value, self.unit
        )
    }
}

#[derive(Debug, Clone)]
pub struct TimeConstraints {
    pub allowed_hours: Option<Vec<u8>>, // 0-23
    pub allowed_days: Option<Vec<u8>>,  // 0-6 (Sunday = 0)
    pub timezone: Option<String>,       // IANA timezone identifier
}

impl Default for CapabilityIsolationPolicy {
    fn default() -> Self {
        Self {
            allowed_capabilities: vec!["*".to_string()], // Allow all by default
            denied_capabilities: vec![],
            namespace_policies: HashMap::new(),
            resource_constraints: None,
            time_constraints: None,
        }
    }
}

impl CapabilityIsolationPolicy {
    /// Create a restrictive policy that denies all by default
    pub fn restrictive() -> Self {
        Self {
            allowed_capabilities: vec![],
            denied_capabilities: vec!["*".to_string()],
            namespace_policies: HashMap::new(),
            resource_constraints: None,
            time_constraints: None,
        }
    }

    /// Create a namespace-based policy
    pub fn with_namespace_policy(mut self, namespace: &str, policy: NamespacePolicy) -> Self {
        self.namespace_policies
            .insert(namespace.to_string(), policy);
        self
    }

    /// Add resource constraints
    pub fn with_resource_constraints(mut self, constraints: ResourceConstraints) -> Self {
        self.resource_constraints = Some(constraints);
        self
    }

    /// Add time constraints
    pub fn with_time_constraints(mut self, constraints: TimeConstraints) -> Self {
        self.time_constraints = Some(constraints);
        self
    }

    /// Check if a capability is allowed based on namespace policies
    pub fn check_namespace_access(&self, capability_id: &str) -> bool {
        for (namespace, policy) in &self.namespace_policies {
            if capability_id.starts_with(namespace) {
                // Check allowed patterns
                let mut allowed = false;
                for pattern in &policy.allowed_patterns {
                    if self.matches_pattern(capability_id, pattern) {
                        allowed = true;
                        break;
                    }
                }
                if !allowed {
                    return false;
                }

                // Check denied patterns
                for pattern in &policy.denied_patterns {
                    if self.matches_pattern(capability_id, pattern) {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Check if current time is within allowed constraints
    pub fn check_time_constraints(&self) -> bool {
        if let Some(time_constraints) = &self.time_constraints {
            let now = chrono::Utc::now();

            // Check hours
            if let Some(allowed_hours) = &time_constraints.allowed_hours {
                let current_hour = now.hour() as u8;
                if !allowed_hours.contains(&current_hour) {
                    return false;
                }
            }

            // Check days
            if let Some(allowed_days) = &time_constraints.allowed_days {
                let current_day = now.weekday().num_days_from_sunday() as u8;
                if !allowed_days.contains(&current_day) {
                    return false;
                }
            }
        }
        true
    }

    /// Simple pattern matching for glob patterns
    fn matches_pattern(&self, capability_id: &str, pattern: &str) -> bool {
        if pattern == "*" {
            return true;
        }

        if pattern.contains('*') {
            // Simple glob matching - convert * to .* for regex
            let regex_pattern = pattern.replace('*', ".*");
            if let Ok(regex) = regex::Regex::new(&regex_pattern) {
                return regex.is_match(capability_id);
            }
        }

        capability_id == pattern
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderType {
    Local(LocalCapability),
    Http(HttpCapability),
    MCP(MCPCapability),
    A2A(A2ACapability),
    OpenApi(OpenApiCapability),
    Plugin(PluginCapability),
    RemoteRTFS(RemoteRTFSCapability),
    Stream(StreamCapabilityImpl),
    Registry(RegistryCapability),
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

#[derive(Clone)]
pub struct RegistryCapability {
    pub capability_id: String,
    pub registry: Arc<RwLock<rtfs::runtime::capabilities::registry::CapabilityRegistry>>,
}

impl std::fmt::Debug for RegistryCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegistryCapability")
            .field("capability_id", &self.capability_id)
            .finish()
    }
}

impl PartialEq for RegistryCapability {
    fn eq(&self, other: &Self) -> bool {
        self.capability_id == other.capability_id
    }
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
    pub(crate) capability_registry:
        Arc<RwLock<rtfs::runtime::capabilities::registry::CapabilityRegistry>>,
    pub(crate) type_validator: Arc<rtfs::runtime::type_validator::TypeValidator>,
    pub(crate) executor_registry:
        std::collections::HashMap<std::any::TypeId, super::executors::ExecutorVariant>,
    pub(crate) isolation_policy: CapabilityIsolationPolicy,
    pub(crate) causal_chain: Option<Arc<std::sync::Mutex<crate::causal_chain::CausalChain>>>,
    pub(crate) resource_monitor: Option<Arc<super::resource_monitor::ResourceMonitor>>,
    pub(crate) debug_callback: Option<Arc<dyn Fn(String) + Send + Sync>>,
    /// Optional session pool for stateful capabilities (generic, provider-agnostic)
    /// Uses RwLock for interior mutability since marketplace is wrapped in Arc
    pub(crate) session_pool: Arc<RwLock<Option<Arc<crate::capabilities::SessionPoolManager>>>>,
    /// Optional catalog service for indexing capabilities
    pub(crate) catalog: Arc<RwLock<Option<Arc<crate::catalog::CatalogService>>>>,
}

/// Trait for capability discovery providers
#[async_trait::async_trait]
pub trait CapabilityDiscovery: Send + Sync {
    /// Discover capabilities and return their manifests
    async fn discover(&self) -> RuntimeResult<Vec<CapabilityManifest>>;

    /// Get the name of this discovery provider
    fn name(&self) -> &str;

    /// Get this object as Any for downcasting
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Query filters for capability marketplace discovery
#[derive(Debug, Clone, Default)]
pub struct CapabilityQuery {
    /// Filter by capability kind
    pub kind: Option<CapabilityKind>,
    /// Filter by planning capability
    pub planning: Option<bool>,
    /// Filter by stateful capability
    pub stateful: Option<bool>,
    /// Filter by interactive capability
    pub interactive: Option<bool>,
    /// Filter by capability ID pattern
    pub id_pattern: Option<String>,
    /// Filter by provider type
    pub provider_type: Option<ProviderType>,
    /// Filter by permissions
    pub required_permissions: Option<Vec<String>>,
    /// Filter by effects
    pub required_effects: Option<Vec<String>>,
    /// Limit number of results
    pub limit: Option<usize>,
}

impl CapabilityQuery {
    /// Create a new empty query
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter by capability kind
    pub fn with_kind(mut self, kind: CapabilityKind) -> Self {
        self.kind = Some(kind);
        self
    }

    /// Filter for agent capabilities only
    pub fn agents_only(mut self) -> Self {
        self.kind = Some(CapabilityKind::Agent);
        self
    }

    /// Filter for primitive capabilities only
    pub fn primitives_only(mut self) -> Self {
        self.kind = Some(CapabilityKind::Primitive);
        self
    }

    /// Filter for composite capabilities only
    pub fn composites_only(mut self) -> Self {
        self.kind = Some(CapabilityKind::Composite);
        self
    }

    /// Filter by planning capability
    pub fn with_planning(mut self, planning: bool) -> Self {
        self.planning = Some(planning);
        self
    }

    /// Filter by stateful capability
    pub fn with_stateful(mut self, stateful: bool) -> Self {
        self.stateful = Some(stateful);
        self
    }

    /// Filter by interactive capability
    pub fn with_interactive(mut self, interactive: bool) -> Self {
        self.interactive = Some(interactive);
        self
    }

    /// Filter by ID pattern (supports glob patterns)
    pub fn with_id_pattern(mut self, pattern: String) -> Self {
        self.id_pattern = Some(pattern);
        self
    }

    /// Filter by provider type
    pub fn with_provider_type(mut self, provider_type: ProviderType) -> Self {
        self.provider_type = Some(provider_type);
        self
    }

    /// Filter by required permissions
    pub fn with_permissions(mut self, permissions: Vec<String>) -> Self {
        self.required_permissions = Some(permissions);
        self
    }

    /// Filter by required effects
    pub fn with_effects(mut self, effects: Vec<String>) -> Self {
        self.required_effects = Some(effects);
        self
    }

    /// Limit number of results
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Check if a capability matches this query
    pub fn matches(&self, manifest: &CapabilityManifest) -> bool {
        // Check kind filter
        if let Some(ref kind) = self.kind {
            if manifest.kind() != *kind {
                return false;
            }
        }

        // Check planning filter
        if let Some(planning) = self.planning {
            if manifest.can_plan() != planning {
                return false;
            }
        }

        // Check stateful filter
        if let Some(stateful) = self.stateful {
            if manifest.is_stateful() != stateful {
                return false;
            }
        }

        // Check interactive filter
        if let Some(interactive) = self.interactive {
            if manifest.is_interactive() != interactive {
                return false;
            }
        }

        // Check ID pattern filter
        if let Some(ref pattern) = self.id_pattern {
            if !self.matches_pattern(&manifest.id, pattern) {
                return false;
            }
        }

        // Check provider type filter
        if let Some(ref provider_type) = self.provider_type {
            if manifest.provider != *provider_type {
                return false;
            }
        }

        // Check permissions filter
        if let Some(ref required_permissions) = self.required_permissions {
            for permission in required_permissions {
                if !manifest.permissions.contains(permission) {
                    return false;
                }
            }
        }

        // Check effects filter
        if let Some(ref required_effects) = self.required_effects {
            for effect in required_effects {
                if !manifest.effects.contains(effect) {
                    return false;
                }
            }
        }

        true
    }

    /// Simple pattern matching for glob patterns
    fn matches_pattern(&self, capability_id: &str, pattern: &str) -> bool {
        if pattern == "*" {
            return true;
        }

        if pattern.contains('*') {
            // Simple glob matching - convert * to .* for regex
            let regex_pattern = pattern.replace('*', ".*");
            if let Ok(regex) = regex::Regex::new(&regex_pattern) {
                return regex.is_match(capability_id);
            }
        }

        capability_id == pattern
    }
}

/// Trait for capability executors
pub trait CapabilityExecutor: Send + Sync {
    /// Execute a capability with the given input
    fn execute(
        &self,
        input: Value,
        context: RuntimeContext,
    ) -> impl std::future::Future<Output = RuntimeResult<Value>> + Send;

    /// Get the capability ID this executor handles
    fn capability_id(&self) -> &str;
}

// Implementation moved to marketplace.rs to avoid conflicts
