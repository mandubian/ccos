use crate::streaming::{
    BidirectionalConfig, DuplexChannels, StreamCallbacks, StreamConfig, StreamType,
    StreamingProvider,
};
use chrono::{DateTime, Datelike, Timelike, Utc};
use futures::future::BoxFuture;
use rtfs::ast::TypeExpr;
use rtfs::runtime::error::RuntimeResult;
use rtfs::runtime::host_interface::HostInterface;
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::values::Value;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock as StdRwLock;
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
    /// Domains this capability belongs to (e.g., "github", "code", "devops", "communication")
    /// Domains are hierarchical strings using dot notation: "cloud.aws.s3", "code.rust"
    /// New domains can be added dynamically without code changes
    pub domains: Vec<String>,
    /// Categories for grouping capabilities (e.g., "crud", "search", "transform", "notify")
    /// Categories describe what kind of operation the capability performs
    /// New categories can be added dynamically without code changes  
    pub categories: Vec<String>,
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
            domains: Vec::new(),
            categories: Vec::new(),
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
            domains: Vec::new(),
            categories: Vec::new(),
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

    /// Add domains to this capability
    /// Domains are hierarchical strings using dot notation: "cloud.aws.s3", "code.rust"
    pub fn with_domains(mut self, domains: Vec<String>) -> Self {
        self.domains = domains;
        self
    }

    /// Add a single domain to this capability
    pub fn with_domain(mut self, domain: impl Into<String>) -> Self {
        self.domains.push(domain.into());
        self
    }

    /// Add categories to this capability
    /// Categories describe what kind of operation: "crud", "search", "transform", "notify"
    pub fn with_categories(mut self, categories: Vec<String>) -> Self {
        self.categories = categories;
        self
    }

    /// Add a single category to this capability
    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        self.categories.push(category.into());
        self
    }

    /// Check if capability matches a domain (supports prefix matching)
    /// e.g., "github" matches "github.issues" and "github.repos"
    pub fn matches_domain(&self, domain: &str) -> bool {
        self.domains.iter().any(|d| {
            d == domain
                || d.starts_with(&format!("{}.", domain))
                || domain.starts_with(&format!("{}.", d))
        })
    }

    /// Check if capability matches any of the given domains
    pub fn matches_any_domain(&self, domains: &[String]) -> bool {
        domains.iter().any(|d| self.matches_domain(d))
    }

    /// Check if capability has a specific category
    pub fn has_category(&self, category: &str) -> bool {
        self.categories.iter().any(|c| c == category)
    }

    /// Check if capability has any of the given categories
    pub fn has_any_category(&self, categories: &[String]) -> bool {
        categories.iter().any(|c| self.has_category(c))
    }

    /// Infer domains from source (server/registry name) and capability name
    ///
    /// For example:
    /// - source="github", name="list_issues" → ["github", "github.issues"]
    /// - source="modelcontextprotocol/github", name="get_pull_request" → ["github", "github.pull_requests"]
    /// - source="slack", name="send_message" → ["slack", "slack.messages"]
    ///
    /// Returns the inferred domains that can be added to the capability
    pub fn infer_domains(source: &str, capability_name: &str) -> Vec<String> {
        let mut domains = Vec::new();

        // Step 1: Extract primary domain from source
        let primary_domain = Self::extract_primary_domain(source);
        if !primary_domain.is_empty() {
            domains.push(primary_domain.clone());
        }

        // Step 2: Extract sub-domain from capability name
        if let Some(sub_domain) = Self::extract_sub_domain(capability_name) {
            if !primary_domain.is_empty() {
                domains.push(format!("{}.{}", primary_domain, sub_domain));
            } else {
                domains.push(sub_domain);
            }
        }

        domains
    }

    /// Extract the primary domain from a source/server name
    /// Handles formats like:
    /// - "github" → "github"
    /// - "modelcontextprotocol/github" → "github"  
    /// - "github-mcp-server" → "github"
    /// - "my-slack-bot" → "slack"
    fn extract_primary_domain(source: &str) -> String {
        let source_lower = source.to_lowercase();

        // Handle namespace/name format (e.g., "modelcontextprotocol/github")
        let name_part = if let Some(pos) = source_lower.rfind('/') {
            &source_lower[pos + 1..]
        } else {
            &source_lower
        };

        // Remove common suffixes
        let clean_name = name_part
            .replace("-mcp-server", "")
            .replace("_mcp_server", "")
            .replace("-server", "")
            .replace("_server", "")
            .replace("-mcp", "")
            .replace("_mcp", "")
            .replace("-bot", "")
            .replace("_bot", "")
            .replace("-api", "")
            .replace("_api", "");

        // Map to known domains or use the cleaned name
        match clean_name.as_str() {
            s if s.contains("github") || s.contains("gh") => "github".to_string(),
            s if s.contains("slack") => "slack".to_string(),
            s if s.contains("discord") => "discord".to_string(),
            s if s.contains("jira") => "jira".to_string(),
            s if s.contains("confluence") => "confluence".to_string(),
            s if s.contains("notion") => "notion".to_string(),
            s if s.contains("linear") => "linear".to_string(),
            s if s.contains("aws") || s.contains("amazon") => "cloud.aws".to_string(),
            s if s.contains("gcp") || s.contains("google") => "cloud.gcp".to_string(),
            s if s.contains("azure") => "cloud.azure".to_string(),
            s if s.contains("postgres")
                || s.contains("mysql")
                || s.contains("sqlite")
                || s.contains("database")
                || s.contains("db") =>
            {
                "database".to_string()
            }
            s if s.contains("file") || s.contains("filesystem") || s.contains("fs") => {
                "filesystem".to_string()
            }
            s if s.contains("http") || s.contains("fetch") || s.contains("web") => {
                "web".to_string()
            }
            s if s.contains("email") || s.contains("mail") || s.contains("smtp") => {
                "email".to_string()
            }
            s if s.contains("calendar") || s.contains("gcal") => "calendar".to_string(),
            _ => clean_name.replace(['-', '_'], "."),
        }
    }

    /// Extract sub-domain from capability/tool name
    /// Maps common patterns to resource types:
    /// - "list_issues", "get_issue", "create_issue" → "issues"
    /// - "list_pull_requests", "get_pr" → "pull_requests"
    /// - "send_message", "post_message" → "messages"
    fn extract_sub_domain(capability_name: &str) -> Option<String> {
        let name_lower = capability_name.to_lowercase();

        // Remove action prefixes to get the resource part
        let resource_part = name_lower
            .strip_prefix("list_")
            .or_else(|| name_lower.strip_prefix("get_"))
            .or_else(|| name_lower.strip_prefix("get_all_"))
            .or_else(|| name_lower.strip_prefix("create_"))
            .or_else(|| name_lower.strip_prefix("add_"))
            .or_else(|| name_lower.strip_prefix("update_"))
            .or_else(|| name_lower.strip_prefix("edit_"))
            .or_else(|| name_lower.strip_prefix("delete_"))
            .or_else(|| name_lower.strip_prefix("remove_"))
            .or_else(|| name_lower.strip_prefix("search_"))
            .or_else(|| name_lower.strip_prefix("find_"))
            .or_else(|| name_lower.strip_prefix("read_"))
            .or_else(|| name_lower.strip_prefix("write_"))
            .or_else(|| name_lower.strip_prefix("send_"))
            .or_else(|| name_lower.strip_prefix("post_"))
            .or_else(|| name_lower.strip_prefix("fetch_"))
            .or_else(|| name_lower.strip_prefix("download_"))
            .or_else(|| name_lower.strip_prefix("upload_"));

        resource_part.map(|r| {
            // Normalize common abbreviations and singulars to plurals
            match r {
                "issue" => "issues".to_string(),
                "pr" | "pull_request" => "pull_requests".to_string(),
                "repo" | "repository" => "repos".to_string(),
                "branch" => "branches".to_string(),
                "commit" => "commits".to_string(),
                "file" | "files" => "files".to_string(),
                "dir" | "directory" | "folder" => "directories".to_string(),
                "message" | "msg" => "messages".to_string(),
                "channel" => "channels".to_string(),
                "user" => "users".to_string(),
                "team" => "teams".to_string(),
                "workflow" => "workflows".to_string(),
                "action" => "actions".to_string(),
                "release" => "releases".to_string(),
                "tag" => "tags".to_string(),
                "comment" => "comments".to_string(),
                "review" => "reviews".to_string(),
                "label" => "labels".to_string(),
                "milestone" => "milestones".to_string(),
                other => {
                    // Keep as-is, replacing underscores with dots for hierarchy
                    other.replace('_', ".")
                }
            }
        })
    }

    /// Infer category from capability name based on action pattern
    /// Returns categories like "crud.read", "crud.write", "search", "notify"
    pub fn infer_category(capability_name: &str) -> Vec<String> {
        let name_lower = capability_name.to_lowercase();
        let mut categories = Vec::new();

        // Determine CRUD category
        if name_lower.starts_with("list_")
            || name_lower.starts_with("get_")
            || name_lower.starts_with("read_")
            || name_lower.starts_with("fetch_")
            || name_lower.starts_with("download_")
        {
            categories.push("crud.read".to_string());
        } else if name_lower.starts_with("create_")
            || name_lower.starts_with("add_")
            || name_lower.starts_with("new_")
            || name_lower.starts_with("post_")
        {
            categories.push("crud.create".to_string());
        } else if name_lower.starts_with("update_")
            || name_lower.starts_with("edit_")
            || name_lower.starts_with("modify_")
            || name_lower.starts_with("patch_")
        {
            categories.push("crud.update".to_string());
        } else if name_lower.starts_with("delete_")
            || name_lower.starts_with("remove_")
            || name_lower.starts_with("drop_")
        {
            categories.push("crud.delete".to_string());
        }

        // Determine additional categories
        if name_lower.starts_with("search_")
            || name_lower.starts_with("find_")
            || name_lower.contains("_search")
        {
            categories.push("search".to_string());
        }

        if name_lower.starts_with("send_")
            || name_lower.starts_with("notify_")
            || name_lower.contains("_message")
        {
            categories.push("notify".to_string());
        }

        if name_lower.starts_with("transform_")
            || name_lower.starts_with("convert_")
            || name_lower.starts_with("format_")
        {
            categories.push("transform".to_string());
        }

        if name_lower.starts_with("validate_")
            || name_lower.starts_with("check_")
            || name_lower.starts_with("verify_")
        {
            categories.push("validate".to_string());
        }

        if name_lower.starts_with("upload_") || name_lower.contains("_upload") {
            categories.push("upload".to_string());
        }

        if categories.is_empty() {
            categories.push("other".to_string());
        }

        categories
    }

    /// Automatically populate domains and categories based on source and name
    /// Use this when creating a manifest from MCP or other external sources
    pub fn with_inferred_domains_and_categories(mut self, source: &str) -> Self {
        if self.domains.is_empty() {
            self.domains = Self::infer_domains(source, &self.name);
        }
        if self.categories.is_empty() {
            self.categories = Self::infer_category(&self.name);
        }
        self
    }

    /// Get the last updated timestamp from metadata
    pub fn last_updated(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata
            .get("last_updated")
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc))
    }

    /// Set the last updated timestamp in metadata
    pub fn set_last_updated(mut self) -> Self {
        self.metadata
            .insert("last_updated".to_string(), chrono::Utc::now().to_rfc3339());
        self
    }

    /// Get the previous version from metadata
    pub fn previous_version(&self) -> Option<String> {
        self.metadata.get("previous_version").cloned()
    }

    /// Set the previous version in metadata (used when updating)
    pub fn with_previous_version(mut self, previous_version: String) -> Self {
        self.metadata
            .insert("previous_version".to_string(), previous_version);
        self
    }

    /// Get version history from metadata
    pub fn version_history(&self) -> Vec<String> {
        self.metadata
            .get("version_history")
            .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
            .unwrap_or_default()
    }

    /// Add a version to the history
    pub fn add_to_version_history(mut self, version: String) -> Self {
        let mut history = self.version_history();
        if !history.contains(&version) {
            history.push(version);
            self.metadata.insert(
                "version_history".to_string(),
                serde_json::to_string(&history).unwrap_or_default(),
            );
        }
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
    Native(NativeCapability),
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
    pub registry: Arc<RwLock<crate::capabilities::registry::CapabilityRegistry>>,
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
    pub auth_token: Option<String>,
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

#[derive(Clone)]
pub struct NativeCapability {
    pub handler: Arc<dyn Fn(&Value) -> BoxFuture<'static, RuntimeResult<Value>> + Send + Sync>,
    pub security_level: String,
    pub metadata: HashMap<String, String>,
}

impl std::fmt::Debug for NativeCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeCapability")
            .field("security_level", &self.security_level)
            .field("metadata", &self.metadata)
            .finish()
    }
}

