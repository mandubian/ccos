use crate::ast::TypeExpr;
use crate::runtime::streaming::{StreamType, BidirectionalConfig, DuplexChannels, StreamConfig, StreamingProvider};
use crate::ccos::capabilities::registry::CapabilityRegistry;
use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::runtime::values::Value;
use chrono::{DateTime, Utc, Timelike, Datelike};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Serialize, Deserialize};

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
        for (name, limit) in &self.extended_limits {
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
        self.namespace_policies.insert(namespace.to_string(), policy);
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
    pub(crate) isolation_policy: CapabilityIsolationPolicy,
    pub(crate) causal_chain: Option<Arc<std::sync::Mutex<crate::ccos::causal_chain::CausalChain>>>,
    pub(crate) resource_monitor: Option<()>, // TODO: Add ResourceMonitor when available
    pub(crate) debug_callback: Option<Arc<dyn Fn(String) + Send + Sync>>,
}

impl CapabilityMarketplace {
    /// Create a new CapabilityMarketplace with the given registry
    pub fn new(registry: Arc<RwLock<CapabilityRegistry>>) -> Self {
        Self {
            capabilities: Arc::new(RwLock::new(HashMap::new())),
            discovery_agents: Vec::new(),
            capability_registry: registry,
            network_registry: None,
            type_validator: Arc::new(crate::runtime::type_validator::TypeValidator::new()),
            executor_registry: std::collections::HashMap::new(),
            isolation_policy: CapabilityIsolationPolicy::default(),
            causal_chain: None,
            resource_monitor: None,
            debug_callback: None,
        }
    }

    /// Create a new CapabilityMarketplace with causal chain and debug callback
    pub fn with_causal_chain_and_debug_callback(
        registry: Arc<RwLock<CapabilityRegistry>>,
        causal_chain: Option<Arc<std::sync::Mutex<crate::ccos::causal_chain::CausalChain>>>,
        debug_callback: Option<Arc<dyn Fn(String) + Send + Sync>>,
    ) -> Self {
        Self {
            capabilities: Arc::new(RwLock::new(HashMap::new())),
            discovery_agents: Vec::new(),
            capability_registry: registry,
            network_registry: None,
            type_validator: Arc::new(crate::runtime::type_validator::TypeValidator::new()),
            executor_registry: std::collections::HashMap::new(),
            isolation_policy: CapabilityIsolationPolicy::default(),
            causal_chain,
            resource_monitor: None,
            debug_callback,
        }
    }

    /// Bootstrap the marketplace by discovering capabilities
    pub async fn bootstrap(&self) -> RuntimeResult<()> {
        // For now, just return Ok - this would normally run discovery agents
        Ok(())
    }

    /// Get a capability by ID
    pub async fn get_capability(&self, capability_id: &str) -> Option<CapabilityManifest> {
        let capabilities = self.capabilities.read().await;
        capabilities.get(capability_id).cloned()
    }

    /// Execute a capability
    pub async fn execute_capability(&self, capability_id: &str, args: &[Value]) -> RuntimeResult<Value> {
        // For now, return a simple echo for testing
        if capability_id == "ccos.echo" {
            Ok(args.get(0).cloned().unwrap_or(Value::Nil))
        } else {
            Err(RuntimeError::Generic(format!("Capability '{}' not found", capability_id)))
        }
    }

    /// Register a local capability
    pub async fn register_local_capability(&self, _capability_id: &str, _manifest: CapabilityManifest) -> RuntimeResult<()> {
        // For now, just return Ok - this would normally register the capability
        Ok(())
    }

    /// Register a streaming capability
    pub async fn register_streaming_capability(&self, _capability_id: &str, _manifest: CapabilityManifest) -> RuntimeResult<()> {
        // For now, just return Ok - this would normally register the streaming capability
        Ok(())
    }

    /// Start a stream with config
    pub async fn start_stream_with_config(&self, _capability_id: &str, _config: StreamConfig) -> RuntimeResult<String> {
        // For now, return a dummy handle
        Ok("dummy-stream-handle".to_string())
    }

    /// Start a bidirectional stream with config
    pub async fn start_bidirectional_stream_with_config(&self, _capability_id: &str, _config: BidirectionalConfig) -> RuntimeResult<String> {
        // For now, return a dummy handle
        Ok("dummy-bidirectional-stream-handle".to_string())
    }

    /// Convert JSON to RTFS value
    pub fn json_to_rtfs_value(json: serde_json::Value) -> RuntimeResult<Value> {
        match json {
            serde_json::Value::Null => Ok(Value::Nil),
            serde_json::Value::Bool(b) => Ok(Value::Boolean(b)),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Ok(Value::Number(i as f64))
                } else if let Some(f) = n.as_f64() {
                    Ok(Value::Number(f))
                } else {
                    Err(RuntimeError::Generic("Invalid number".to_string()))
                }
            }
            serde_json::Value::String(s) => Ok(Value::String(s)),
            serde_json::Value::Array(arr) => {
                let values: Result<Vec<Value>, _> = arr.into_iter().map(Self::json_to_rtfs_value).collect();
                Ok(Value::Vector(values?))
            }
            serde_json::Value::Object(obj) => {
                let mut map = std::collections::HashMap::new();
                for (k, v) in obj {
                    map.insert(k, Self::json_to_rtfs_value(v)?);
                }
                Ok(Value::Map(map))
            }
        }
    }
}

#[async_trait::async_trait]
pub trait CapabilityDiscovery: Send + Sync {
    async fn discover(&self) -> Result<Vec<CapabilityManifest>, RuntimeError>;
    
    /// Get the name of this discovery provider
    fn name(&self) -> &str;
    
    /// Get a reference to the underlying type for downcasting
    fn as_any(&self) -> &dyn std::any::Any;
}

pub struct NoOpCapabilityDiscovery;

#[async_trait::async_trait]
impl CapabilityDiscovery for NoOpCapabilityDiscovery {
    async fn discover(&self) -> Result<Vec<CapabilityManifest>, RuntimeError> { Ok(vec![]) }
    
    fn name(&self) -> &str {
        "NoOpDiscovery"
    }
    
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