impl PartialEq for NativeCapability {
    fn eq(&self, other: &Self) -> bool {
        self.security_level == other.security_level && self.metadata == other.metadata
    }
}

pub struct CapabilityMarketplace {
    pub(crate) capabilities: Arc<RwLock<HashMap<String, CapabilityManifest>>>,
    pub(crate) discovery_agents: Vec<Box<dyn CapabilityDiscovery>>,
    pub(crate) capability_registry: Arc<RwLock<crate::capabilities::registry::CapabilityRegistry>>,
    pub(crate) network_registry: Option<NetworkRegistryConfig>,
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
    /// Optional factory to create a Host for RTFS capability execution (defaults to PureHost)
    pub(crate) rtfs_host_factory: Arc<
        StdRwLock<
            Option<
                Arc<
                    dyn Fn() -> Arc<dyn HostInterface + Send + Sync>
                        + Send
                        + Sync,
                >,
            >,
        >,
    >,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_domains_from_github_server() {
        // Simple server name
        let domains = CapabilityManifest::infer_domains("github", "list_issues");
        assert!(domains.contains(&"github".to_string()));
        assert!(domains.contains(&"github.issues".to_string()));

        // Namespaced server name (MCP registry format)
        let domains =
            CapabilityManifest::infer_domains("modelcontextprotocol/github", "get_pull_request");
        assert!(domains.contains(&"github".to_string()));
        assert!(domains.contains(&"github.pull_requests".to_string()));

        // Server name with suffix
        let domains = CapabilityManifest::infer_domains("github-mcp-server", "create_issue");
        assert!(domains.contains(&"github".to_string()));
        assert!(domains.contains(&"github.issues".to_string()));
    }

    #[test]
    fn test_infer_domains_from_slack_server() {
        let domains = CapabilityManifest::infer_domains("slack", "send_message");
        assert!(domains.contains(&"slack".to_string()));
        assert!(domains.contains(&"slack.messages".to_string()));

        let domains = CapabilityManifest::infer_domains("my-slack-bot", "list_channels");
        assert!(domains.contains(&"slack".to_string()));
        assert!(domains.contains(&"slack.channels".to_string()));
    }

    #[test]
    fn test_infer_domains_from_cloud_providers() {
        let domains = CapabilityManifest::infer_domains("aws-s3", "upload_file");
        assert!(domains.contains(&"cloud.aws".to_string()));
        assert!(domains.contains(&"cloud.aws.files".to_string()));

        let domains = CapabilityManifest::infer_domains("google-cloud", "list_buckets");
        assert!(domains.contains(&"cloud.gcp".to_string()));
    }

    #[test]
    fn test_infer_category_from_capability_name() {
        // Read operations
        let cats = CapabilityManifest::infer_category("list_issues");
        assert!(cats.contains(&"crud.read".to_string()));

        let cats = CapabilityManifest::infer_category("get_user");
        assert!(cats.contains(&"crud.read".to_string()));

        // Create operations
        let cats = CapabilityManifest::infer_category("create_issue");
        assert!(cats.contains(&"crud.create".to_string()));

        // Update operations
        let cats = CapabilityManifest::infer_category("update_issue");
        assert!(cats.contains(&"crud.update".to_string()));

        // Delete operations
        let cats = CapabilityManifest::infer_category("delete_issue");
        assert!(cats.contains(&"crud.delete".to_string()));

        // Search operations
        let cats = CapabilityManifest::infer_category("search_issues");
        assert!(cats.contains(&"search".to_string()));

        // Notify operations
        let cats = CapabilityManifest::infer_category("send_message");
        assert!(cats.contains(&"notify".to_string()));
    }

    #[test]
    fn test_extract_primary_domain() {
        assert_eq!(
            CapabilityManifest::extract_primary_domain("github"),
            "github"
        );
        assert_eq!(
            CapabilityManifest::extract_primary_domain("modelcontextprotocol/github"),
            "github"
        );
        assert_eq!(
            CapabilityManifest::extract_primary_domain("GitHub-MCP-Server"),
            "github"
        );
        assert_eq!(
            CapabilityManifest::extract_primary_domain("my-postgres-db"),
            "database"
        );
        assert_eq!(
            CapabilityManifest::extract_primary_domain("aws-lambda"),
            "cloud.aws"
        );
    }

    #[test]
    fn test_extract_sub_domain() {
        assert_eq!(
            CapabilityManifest::extract_sub_domain("list_issues"),
            Some("issues".to_string())
        );
        assert_eq!(
            CapabilityManifest::extract_sub_domain("get_pull_request"),
            Some("pull_requests".to_string())
        );
        assert_eq!(
            CapabilityManifest::extract_sub_domain("get_pr"),
            Some("pull_requests".to_string())
        );
        assert_eq!(
            CapabilityManifest::extract_sub_domain("send_message"),
            Some("messages".to_string())
        );
        assert_eq!(
            CapabilityManifest::extract_sub_domain("create_branch"),
            Some("branches".to_string())
        );
        assert_eq!(
            CapabilityManifest::extract_sub_domain("some_random_thing"),
            None
        );
    }

    #[test]
    fn test_with_inferred_domains_and_categories() {
        let manifest = CapabilityManifest::new(
            "mcp.github.list_issues".to_string(),
            "list_issues".to_string(),
            "List GitHub issues".to_string(),
            ProviderType::MCP(MCPCapability {
                server_url: "http://localhost:8080".to_string(),
                tool_name: "list_issues".to_string(),
                timeout_ms: 30000,
                auth_token: None,
            }),
            "1.0.0".to_string(),
        )
        .with_inferred_domains_and_categories("modelcontextprotocol/github");

        assert!(manifest.domains.contains(&"github".to_string()));
        assert!(manifest.domains.contains(&"github.issues".to_string()));
        assert!(manifest.categories.contains(&"crud.read".to_string()));
    }
}
