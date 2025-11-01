use super::executors::CapabilityExecutor;
use super::executors::{
    A2AExecutor, ExecutorVariant, HttpExecutor, LocalExecutor, MCPExecutor, OpenApiExecutor,
    RegistryExecutor,
};
use super::resource_monitor::ResourceMonitor;
use super::types::*;
// Temporarily disabled to fix resource monitoring tests
// use super::network_discovery::{NetworkDiscoveryProvider, NetworkDiscoveryBuilder};
// use super::mcp_discovery::{MCPDiscoveryProvider, MCPDiscoveryBuilder, MCPServerConfig};
// use super::a2a_discovery::{A2ADiscoveryProvider, A2ADiscoveryBuilder, A2AAgentConfig};
use rtfs::ast::{MapKey, TypeExpr};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
// RuntimeContext no longer needed in missing-capability path
use super::mcp_discovery::{MCPDiscoveryProvider, MCPServerConfig};
use crate::synthesis::schema_serializer::type_expr_to_rtfs_pretty;
use crate::streaming::{McpStreamingProvider, StreamConfig, StreamHandle, StreamType, StreamingProvider};
use rtfs::runtime::type_validator::{TypeCheckingConfig, TypeValidator, VerificationContext};
use rtfs::runtime::values::Value;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Serializable representation of ProviderType (subset of variants without closures)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SerializableProvider {
    Http {
        base_url: String,
        timeout_ms: u64,
        auth_token: Option<String>,
    },
    OpenApi {
        base_url: String,
        spec_url: Option<String>,
        timeout_ms: u64,
        operations: Vec<OpenApiOperation>,
        auth: Option<OpenApiAuth>,
    },
    Mcp {
        server_url: String,
        tool_name: String,
        timeout_ms: u64,
    },
    A2a {
        agent_id: String,
        endpoint: String,
        protocol: String,
        timeout_ms: u64,
    },
    RemoteRtfs {
        endpoint: String,
        timeout_ms: u64,
        auth_token: Option<String>,
    },
    // Non-serializable variants (Local/Stream/Registry/Plugin) are intentionally omitted
}

impl SerializableProvider {
    fn from_provider(p: &ProviderType) -> Option<Self> {
        match p {
            ProviderType::Http(h) => Some(SerializableProvider::Http {
                base_url: h.base_url.clone(),
                timeout_ms: h.timeout_ms,
                auth_token: h.auth_token.clone(),
            }),
            ProviderType::OpenApi(o) => Some(SerializableProvider::OpenApi {
                base_url: o.base_url.clone(),
                spec_url: o.spec_url.clone(),
                timeout_ms: o.timeout_ms,
                operations: o.operations.clone(),
                auth: o.auth.clone(),
            }),
            ProviderType::MCP(m) => Some(SerializableProvider::Mcp {
                server_url: m.server_url.clone(),
                tool_name: m.tool_name.clone(),
                timeout_ms: m.timeout_ms,
            }),
            ProviderType::A2A(a) => Some(SerializableProvider::A2a {
                agent_id: a.agent_id.clone(),
                endpoint: a.endpoint.clone(),
                protocol: a.protocol.clone(),
                timeout_ms: a.timeout_ms,
            }),
            ProviderType::RemoteRTFS(r) => Some(SerializableProvider::RemoteRtfs {
                endpoint: r.endpoint.clone(),
                timeout_ms: r.timeout_ms,
                auth_token: r.auth_token.clone(),
            }),
            // Skip non-serializable providers
            ProviderType::Local(_)
            | ProviderType::Stream(_)
            | ProviderType::Registry(_)
            | ProviderType::Plugin(_) => None,
        }
    }

    fn into_provider(self) -> ProviderType {
        match self {
            SerializableProvider::Http {
                base_url,
                timeout_ms,
                auth_token,
            } => ProviderType::Http(HttpCapability {
                base_url,
                auth_token,
                timeout_ms,
            }),
            SerializableProvider::OpenApi {
                base_url,
                spec_url,
                timeout_ms,
                operations,
                auth,
            } => ProviderType::OpenApi(OpenApiCapability {
                base_url,
                spec_url,
                operations,
                auth,
                timeout_ms,
            }),
            SerializableProvider::Mcp {
                server_url,
                tool_name,
                timeout_ms,
            } => ProviderType::MCP(MCPCapability {
                server_url,
                tool_name,
                timeout_ms,
            }),
            SerializableProvider::A2a {
                agent_id,
                endpoint,
                protocol,
                timeout_ms,
            } => ProviderType::A2A(A2ACapability {
                agent_id,
                endpoint,
                protocol,
                timeout_ms,
            }),
            SerializableProvider::RemoteRtfs {
                endpoint,
                timeout_ms,
                auth_token,
            } => ProviderType::RemoteRTFS(RemoteRTFSCapability {
                endpoint,
                timeout_ms,
                auth_token,
            }),
        }
    }
}

/// Serializable DTO for capability manifests
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SerializableManifest {
    id: String,
    name: String,
    description: String,
    version: String,
    provider: SerializableProvider,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    input_schema: Option<TypeExpr>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    output_schema: Option<TypeExpr>,
    #[serde(default)]
    permissions: Vec<String>,
    #[serde(default)]
    effects: Vec<String>,
    #[serde(default)]
    metadata: HashMap<String, String>,
}

impl From<&CapabilityManifest> for Option<SerializableManifest> {
    fn from(c: &CapabilityManifest) -> Self {
        let provider = SerializableProvider::from_provider(&c.provider)?;
        Some(SerializableManifest {
            id: c.id.clone(),
            name: c.name.clone(),
            description: c.description.clone(),
            version: c.version.clone(),
            provider,
            input_schema: c.input_schema.clone(),
            output_schema: c.output_schema.clone(),
            permissions: c.permissions.clone(),
            effects: c.effects.clone(),
            metadata: c.metadata.clone(),
        })
    }
}

impl From<SerializableManifest> for CapabilityManifest {
    fn from(s: SerializableManifest) -> Self {
        CapabilityManifest {
            id: s.id,
            name: s.name,
            description: s.description,
            provider: s.provider.into_provider(),
            version: s.version,
            input_schema: s.input_schema,
            output_schema: s.output_schema,
            attestation: None,
            provenance: None,
            permissions: s.permissions,
            effects: s.effects,
            metadata: s.metadata,
            agent_metadata: None,
        }
    }
}

impl CapabilityMarketplace {
    pub fn new(
        capability_registry: Arc<
            RwLock<rtfs::runtime::capabilities::registry::CapabilityRegistry>,
        >,
    ) -> Self {
        Self::with_causal_chain(capability_registry, None)
    }

    pub fn with_causal_chain(
        capability_registry: Arc<
            RwLock<rtfs::runtime::capabilities::registry::CapabilityRegistry>,
        >,
        causal_chain: Option<Arc<std::sync::Mutex<crate::causal_chain::CausalChain>>>,
    ) -> Self {
        Self::with_causal_chain_and_debug_callback(capability_registry, causal_chain, None)
    }

    pub fn with_causal_chain_and_debug_callback(
        capability_registry: Arc<
            RwLock<rtfs::runtime::capabilities::registry::CapabilityRegistry>,
        >,
        causal_chain: Option<Arc<std::sync::Mutex<crate::causal_chain::CausalChain>>>,
        debug_callback: Option<Arc<dyn Fn(String) + Send + Sync>>,
    ) -> Self {
        let mut marketplace = Self {
            capabilities: Arc::new(RwLock::new(HashMap::new())),
            discovery_agents: Vec::new(),
            capability_registry,
            network_registry: None,
            type_validator: Arc::new(TypeValidator::new()),
            executor_registry: HashMap::new(),
            isolation_policy: CapabilityIsolationPolicy::default(),
            causal_chain,
            resource_monitor: None,
            debug_callback,
            session_pool: Arc::new(RwLock::new(None)),
        };
        marketplace.executor_registry.insert(
            TypeId::of::<MCPCapability>(),
            ExecutorVariant::MCP(MCPExecutor),
        );
        marketplace.executor_registry.insert(
            TypeId::of::<A2ACapability>(),
            ExecutorVariant::A2A(A2AExecutor),
        );
        marketplace.executor_registry.insert(
            TypeId::of::<LocalCapability>(),
            ExecutorVariant::Local(LocalExecutor),
        );
        marketplace.executor_registry.insert(
            TypeId::of::<HttpCapability>(),
            ExecutorVariant::Http(HttpExecutor),
        );
        marketplace.executor_registry.insert(
            TypeId::of::<OpenApiCapability>(),
            ExecutorVariant::OpenApi(OpenApiExecutor),
        );
        marketplace.executor_registry.insert(
            TypeId::of::<RegistryCapability>(),
            ExecutorVariant::Registry(RegistryExecutor),
        );
        marketplace
    }

    /// Set a debug callback function to receive debug messages instead of printing to stderr
    pub fn set_debug_callback<F>(&mut self, callback: F)
    where
        F: Fn(String) + Send + Sync + 'static,
    {
        self.debug_callback = Some(Arc::new(callback));
    }

    /// Set session pool for stateful capabilities (generic, provider-agnostic)
    pub async fn set_session_pool(
        &self,
        session_pool: Arc<crate::capabilities::SessionPoolManager>,
    ) {
        *self.session_pool.write().await = Some(session_pool);
    }

    /// Create marketplace with resource monitoring enabled
    pub fn with_resource_monitoring(
        capability_registry: Arc<
            RwLock<rtfs::runtime::capabilities::registry::CapabilityRegistry>,
        >,
        causal_chain: Option<Arc<std::sync::Mutex<crate::causal_chain::CausalChain>>>,
        monitoring_config: ResourceMonitoringConfig,
    ) -> Self {
        let mut marketplace = Self::with_causal_chain(capability_registry, causal_chain);
        marketplace.resource_monitor = Some(Arc::new(ResourceMonitor::new(monitoring_config)));
        marketplace
    }

    /// Set the isolation policy for the marketplace
    pub fn set_isolation_policy(&mut self, policy: CapabilityIsolationPolicy) {
        self.isolation_policy = policy;
    }

    /// Validate if a capability is allowed according to the isolation policy
    fn validate_capability_access(&self, capability_id: &str) -> RuntimeResult<()> {
        // Check time constraints first
        if !self.isolation_policy.check_time_constraints() {
            return Err(RuntimeError::Generic(format!(
                "Capability '{}' access denied due to time constraints",
                capability_id
            )));
        }

        // Check namespace policies
        if !self.isolation_policy.check_namespace_access(capability_id) {
            return Err(RuntimeError::Generic(format!(
                "Capability '{}' access denied by namespace policy",
                capability_id
            )));
        }

        // Check denied patterns first (deny takes precedence)
        for pattern in &self.isolation_policy.denied_capabilities {
            if self.matches_pattern(capability_id, pattern) {
                return Err(RuntimeError::Generic(format!(
                    "Capability '{}' is denied by isolation policy pattern '{}'",
                    capability_id, pattern
                )));
            }
        }

        // Check allowed patterns
        let mut allowed = false;
        for pattern in &self.isolation_policy.allowed_capabilities {
            if self.matches_pattern(capability_id, pattern) {
                allowed = true;
                break;
            }
        }

        if !allowed {
            return Err(RuntimeError::Generic(format!(
                "Capability '{}' is not allowed by isolation policy",
                capability_id
            )));
        }

        Ok(())
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

    /// Bootstrap the marketplace with discovered capabilities from the registry
    /// This method is called during startup to populate the marketplace with
    /// built-in capabilities and any discovered from external sources
    pub async fn bootstrap(&self) -> RuntimeResult<()> {
        // Register default capabilities first
        crate::capabilities::register_default_capabilities(self).await?;

        // Load built-in capabilities from the capability registry
        // Note: RTFS stub registry doesn't have list_capabilities, so we skip this
        // CCOS capabilities are registered through register_default_capabilities instead
        /*
        let registry = self.capability_registry.read().await;

        // Get all registered capabilities from the registry
        for capability_id in registry.list_capabilities() {
            let capability_id = capability_id.to_string();
            let provenance = CapabilityProvenance {
                source: "registry_bootstrap".to_string(),
                version: Some("1.0.0".to_string()),
                content_hash: self.compute_content_hash(&format!("registry:{}", capability_id)),
                custody_chain: vec!["registry_bootstrap".to_string()],
                registered_at: Utc::now(),
            };

            let manifest = CapabilityManifest {
                id: capability_id.clone(),
                name: capability_id.clone(),
                description: format!("Registry capability: {}", capability_id),
                provider: ProviderType::Registry(RegistryCapability {
                    capability_id: capability_id.clone(),
                    registry: Arc::clone(&self.capability_registry),
                }),
                version: "1.0.0".to_string(),
                input_schema: None,
                output_schema: None,
                attestation: None,
                provenance: Some(provenance),
                permissions: vec![],
                effects: vec![],
                metadata: HashMap::new(),
                agent_metadata: None,
            };

            let mut caps = self.capabilities.write().await;
            caps.insert(capability_id.clone(), manifest);
        }
        */

        // Run discovery agents to find additional capabilities
        for agent in &self.discovery_agents {
            match agent.discover().await {
                Ok(discovered_capabilities) => {
                    let mut caps = self.capabilities.write().await;
                    for capability in discovered_capabilities {
                        caps.insert(capability.id.clone(), capability);
                    }
                }
                Err(e) => {
                    // Log discovery errors but don't fail bootstrap
                    eprintln!("Discovery agent failed: {:?}", e);
                }
            }
        }

        Ok(())
    }

    /// Add a discovery agent for dynamic capability discovery
    pub fn add_discovery_agent(&mut self, agent: Box<dyn CapabilityDiscovery>) {
        self.discovery_agents.push(agent);
    }

    // Temporarily disabled to fix resource monitoring tests
    /*
    /// Add a network discovery provider to the marketplace
    pub fn add_network_discovery(&mut self, config: NetworkDiscoveryBuilder) -> RuntimeResult<()> {
        let provider = config.build()?;
        self.discovery_agents.push(Box::new(provider));
        Ok(())
    }
    */

    /*
    /// Add a network discovery provider using the builder pattern
    pub fn add_network_discovery_builder(&mut self, builder: NetworkDiscoveryBuilder) -> RuntimeResult<()> {
        let provider = builder.build()?;
        self.discovery_agents.push(Box::new(provider));
        Ok(())
    }

    /// Add an MCP discovery provider
    pub fn add_mcp_discovery(&mut self, config: MCPServerConfig) -> RuntimeResult<()> {
        let provider = MCPDiscoveryProvider::new(config)?;
        self.discovery_agents.push(Box::new(provider));
        Ok(())
    }

    /// Add an MCP discovery provider using the builder pattern
    pub fn add_mcp_discovery_builder(&mut self, builder: MCPDiscoveryBuilder) -> RuntimeResult<()> {
        let provider = builder.build()?;
        self.discovery_agents.push(Box::new(provider));
        Ok(())
    }

    /// Add an A2A discovery provider
    pub fn add_a2a_discovery(&mut self, config: A2AAgentConfig) -> RuntimeResult<()> {
        let provider = A2ADiscoveryProvider::new(config)?;
        self.discovery_agents.push(Box::new(provider));
        Ok(())
    }

    /// Add an A2A discovery provider using the builder pattern
    pub fn add_a2a_discovery_builder(&mut self, builder: A2ADiscoveryBuilder) -> RuntimeResult<()> {
        let provider = builder.build()?;
        self.discovery_agents.push(Box::new(provider));
        Ok(())
    }

    /// Discover capabilities from all configured network sources
    pub async fn discover_from_network(&self) -> RuntimeResult<Vec<CapabilityManifest>> {
        let mut all_capabilities = Vec::new();

        for agent in &self.discovery_agents {
            if agent.name() == "NetworkDiscovery" {
                match agent.discover().await {
                    Ok(capabilities) => {
                        eprintln!("Discovered {} capabilities from network source", capabilities.len());
                        all_capabilities.extend(capabilities);
                    }
                    Err(e) => {
                        eprintln!("Network discovery failed: {}", e);
                        // Continue with other discovery agents even if one fails
                    }
                }
            }
        }

        Ok(all_capabilities)
    }

    /// Perform health checks on all network discovery providers
    pub async fn check_network_health(&self) -> RuntimeResult<HashMap<String, bool>> {
        let mut health_status = HashMap::new();

        for agent in &self.discovery_agents {
            if agent.name() == "NetworkDiscovery" {
                // Try to downcast to NetworkDiscoveryProvider for health check
                if let Some(network_provider) = agent.as_any().downcast_ref::<NetworkDiscoveryProvider>() {
                    match network_provider.health_check().await {
                        Ok(is_healthy) => {
                            health_status.insert(agent.name().to_string(), is_healthy);
                        }
                        Err(_) => {
                            health_status.insert(agent.name().to_string(), false);
                        }
                    }
                } else {
                    health_status.insert(agent.name().to_string(), false);
                }
            }
        }

        Ok(health_status)
    }
    */

    /// Get the count of registered capabilities
    pub async fn capability_count(&self) -> usize {
        let capabilities = self.capabilities.read().await;
        capabilities.len()
    }

    /// Check if a capability exists
    pub async fn has_capability(&self, id: &str) -> bool {
        let capabilities = self.capabilities.read().await;
        capabilities.contains_key(id)
    }

    fn compute_content_hash(&self, content: &str) -> String {
        super::discovery::compute_content_hash(content)
    }

    pub async fn register_streaming_capability(
        &self,
        id: String,
        name: String,
        description: String,
        stream_type: StreamType,
        provider: StreamingProvider,
        input_schema: Option<TypeExpr>,
        output_schema: Option<TypeExpr>,
        effects: Vec<String>,
    ) -> Result<(), RuntimeError> {
        let mut effect_set: HashSet<String> = HashSet::with_capacity(effects.len() + 1);
        effect_set.insert(":streaming".to_string());
        for effect in effects {
            let trimmed = effect.trim();
            if trimmed.is_empty() {
                continue;
            }
            effect_set.insert(trimmed.to_string());
        }
        let mut normalized_effects: Vec<String> = effect_set.into_iter().collect();
        normalized_effects.sort();

        let stream_type_label = match &stream_type {
            StreamType::Unidirectional => "unidirectional",
            StreamType::Bidirectional => "bidirectional",
            StreamType::Duplex => "duplex",
        };

        let provenance = CapabilityProvenance {
            source: "streaming".to_string(),
            version: Some("1.0.0".to_string()),
            content_hash: self.compute_content_hash(&format!("{}{}{}", id, name, description)),
            custody_chain: vec!["streaming_registration".to_string()],
            registered_at: Utc::now(),
        };
        let stream_impl = StreamCapabilityImpl {
            provider,
            stream_type,
            input_schema: input_schema.clone(),
            output_schema: output_schema.clone(),
            supports_progress: true,
            supports_cancellation: true,
            bidirectional_config: None,
            duplex_config: None,
            stream_config: None,
        };
        let mut metadata = HashMap::new();
        metadata.insert("stream_type".to_string(), stream_type_label.to_string());
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::Stream(stream_impl),
            version: "1.0.0".to_string(),
            input_schema,
            output_schema,
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            effects: normalized_effects,
            metadata,
            agent_metadata: None,
        };
        let mut caps = self.capabilities.write().await;
        caps.insert(id, capability);
        Ok(())
    }

    /// Register a local capability with audit logging
    pub async fn register_local_capability(
        &self,
        id: String,
        name: String,
        description: String,
        handler: Arc<dyn Fn(&Value) -> RuntimeResult<Value> + Send + Sync>,
    ) -> RuntimeResult<()> {
        let provenance = CapabilityProvenance {
            source: "local".to_string(),
            version: Some("1.0.0".to_string()),
            content_hash: self.compute_content_hash(&id),
            custody_chain: vec!["local_registration".to_string()],
            registered_at: Utc::now(),
        };

        let manifest = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::Local(LocalCapability { handler }),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            effects: vec![],
            metadata: HashMap::new(),
            agent_metadata: None,
        };

        // Register the capability
        {
            let mut caps = self.capabilities.write().await;
            caps.insert(id.clone(), manifest);
        }

        // Emit audit event to Causal Chain
        self.emit_capability_audit_event("capability_registered", &id, None)
            .await?;

        Ok(())
    }

    /// Register a capability manifest directly (for testing and advanced use cases)
    pub async fn register_capability_manifest(
        &self,
        manifest: CapabilityManifest,
    ) -> RuntimeResult<()> {
        let id = manifest.id.clone();

        // Register the capability
        {
            let mut caps = self.capabilities.write().await;
            caps.insert(id.clone(), manifest);
        }

        // Emit audit event to Causal Chain
        self.emit_capability_audit_event("capability_registered", &id, None)
            .await?;

        Ok(())
    }

    /// Remove a capability with audit logging
    pub async fn remove_capability(&self, id: &str) -> RuntimeResult<()> {
        let was_present = {
            let mut caps = self.capabilities.write().await;
            caps.remove(id).is_some()
        };

        if was_present {
            // Emit audit event to Causal Chain
            self.emit_capability_audit_event("capability_removed", id, None)
                .await?;
        }

        Ok(())
    }

    /// Emit audit event to Causal Chain
    pub async fn emit_capability_audit_event(
        &self,
        event_type: &str,
        capability_id: &str,
        additional_data: Option<HashMap<String, String>>,
    ) -> RuntimeResult<()> {
        let mut event_data = HashMap::new();
        event_data.insert("event_type".to_string(), event_type.to_string());
        event_data.insert("capability_id".to_string(), capability_id.to_string());
        event_data.insert("timestamp".to_string(), Utc::now().to_rfc3339());

        if let Some(additional) = additional_data {
            event_data.extend(additional);
        }

        // Log the audit event using callback or fallback to stderr
        let audit_message = format!("CAPABILITY_AUDIT: {:?}", event_data);
        if let Some(callback) = &self.debug_callback {
            callback(audit_message);
        } else {
            eprintln!("{}", audit_message);
        }

        // Record in Causal Chain if available
        if let Some(causal_chain) = &self.causal_chain {
            let action_type = match event_type {
                "capability_registered" => crate::types::ActionType::CapabilityRegistered,
                "capability_removed" => crate::types::ActionType::CapabilityRemoved,
                "capability_updated" => crate::types::ActionType::CapabilityUpdated,
                "capability_discovery_completed" => {
                    crate::types::ActionType::CapabilityDiscoveryCompleted
                }
                _ => crate::types::ActionType::CapabilityCall, // fallback
            };

            let action = crate::types::Action {
                action_id: uuid::Uuid::new_v4().to_string(),
                intent_id: "capability_marketplace".to_string(), // Use placeholder for capability events
                plan_id: "capability_marketplace".to_string(), // Use placeholder for capability events
                action_type,
                parent_action_id: None,
                function_name: Some(capability_id.to_string()),
                arguments: None,
                result: None,
                cost: None,
                duration_ms: None,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                metadata: {
                    let mut meta = HashMap::new();
                    meta.insert(
                        "capability_id".to_string(),
                        rtfs::runtime::values::Value::String(capability_id.to_string()),
                    );
                    meta.insert(
                        "event_type".to_string(),
                        rtfs::runtime::values::Value::String(event_type.to_string()),
                    );
                    for (k, v) in event_data {
                        meta.insert(k, rtfs::runtime::values::Value::String(v));
                    }
                    meta
                },
            };

            if let Ok(mut chain) = causal_chain.lock() {
                if let Err(e) = chain.append(&action) {
                    eprintln!(
                        "Failed to record capability audit event in Causal Chain: {:?}",
                        e
                    );
                }
            } else {
                eprintln!("Failed to acquire lock on Causal Chain");
            }
        }

        Ok(())
    }

    pub async fn register_local_capability_with_schema(
        &self,
        id: String,
        name: String,
        description: String,
        handler: Arc<dyn Fn(&Value) -> RuntimeResult<Value> + Send + Sync>,
        input_schema: Option<TypeExpr>,
        output_schema: Option<TypeExpr>,
    ) -> Result<(), RuntimeError> {
        let provenance = CapabilityProvenance {
            source: "local".to_string(),
            version: Some("1.0.0".to_string()),
            content_hash: self.compute_content_hash(&format!("{}{}{}", id, name, description)),
            custody_chain: vec!["local_registration".to_string()],
            registered_at: chrono::Utc::now(),
        };
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::Local(LocalCapability { handler }),
            version: "1.0.0".to_string(),
            input_schema,
            output_schema,
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            effects: vec![],
            metadata: HashMap::new(),
            agent_metadata: None,
        };
        let mut caps = self.capabilities.write().await;
        caps.insert(id, capability);
        Ok(())
    }

    /// Register a local capability with schema and metadata
    ///
    /// Generic method that works for any provider type (MCP, OpenAPI, etc.)
    /// The metadata HashMap can contain provider-specific fields flattened from
    /// hierarchical RTFS structure.
    pub async fn register_local_capability_with_metadata(
        &self,
        id: String,
        name: String,
        description: String,
        handler: Arc<dyn Fn(&Value) -> RuntimeResult<Value> + Send + Sync>,
        input_schema: Option<TypeExpr>,
        output_schema: Option<TypeExpr>,
        metadata: HashMap<String, String>,
    ) -> Result<(), RuntimeError> {
        let provenance = CapabilityProvenance {
            source: "local".to_string(),
            version: Some("1.0.0".to_string()),
            content_hash: self.compute_content_hash(&format!("{}{}{}", id, name, description)),
            custody_chain: vec!["local_registration".to_string()],
            registered_at: chrono::Utc::now(),
        };
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::Local(LocalCapability { handler }),
            version: "1.0.0".to_string(),
            input_schema,
            output_schema,
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            effects: vec![],
            metadata, // Provider-specific metadata (generic)
            agent_metadata: None,
        };
        let mut caps = self.capabilities.write().await;
        caps.insert(id, capability);
        Ok(())
    }

    pub async fn register_http_capability(
        &self,
        id: String,
        name: String,
        description: String,
        base_url: String,
        auth_token: Option<String>,
    ) -> Result<(), RuntimeError> {
        let provenance = CapabilityProvenance {
            source: format!("http:{}", base_url),
            version: Some("1.0.0".to_string()),
            content_hash: self
                .compute_content_hash(&format!("{}{}{}{}", id, name, description, base_url)),
            custody_chain: vec!["http_registration".to_string()],
            registered_at: chrono::Utc::now(),
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
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            effects: vec![],
            metadata: HashMap::new(),
            agent_metadata: None,
        };
        let mut caps = self.capabilities.write().await;
        caps.insert(id, capability);
        Ok(())
    }

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
        let provenance = CapabilityProvenance {
            source: format!("http:{}", base_url),
            version: Some("1.0.0".to_string()),
            content_hash: self
                .compute_content_hash(&format!("{}{}{}{}", id, name, description, base_url)),
            custody_chain: vec!["http_registration".to_string()],
            registered_at: chrono::Utc::now(),
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
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            effects: vec![],
            metadata: HashMap::new(),
            agent_metadata: None,
        };
        let mut caps = self.capabilities.write().await;
        caps.insert(id, capability);
        Ok(())
    }

    pub async fn register_mcp_capability(
        &self,
        id: String,
        name: String,
        description: String,
        server_url: String,
        tool_name: String,
        timeout_ms: u64,
    ) -> Result<(), RuntimeError> {
        let provenance = CapabilityProvenance {
            source: format!("mcp:{}/{}", server_url, tool_name),
            version: Some("1.0.0".to_string()),
            content_hash: self.compute_content_hash(&format!(
                "{}{}{}{}{}{}",
                id, name, description, server_url, tool_name, timeout_ms
            )),
            custody_chain: vec!["mcp_registration".to_string()],
            registered_at: chrono::Utc::now(),
        };
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::MCP(MCPCapability {
                server_url,
                tool_name,
                timeout_ms,
            }),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            effects: vec![],
            metadata: HashMap::new(),
            agent_metadata: None,
        };
        let mut caps = self.capabilities.write().await;
        caps.insert(id, capability);
        Ok(())
    }

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
        let provenance = CapabilityProvenance {
            source: format!("mcp:{}/{}", server_url, tool_name),
            version: Some("1.0.0".to_string()),
            content_hash: self.compute_content_hash(&format!(
                "{}{}{}{}{}{}",
                id, name, description, server_url, tool_name, timeout_ms
            )),
            custody_chain: vec!["mcp_registration".to_string()],
            registered_at: chrono::Utc::now(),
        };
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::MCP(MCPCapability {
                server_url,
                tool_name,
                timeout_ms,
            }),
            version: "1.0.0".to_string(),
            input_schema,
            output_schema,
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            effects: vec![],
            metadata: HashMap::new(),
            agent_metadata: None,
        };
        let mut caps = self.capabilities.write().await;
        caps.insert(id, capability);
        Ok(())
    }

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
        let provenance = CapabilityProvenance {
            source: format!("a2a:{}@{}", agent_id, endpoint),
            version: Some("1.0.0".to_string()),
            content_hash: self.compute_content_hash(&format!(
                "{}{}{}{}{}{}{}",
                id, name, description, agent_id, endpoint, protocol, timeout_ms
            )),
            custody_chain: vec!["a2a_registration".to_string()],
            registered_at: chrono::Utc::now(),
        };
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::A2A(A2ACapability {
                agent_id,
                endpoint,
                protocol,
                timeout_ms,
            }),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            effects: vec![],
            metadata: HashMap::new(),
            agent_metadata: None,
        };
        let mut caps = self.capabilities.write().await;
        caps.insert(id, capability);
        Ok(())
    }

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
        let provenance = CapabilityProvenance {
            source: format!("a2a:{}@{}", agent_id, endpoint),
            version: Some("1.0.0".to_string()),
            content_hash: self.compute_content_hash(&format!(
                "{}{}{}{}{}{}{}",
                id, name, description, agent_id, endpoint, protocol, timeout_ms
            )),
            custody_chain: vec!["a2a_registration".to_string()],
            registered_at: chrono::Utc::now(),
        };
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::A2A(A2ACapability {
                agent_id,
                endpoint,
                protocol,
                timeout_ms,
            }),
            version: "1.0.0".to_string(),
            input_schema,
            output_schema,
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            effects: vec![],
            metadata: HashMap::new(),
            agent_metadata: None,
        };
        let mut caps = self.capabilities.write().await;
        caps.insert(id, capability);
        Ok(())
    }

    pub async fn register_plugin_capability(
        &self,
        id: String,
        name: String,
        description: String,
        plugin_path: String,
        function_name: String,
    ) -> Result<(), RuntimeError> {
        let provenance = CapabilityProvenance {
            source: format!("plugin:{}#{}", plugin_path, function_name),
            version: Some("1.0.0".to_string()),
            content_hash: self.compute_content_hash(&format!(
                "{}{}{}{}{}",
                id, name, description, plugin_path, function_name
            )),
            custody_chain: vec!["plugin_registration".to_string()],
            registered_at: chrono::Utc::now(),
        };
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::Plugin(PluginCapability {
                plugin_path,
                function_name,
            }),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            effects: vec![],
            metadata: HashMap::new(),
            agent_metadata: None,
        };
        let mut caps = self.capabilities.write().await;
        caps.insert(id, capability);
        Ok(())
    }

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
        let provenance = CapabilityProvenance {
            source: format!("plugin:{}#{}", plugin_path, function_name),
            version: Some("1.0.0".to_string()),
            content_hash: self.compute_content_hash(&format!(
                "{}{}{}{}{}",
                id, name, description, plugin_path, function_name
            )),
            custody_chain: vec!["plugin_registration".to_string()],
            registered_at: chrono::Utc::now(),
        };
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::Plugin(PluginCapability {
                plugin_path,
                function_name,
            }),
            version: "1.0.0".to_string(),
            input_schema,
            output_schema,
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            effects: vec![],
            metadata: HashMap::new(),
            agent_metadata: None,
        };
        let mut caps = self.capabilities.write().await;
        caps.insert(id, capability);
        Ok(())
    }

    pub async fn register_remote_rtfs_capability(
        &self,
        id: String,
        name: String,
        description: String,
        endpoint: String,
        auth_token: Option<String>,
        timeout_ms: u64,
    ) -> Result<(), RuntimeError> {
        let provenance = CapabilityProvenance {
            source: format!("remote-rtfs:{}", endpoint),
            version: Some("1.0.0".to_string()),
            content_hash: self.compute_content_hash(&format!(
                "{}{}{}{}{}",
                id, name, description, endpoint, timeout_ms
            )),
            custody_chain: vec!["remote_rtfs_registration".to_string()],
            registered_at: chrono::Utc::now(),
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
            effects: vec![],
            metadata: HashMap::new(),
            agent_metadata: None,
        };
        let mut caps = self.capabilities.write().await;
        caps.insert(id, capability);
        Ok(())
    }

    pub async fn start_stream_with_config(
        &self,
        capability_id: &str,
        params: &Value,
        config: &StreamConfig,
    ) -> RuntimeResult<StreamHandle> {
        let capability = self.get_capability(capability_id).await.ok_or_else(|| {
            RuntimeError::Generic(format!("Capability '{}' not found", capability_id))
        })?;
        if let ProviderType::Stream(stream_impl) = &capability.provider {
            if config.callbacks.is_some() {
                stream_impl
                    .provider
                    .start_stream_with_config(params, config)
                    .await
            } else {
                let handle = stream_impl.provider.start_stream(params)?;
                Ok(handle)
            }
        } else {
            Err(RuntimeError::Generic(format!(
                "Capability '{}' is not a stream capability",
                capability_id
            )))
        }
    }

    pub async fn start_bidirectional_stream_with_config(
        &self,
        capability_id: &str,
        params: &Value,
        config: &StreamConfig,
    ) -> RuntimeResult<StreamHandle> {
        let capability = self.get_capability(capability_id).await.ok_or_else(|| {
            RuntimeError::Generic(format!("Capability '{}' not found", capability_id))
        })?;
        if let ProviderType::Stream(stream_impl) = &capability.provider {
            if !matches!(stream_impl.stream_type, StreamType::Bidirectional) {
                return Err(RuntimeError::Generic(format!(
                    "Capability '{}' is not bidirectional",
                    capability_id
                )));
            }
            if config.callbacks.is_some() {
                stream_impl
                    .provider
                    .start_bidirectional_stream_with_config(params, config)
                    .await
            } else {
                let handle = stream_impl.provider.start_bidirectional_stream(params)?;
                Ok(handle)
            }
        } else {
            Err(RuntimeError::Generic(format!(
                "Capability '{}' is not a stream capability",
                capability_id
            )))
        }
    }

    pub async fn get_capability(&self, id: &str) -> Option<CapabilityManifest> {
        let capabilities = self.capabilities.read().await;
        capabilities.get(id).cloned()
    }

    pub async fn list_capabilities(&self) -> Vec<CapabilityManifest> {
        let capabilities = self.capabilities.read().await;
        capabilities.values().cloned().collect()
    }

    /// List capabilities with query filters
    pub async fn list_capabilities_with_query(
        &self,
        query: &CapabilityQuery,
    ) -> Vec<CapabilityManifest> {
        let capabilities = self.capabilities.read().await;
        let mut results: Vec<CapabilityManifest> = capabilities
            .values()
            .filter(|manifest| query.matches(manifest))
            .cloned()
            .collect();

        // Apply limit if specified
        if let Some(limit) = query.limit {
            results.truncate(limit);
        }

        results
    }

    /// List only agent capabilities
    pub async fn list_agents(&self) -> Vec<CapabilityManifest> {
        self.list_capabilities_with_query(&CapabilityQuery::new().agents_only())
            .await
    }

    /// List only primitive capabilities
    pub async fn list_primitives(&self) -> Vec<CapabilityManifest> {
        self.list_capabilities_with_query(&CapabilityQuery::new().primitives_only())
            .await
    }

    /// List only composite capabilities
    pub async fn list_composites(&self) -> Vec<CapabilityManifest> {
        self.list_capabilities_with_query(&CapabilityQuery::new().composites_only())
            .await
    }

    /// Search for capabilities by ID pattern
    pub async fn search_by_id(&self, pattern: &str) -> Vec<CapabilityManifest> {
        self.list_capabilities_with_query(
            &CapabilityQuery::new().with_id_pattern(pattern.to_string()),
        )
        .await
    }

    /// Execute a capability with enhanced metadata support
    pub async fn execute_capability_enhanced(
        &self,
        id: &str,
        inputs: &Value,
        _metadata: Option<&rtfs::runtime::execution_outcome::CallMetadata>,
    ) -> RuntimeResult<Value> {
        // For now, delegate to the existing method
        // TODO: Use metadata for enhanced execution context
        self.execute_capability(id, inputs).await
    }

    // execute_effect_request removed - unified into execute_capability_enhanced

    pub async fn execute_capability(&self, id: &str, inputs: &Value) -> RuntimeResult<Value> {
        // Validate capability access according to isolation policy
        self.validate_capability_access(id)?;

        // Check resource constraints before execution
        if let Some(resource_monitor) = &self.resource_monitor {
            if let Some(constraints) = &self.isolation_policy.resource_constraints {
                let violations = resource_monitor.check_violations(id, constraints).await?;

                // Check for hard violations that should prevent execution
                let hard_violations: Vec<_> = violations
                    .iter()
                    .filter(|v| v.is_hard_violation())
                    .collect();

                if !hard_violations.is_empty() {
                    let violation_details: Vec<String> =
                        hard_violations.iter().map(|v| v.to_string()).collect();
                    return Err(RuntimeError::Generic(format!(
                        "Resource constraints violated for capability '{}': {}",
                        id,
                        violation_details.join(", ")
                    )));
                }

                // Log soft violations but continue execution
                let soft_violations: Vec<_> = violations
                    .iter()
                    .filter(|v| !v.is_hard_violation())
                    .collect();

                for violation in soft_violations {
                    eprintln!(
                        "Soft resource violation for capability '{}': {}",
                        id,
                        violation.to_string()
                    );
                }
            }
        }

        // Fetch manifest or fall back to registry execution
        let manifest_opt = { self.capabilities.read().await.get(id).cloned() };
        let manifest = if let Some(m) = manifest_opt {
            m
        } else {
            let registry = self.capability_registry.read().await;
            // Extract arguments from the input Value::List if it's a list, otherwise wrap in vector
            let args = match inputs {
                Value::List(list) => list.clone(),
                _ => vec![inputs.clone()],
            };
            // If capability not registered locally, surface a clear error
            // Note: RTFS stub registry doesn't support enqueue_missing_capability
            // TODO: Implement missing capability resolution at CCOS level
            return Err(RuntimeError::UnknownCapability(id.to_string()));
        };

        // Check for session management requirements (generic, metadata-driven)
        // This works for ANY provider that declares session needs via metadata
        if !manifest.metadata.is_empty() {
            // Check if capability requires session management (generic pattern)
            let requires_session = manifest
                .metadata
                .iter()
                .any(|(k, v)| k.ends_with("_requires_session") && (v == "true" || v == "auto"));

            if requires_session {
                eprintln!(
                    "📋 Metadata indicates session management required for: {}",
                    id
                );

                // Delegate to session pool for session-managed execution
                let pool_opt = {
                    let guard = self.session_pool.read().await;
                    guard.clone() // Clone the Arc<SessionPoolManager>
                };

                if let Some(pool) = pool_opt {
                    eprintln!("🔄 Delegating to session pool for session management");
                    let args = match inputs {
                        Value::List(list) => list.clone(),
                        _ => vec![inputs.clone()],
                    };

                    // Session pool will:
                    // 1. Detect provider type from metadata (mcp_, graphql_, etc.)
                    // 2. Route to appropriate SessionHandler
                    // 3. Handler initializes/reuses session
                    // 4. Handler executes with session (auth, headers, etc.)
                    // 5. Returns result
                    return pool.execute_with_session(id, &manifest.metadata, &args);
                } else {
                    eprintln!("⚠️  Session management required but no session pool configured");
                    eprintln!("   Falling through to normal execution (will likely fail with 401)");
                }
            }
        }

        // Prepare boundary verification context
        let boundary_context = VerificationContext::capability_boundary(id);
        let type_config = TypeCheckingConfig::default();

        // Validate inputs if a schema is provided
        if let Some(input_schema) = &manifest.input_schema {
            self.type_validator
                .validate_with_config(inputs, input_schema, &type_config, &boundary_context)
                .map_err(|e| RuntimeError::Generic(format!("Input validation failed: {}", e)))?;
        }

        // Execute via executor registry or provider fallback
        let exec_result = if let Some(executor) =
            self.executor_registry.get(&match &manifest.provider {
                ProviderType::Local(_) => std::any::TypeId::of::<LocalCapability>(),
                ProviderType::Http(_) => std::any::TypeId::of::<HttpCapability>(),
                ProviderType::MCP(_) => std::any::TypeId::of::<MCPCapability>(),
                ProviderType::A2A(_) => std::any::TypeId::of::<A2ACapability>(),
                ProviderType::OpenApi(_) => std::any::TypeId::of::<OpenApiCapability>(),
                ProviderType::Plugin(_) => std::any::TypeId::of::<PluginCapability>(),
                ProviderType::RemoteRTFS(_) => std::any::TypeId::of::<RemoteRTFSCapability>(),
                ProviderType::Stream(_) => std::any::TypeId::of::<StreamCapabilityImpl>(),
                ProviderType::Registry(_) => std::any::TypeId::of::<RegistryCapability>(),
            }) {
            executor.execute(&manifest.provider, inputs).await
        } else {
            match &manifest.provider {
                ProviderType::Local(local) => (local.handler)(inputs),
                ProviderType::Http(http) => self.execute_http_capability(http, inputs).await,
                ProviderType::OpenApi(_) => {
                    let executor = OpenApiExecutor;
                    executor.execute(&manifest.provider, inputs).await
                }
                ProviderType::MCP(_mcp) => {
                    Err(RuntimeError::Generic("MCP not configured".to_string()))
                }
                ProviderType::A2A(_a2a) => {
                    Err(RuntimeError::Generic("A2A not configured".to_string()))
                }
                ProviderType::Plugin(_p) => {
                    Err(RuntimeError::Generic("Plugin not configured".to_string()))
                }
                ProviderType::RemoteRTFS(_r) => Err(RuntimeError::Generic(
                    "Remote RTFS not configured".to_string(),
                )),
                ProviderType::Stream(stream_impl) => {
                    self.execute_stream_capability(stream_impl, inputs).await
                }
                ProviderType::Registry(_) => Err(RuntimeError::Generic(
                    "Registry provider missing executor".to_string(),
                )),
            }
        }?;

        // Validate outputs if a schema is provided
        if let Some(output_schema) = &manifest.output_schema {
            self.type_validator
                .validate_with_config(&exec_result, output_schema, &type_config, &boundary_context)
                .map_err(|e| RuntimeError::Generic(format!("Output validation failed: {}", e)))?;
        }

        // Monitor resources after execution
        if let Some(resource_monitor) = &self.resource_monitor {
            if let Some(constraints) = &self.isolation_policy.resource_constraints {
                // This will log any violations that occurred during execution
                let _violations = resource_monitor.check_violations(id, constraints).await;
            }
        }

        Ok(exec_result)
    }

    async fn execute_stream_capability(
        &self,
        stream_impl: &StreamCapabilityImpl,
        inputs: &Value,
    ) -> RuntimeResult<Value> {
        let handle = stream_impl.provider.start_stream(inputs)?;
        if let Some(schema_aware) = stream_impl
            .provider
            .as_any()
            .downcast_ref::<McpStreamingProvider>()
        {
            schema_aware.set_processor_schema(&handle.stream_id, stream_impl.output_schema.clone());
        }
        Ok(Value::String(format!(
            "Stream started with ID: {}",
            handle.stream_id
        )))
    }

    async fn execute_http_capability(
        &self,
        http: &HttpCapability,
        inputs: &Value,
    ) -> RuntimeResult<Value> {
        let args = match inputs {
            Value::List(list) => list.clone(),
            Value::Vector(vec) => vec.clone(),
            v => vec![v.clone()],
        };
        let url = args
            .get(0)
            .and_then(|v| v.as_string())
            .unwrap_or(&http.base_url);
        let method = args.get(1).and_then(|v| v.as_string()).unwrap_or("GET");
        let default_headers = std::collections::HashMap::new();
        let headers = args
            .get(2)
            .and_then(|v| match v {
                Value::Map(m) => Some(m),
                _ => None,
            })
            .unwrap_or(&default_headers);
        let body = args
            .get(3)
            .and_then(|v| v.as_string())
            .unwrap_or("")
            .to_string();
        let client = reqwest::Client::new();
        let method_enum =
            reqwest::Method::from_bytes(method.as_bytes()).unwrap_or(reqwest::Method::GET);
        let mut req = client.request(method_enum, url);
        if let Some(token) = &http.auth_token {
            req = req.bearer_auth(token);
        }
        for (k, v) in headers.iter() {
            if let MapKey::String(ref key) = k {
                if let Value::String(ref val) = v {
                    req = req.header(key, val);
                }
            }
        }
        if !body.is_empty() {
            req = req.body(body);
        }
        let response = req
            .timeout(std::time::Duration::from_millis(http.timeout_ms))
            .send()
            .await
            .map_err(|e| RuntimeError::Generic(format!("HTTP request failed: {}", e)))?;
        let status = response.status().as_u16() as i64;
        let response_headers = response.headers().clone();
        let resp_body = response.text().await.unwrap_or_default();
        let mut response_map = std::collections::HashMap::new();
        response_map.insert(MapKey::String("status".to_string()), Value::Integer(status));
        response_map.insert(MapKey::String("body".to_string()), Value::String(resp_body));
        let mut headers_map = std::collections::HashMap::new();
        for (key, value) in response_headers.iter() {
            headers_map.insert(
                MapKey::String(key.to_string()),
                Value::String(value.to_str().unwrap_or("").to_string()),
            );
        }
        response_map.insert(
            MapKey::String("headers".to_string()),
            Value::Map(headers_map),
        );
        Ok(Value::Map(response_map))
    }

    pub async fn execute_with_validation(
        &self,
        capability_id: &str,
        params: &HashMap<String, Value>,
    ) -> Result<Value, RuntimeError> {
        let config = TypeCheckingConfig::default();
        self.execute_with_validation_config(capability_id, params, &config)
            .await
    }

    pub async fn execute_with_validation_config(
        &self,
        capability_id: &str,
        params: &HashMap<String, Value>,
        config: &TypeCheckingConfig,
    ) -> Result<Value, RuntimeError> {
        let capability = {
            let capabilities = self.capabilities.read().await;
            capabilities.get(capability_id).cloned().ok_or_else(|| {
                RuntimeError::Generic(format!("Capability not found: {}", capability_id))
            })?
        };
        let boundary_context = VerificationContext::capability_boundary(capability_id);
        // Special-case: if the input schema is a primitive (or any non-map) type AND the caller provided exactly
        // one parameter (commonly named "input"), we treat the inner value directly instead of a map wrapper.
        // This makes test ergonomics nicer and matches intuitive single-argument invocation semantics.
        let mut direct_primitive_input: Option<Value> = None;
        if let Some(input_schema) = &capability.input_schema {
            let is_single = params.len() == 1;
            // Currently only Map { .. } represents structured map inputs. Any other variant is treated as primitive/single value.
            let is_non_map_schema = !matches!(input_schema, TypeExpr::Map { .. });
            if is_single && is_non_map_schema {
                if let Some((_k, v)) = params.iter().next() {
                    // ignore key name, just use the value
                    // Validate directly against schema
                    self.type_validator
                        .validate_with_config(v, input_schema, config, &boundary_context)
                        .map_err(|e| {
                            RuntimeError::Generic(format!("Input validation failed: {}", e))
                        })?;
                    direct_primitive_input = Some(v.clone());
                }
            } else {
                // Fallback to original map-based validation path
                self.validate_input_schema_optimized(
                    params,
                    input_schema,
                    config,
                    &boundary_context,
                )
                .await?;
            }
        }

        let inputs_value = if let Some(v) = direct_primitive_input {
            v
        } else {
            self.params_to_value(params)?
        };
        let result = self
            .execute_capability(capability_id, &inputs_value)
            .await?;
        if let Some(output_schema) = &capability.output_schema {
            self.validate_output_schema_optimized(
                &result,
                output_schema,
                config,
                &boundary_context,
            )
            .await?;
        }
        Ok(result)
    }

    async fn validate_input_schema_optimized(
        &self,
        params: &HashMap<String, Value>,
        schema_expr: &TypeExpr,
        config: &TypeCheckingConfig,
        context: &VerificationContext,
    ) -> Result<(), RuntimeError> {
        let params_value = self.params_to_value(params)?;
        self.type_validator
            .validate_with_config(&params_value, schema_expr, config, context)
            .map_err(|e| RuntimeError::Generic(format!("Input validation failed: {}", e)))?;
        Ok(())
    }

    async fn validate_output_schema_optimized(
        &self,
        result: &Value,
        schema_expr: &TypeExpr,
        config: &TypeCheckingConfig,
        context: &VerificationContext,
    ) -> Result<(), RuntimeError> {
        self.type_validator
            .validate_with_config(result, schema_expr, config, context)
            .map_err(|e| RuntimeError::Generic(format!("Output validation failed: {}", e)))?;
        Ok(())
    }

    fn params_to_value(&self, params: &HashMap<String, Value>) -> Result<Value, RuntimeError> {
        let mut map = HashMap::new();
        for (key, value) in params {
            let map_key = if key.starts_with(':') {
                MapKey::Keyword(rtfs::ast::Keyword(key[1..].to_string()))
            } else {
                MapKey::String(key.clone())
            };
            map.insert(map_key, value.clone());
        }
        Ok(Value::Map(map))
    }

    pub fn json_to_rtfs_value(json: &serde_json::Value) -> RuntimeResult<Value> {
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
                let values: Result<Vec<Value>, RuntimeError> =
                    arr.iter().map(Self::json_to_rtfs_value).collect();
                Ok(Value::Vector(values?))
            }
            serde_json::Value::Object(obj) => {
                let mut map = HashMap::new();
                for (key, value) in obj {
                    let rtfs_key = MapKey::String(key.clone());
                    let rtfs_value = Self::json_to_rtfs_value(value)?;
                    map.insert(rtfs_key, rtfs_value);
                }
                Ok(Value::Map(map))
            }
            serde_json::Value::Null => Ok(Value::Nil),
        }
    }

    /// Return a sanitized snapshot of registered capabilities for observability purposes.
    /// This intentionally omits any sensitive data (auth tokens, plugin internal paths, handlers).
    /// The shape is a vec of minimal JSON objects built manually to avoid leaking internal structs.
    pub async fn public_capabilities_snapshot(
        &self,
        limit: Option<usize>,
    ) -> Vec<serde_json::Value> {
        let caps_guard = self.capabilities.read().await;
        let mut out: Vec<serde_json::Value> = Vec::with_capacity(caps_guard.len());
        for (id, manifest) in caps_guard.iter() {
            if let Some(lim) = limit {
                if out.len() >= lim {
                    break;
                }
            }
            // Derive provider type label
            let provider_type = match &manifest.provider {
                ProviderType::Local(_) => "local",
                ProviderType::Http(_) => "http",
                ProviderType::OpenApi(_) => "openapi",
                ProviderType::MCP(_) => "mcp",
                ProviderType::A2A(_) => "a2a",
                ProviderType::Plugin(_) => "plugin",
                ProviderType::RemoteRTFS(_) => "remote_rtfs",
                ProviderType::Stream(_) => "stream",
                ProviderType::Registry(_) => "registry",
            };
            // Namespace heuristic: split by '.' keep first or use entire if absent
            let namespace = id.split('.').next().unwrap_or("");
            out.push(serde_json::json!({
                "id": id,
                "namespace": namespace,
                "provider_type": provider_type,
                "version": manifest.version,
            }));
        }
        out
    }

    /// Return a lightweight summary useful for counts & grouping without iterating twice.
    pub async fn public_capabilities_aggregate(&self) -> serde_json::Value {
        use std::collections::HashMap;
        let caps_guard = self.capabilities.read().await;
        let mut by_provider: HashMap<&'static str, u64> = HashMap::new();
        let mut namespaces: HashMap<String, u64> = HashMap::new();
        for (id, manifest) in caps_guard.iter() {
            let provider_label: &'static str = match &manifest.provider {
                ProviderType::Local(_) => "local",
                ProviderType::Http(_) => "http",
                ProviderType::OpenApi(_) => "openapi",
                ProviderType::MCP(_) => "mcp",
                ProviderType::A2A(_) => "a2a",
                ProviderType::Plugin(_) => "plugin",
                ProviderType::RemoteRTFS(_) => "remote_rtfs",
                ProviderType::Stream(_) => "stream",
                ProviderType::Registry(_) => "registry",
            };
            *by_provider.entry(provider_label).or_insert(0) += 1;
            let ns = id.split('.').next().unwrap_or("").to_string();
            *namespaces.entry(ns).or_insert(0) += 1;
        }
        serde_json::json!({
            "total": caps_guard.len(),
            "by_provider_type": by_provider,
            "namespaces": namespaces,
        })
    }

    /// Snapshot the isolation policy in a serializable, non-sensitive form.
    pub fn isolation_policy_snapshot(&self) -> serde_json::Value {
        serde_json::json!({
            "allowed_patterns": self.isolation_policy.allowed_capabilities,
            "denied_patterns": self.isolation_policy.denied_capabilities,
            "time_constraints_active": self.isolation_policy.time_constraints.is_some(),
        })
    }

    /// Export serializable capabilities to RTFS files for documentation or external tooling.
    /// This is not used for runtime import yet; it complements JSON export with a human-friendly format.
    pub async fn export_capabilities_to_rtfs_dir<P: AsRef<Path>>(
        &self,
        dir: P,
    ) -> RuntimeResult<usize> {
        let out_dir = dir.as_ref();
        fs::create_dir_all(out_dir).map_err(|e| {
            RuntimeError::Generic(format!("Failed to create RTFS export dir: {}", e))
        })?;
        let caps = self.capabilities.read().await;
        let mut written = 0usize;
        for cap in caps.values() {
            // Skip non-serializable provider types
            let provider_label = match &cap.provider {
                ProviderType::Http(_) => ":http",
                ProviderType::OpenApi(_) => ":openapi",
                ProviderType::MCP(_) => ":mcp",
                ProviderType::A2A(_) => ":a2a",
                ProviderType::RemoteRTFS(_) => ":remote_rtfs",
                ProviderType::Local(_)
                | ProviderType::Stream(_)
                | ProviderType::Registry(_)
                | ProviderType::Plugin(_) => {
                    if let Some(cb) = &self.debug_callback {
                        cb(format!(
                            "Skipping RTFS export for non-serializable provider: {}",
                            cap.id
                        ));
                    }
                    continue;
                }
            };

            let input_schema_str = cap
                .input_schema
                .as_ref()
                .map(|s| type_expr_to_rtfs_pretty(s))
                .unwrap_or_else(|| ":any".to_string());
            let output_schema_str = cap
                .output_schema
                .as_ref()
                .map(|s| type_expr_to_rtfs_pretty(s))
                .unwrap_or_else(|| ":any".to_string());

            let permissions_str = if cap.permissions.is_empty() {
                "[]".to_string()
            } else {
                format!(
                    "[{}]",
                    cap.permissions
                        .iter()
                        .map(|p| if p.starts_with(':') {
                            p.clone()
                        } else {
                            format!(":{}", p)
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            };

            let effects_str = if cap.effects.is_empty() {
                "[]".to_string()
            } else {
                format!(
                    "[{}]",
                    cap.effects
                        .iter()
                        .map(|e| if e.starts_with(':') {
                            e.clone()
                        } else {
                            format!(":{}", e)
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            };

            let provider_meta = match &cap.provider {
                ProviderType::Http(h) => format!(
                    ":provider-meta {{:base_url \"{}\" :timeout_ms {} }}",
                    h.base_url, h.timeout_ms
                ),
                ProviderType::OpenApi(o) => {
                    let mut parts = vec![format!(":base_url \"{}\"", o.base_url)];
                    parts.push(format!(":timeout_ms {}", o.timeout_ms));
                    if let Some(spec) = &o.spec_url {
                        parts.push(format!(":spec_url \"{}\"", spec));
                    }
                    parts.push(format!(":operations {}", o.operations.len()));
                    if let Some(auth) = &o.auth {
                        parts.push(format!(":auth_type \"{}\"", auth.auth_type));
                        parts.push(format!(":auth_location \"{}\"", auth.location));
                    }
                    format!(":provider-meta {{{}}}", parts.join(" "))
                }
                ProviderType::MCP(m) => {
                    let mut parts = vec![
                        format!(":server_url \"{}\"", m.server_url),
                        format!(":tool_name \"{}\"", m.tool_name),
                        format!(":timeout_ms {}", m.timeout_ms),
                    ];
                    if let Some(requires) = cap.metadata.get("mcp_requires_session") {
                        parts.push(format!(
                            ":requires_session \"{}\"",
                            requires.replace('"', "\\\"")
                        ));
                    }
                    format!(":provider-meta {{{}}}", parts.join(" "))
                }
                ProviderType::A2A(a) => format!(
                    ":provider-meta {{:agent_id \"{}\" :endpoint \"{}\" :protocol \"{}\" :timeout_ms {} }}",
                    a.agent_id, a.endpoint, a.protocol, a.timeout_ms
                ),
                ProviderType::RemoteRTFS(r) => format!(
                    ":provider-meta {{:endpoint \"{}\" :timeout_ms {} }}",
                    r.endpoint, r.timeout_ms
                ),
                _ => String::new(),
            };

            let metadata_block = if cap.metadata.is_empty() {
                "  :metadata nil".to_string()
            } else {
                let mut entries: Vec<_> = cap.metadata.iter().collect();
                entries.sort_by(|a, b| a.0.cmp(b.0));
                let mut lines = Vec::with_capacity(entries.len());
                for (key, value) in entries {
                    let escaped = value.replace('"', "\\\"");
                    lines.push(format!("    :{} \"{}\"", key, escaped));
                }
                format!("  :metadata {{\n{}\n  }}", lines.join("\n"))
            };

            let rtfs_content = format!(
                r#";; Exported capability snapshot (read-only)
;; Generated at {}

(capability "{}"
  :name "{}"
  :version "{}"
  :description "{}"
  :provider {}
  {}
{}
  :permissions {}
  :effects {}
  :input-schema {}
  :output-schema {}
  :implementation
    (fn [input]
      "Exported manifest only; runtime-managed execution"
      input)
)
"#,
                chrono::Utc::now().to_rfc3339(),
                cap.id,
                cap.name,
                cap.version,
                cap.description.replace('"', "'"),
                provider_label,
                provider_meta,
                metadata_block,
                permissions_str,
                effects_str,
                input_schema_str,
                output_schema_str
            );

            let mut file_name = cap.id.replace('/', "_").replace(' ', "_");
            if !file_name.ends_with(".rtfs") {
                file_name.push_str(".rtfs");
            }
            let file_path = out_dir.join(file_name);
            fs::write(&file_path, rtfs_content).map_err(|e| {
                RuntimeError::Generic(format!("Failed to write RTFS export for {}: {}", cap.id, e))
            })?;
            written += 1;
        }
        Ok(written)
    }

    /// Export serializable capabilities to a JSON file.
    /// Skips non-serializable providers (Local, Stream, Registry, Plugin).
    pub async fn export_capabilities_to_file<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> RuntimeResult<usize> {
        let caps = self.capabilities.read().await;
        let mut serializable = Vec::new();
        for cap in caps.values() {
            if let Some(s) = Option::<SerializableManifest>::from(cap) {
                serializable.push(s);
            } else if let Some(cb) = &self.debug_callback {
                cb(format!(
                    "Skipping non-serializable provider for capability {}",
                    cap.id
                ));
            }
        }
        let json = serde_json::to_string_pretty(&serializable).map_err(|e| {
            RuntimeError::Generic(format!("Failed to serialize capabilities: {}", e))
        })?;
        std::fs::write(&path, json)
            .map_err(|e| RuntimeError::Generic(format!("Failed to write export file: {}", e)))?;
        Ok(serializable.len())
    }

    /// Import capabilities from a JSON file that was previously exported.
    /// Returns the number of capabilities loaded into the marketplace.
    pub async fn import_capabilities_from_file<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> RuntimeResult<usize> {
        let data = std::fs::read_to_string(&path)
            .map_err(|e| RuntimeError::Generic(format!("Failed to read import file: {}", e)))?;
        let list: Vec<SerializableManifest> = serde_json::from_str(&data)
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse import file: {}", e)))?;
        let mut loaded = 0usize;
        let mut caps = self.capabilities.write().await;
        for s in list {
            let cap: CapabilityManifest = s.into();
            caps.insert(cap.id.clone(), cap);
            loaded += 1;
        }
        Ok(loaded)
    }

    /// Import capabilities exported as RTFS files (one .rtfs per capability) from a directory.
    /// Only supports provider types that are expressible in the exported RTFS (http, mcp, a2a, remote_rtfs).
    pub async fn import_capabilities_from_rtfs_dir<P: AsRef<Path>>(
        &self,
        dir: P,
    ) -> RuntimeResult<usize> {
        let dir_path = dir.as_ref();
        let mut loaded = 0usize;

        let entries = std::fs::read_dir(dir_path).map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to read RTFS dir {}: {}",
                dir_path.display(),
                e
            ))
        })?;

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                if ext != "rtfs" {
                    continue;
                }
            } else {
                continue;
            }

            // First try: use MCPDiscoveryProvider parsing helpers (robust path)
            let parser_res = MCPDiscoveryProvider::new(MCPServerConfig::default());
            if let Ok(parser) = parser_res {
                match parser.load_rtfs_capabilities(path.to_str().unwrap_or_default()) {
                    Ok(module) => {
                        for cap_def in module.capabilities {
                            match parser.rtfs_to_capability_manifest(&cap_def) {
                                Ok(manifest) => {
                                    let mut caps = self.capabilities.write().await;
                                    caps.insert(manifest.id.clone(), manifest);
                                    loaded += 1;
                                }
                                Err(e) => {
                                    if let Some(cb) = &self.debug_callback {
                                        cb(format!(
                                            "Failed to convert RTFS capability in {}: {}",
                                            path.display(),
                                            e
                                        ));
                                    }
                                }
                            }
                        }
                        continue;
                    }
                    Err(_) => {
                        // fall through to heuristic parser below
                    }
                }
            }

            // Fallback: heuristic parsing (legacy behavior)
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(e) => {
                    if let Some(cb) = &self.debug_callback {
                        cb(format!(
                            "Failed to read RTFS file {}: {}",
                            path.display(),
                            e
                        ));
                    }
                    continue;
                }
            };

            // --- existing heuristic parsing logic ---
            // Helper closures for simple RTFS-extracted fields
            let extract_quoted = |key: &str, src: &str| -> Option<String> {
                if let Some(pos) = src.find(key) {
                    let after = &src[pos + key.len()..];
                    if let Some(q1) = after.find('"') {
                        let rest = &after[q1 + 1..];
                        if let Some(q2) = rest.find('"') {
                            return Some(rest[..q2].to_string());
                        }
                    }
                }
                None
            };

            let extract_keyword = |key: &str, src: &str| -> Option<String> {
                if let Some(pos) = src.find(key) {
                    let after = &src[pos + key.len()..];
                    // Split on whitespace/newline and take first token
                    let tok = after
                        .split_whitespace()
                        .next()
                        .map(|s| s.trim().to_string());
                    tok
                } else {
                    None
                }
            };

            let extract_provider_meta = |src: &str| -> HashMap<String, String> {
                let mut map = HashMap::new();
                if let Some(pos) = src.find(":provider-meta") {
                    if let Some(brace_start) = src[pos..].find('{') {
                        let abs_start = pos + brace_start;
                        let mut depth = 0isize;
                        let mut end = None;
                        for (i, ch) in src[abs_start..].chars().enumerate() {
                            match ch {
                                '{' => depth += 1,
                                '}' => {
                                    depth -= 1;
                                    if depth == 0 {
                                        end = Some(abs_start + i + 1);
                                        break;
                                    }
                                }
                                _ => {}
                            }
                        }
                        if let Some(abs_end) = end {
                            let block = &src[abs_start + 1..abs_end - 1];
                            let mut chars = block.chars().peekable();
                            while let Some(ch) = chars.next() {
                                if ch == ':' {
                                    let mut key = String::new();
                                    while let Some(&c) = chars.peek() {
                                        if c.is_whitespace() || c == ':' || c == '{' || c == '}' {
                                            break;
                                        }
                                        key.push(c);
                                        chars.next();
                                    }
                                    if key.is_empty() {
                                        continue;
                                    }
                                    while let Some(&c) = chars.peek() {
                                        if c.is_whitespace() {
                                            chars.next();
                                        } else {
                                            break;
                                        }
                                    }
                                    let mut value = String::new();
                                    if let Some(&next_ch) = chars.peek() {
                                        if next_ch == '"' {
                                            chars.next();
                                            while let Some(c) = chars.next() {
                                                if c == '"' {
                                                    break;
                                                }
                                                value.push(c);
                                            }
                                        } else {
                                            while let Some(&c) = chars.peek() {
                                                if c.is_whitespace() || c == ':' || c == '}' {
                                                    break;
                                                }
                                                value.push(c);
                                                chars.next();
                                            }
                                        }
                                    }
                                    if !key.is_empty() {
                                        map.insert(key.replace('-', "_"), value);
                                    }
                                }
                            }
                        }
                    }
                }
                map
            };

            let extract_metadata = |src: &str| -> HashMap<String, String> {
                let mut map = HashMap::new();
                if let Some(pos) = src.find(":metadata") {
                    if let Some(brace_start) = src[pos..].find('{') {
                        let abs_start = pos + brace_start;
                        let mut depth = 0isize;
                        let mut end = None;
                        for (i, ch) in src[abs_start..].chars().enumerate() {
                            match ch {
                                '{' => depth += 1,
                                '}' => {
                                    depth -= 1;
                                    if depth == 0 {
                                        end = Some(abs_start + i + 1);
                                        break;
                                    }
                                }
                                _ => {}
                            }
                        }
                        if let Some(abs_end) = end {
                            let block = &src[abs_start + 1..abs_end - 1];
                            let mut chars = block.chars().peekable();
                            while let Some(ch) = chars.next() {
                                if ch == ':' {
                                    let mut key = String::new();
                                    while let Some(&c) = chars.peek() {
                                        if c.is_whitespace() || c == ':' || c == '{' || c == '}' {
                                            break;
                                        }
                                        key.push(c);
                                        chars.next();
                                    }
                                    if key.is_empty() {
                                        continue;
                                    }
                                    while let Some(&c) = chars.peek() {
                                        if c.is_whitespace() {
                                            chars.next();
                                        } else {
                                            break;
                                        }
                                    }
                                    if let Some(&next_ch) = chars.peek() {
                                        if next_ch == '"' {
                                            chars.next();
                                            let mut value = String::new();
                                            while let Some(c) = chars.next() {
                                                if c == '"' {
                                                    break;
                                                }
                                                value.push(c);
                                            }
                                            map.insert(key.replace('-', "_"), value);
                                        } else if next_ch == '{' {
                                            // Skip nested map
                                            let mut nested_depth = 0isize;
                                            while let Some(c) = chars.next() {
                                                match c {
                                                    '{' => nested_depth += 1,
                                                    '}' => {
                                                        nested_depth -= 1;
                                                        if nested_depth == 0 {
                                                            break;
                                                        }
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        } else {
                                            let mut value = String::new();
                                            while let Some(&c) = chars.peek() {
                                                if c.is_whitespace() || c == ':' || c == '}' {
                                                    break;
                                                }
                                                value.push(c);
                                                chars.next();
                                            }
                                            if !value.is_empty() {
                                                map.insert(key.replace('-', "_"), value);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                map
            };

            // Extract basic fields
            let id = extract_quoted(":id", &content).or_else(|| {
                // fallback to filename-based id
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_string())
            });
            let name = extract_quoted(":name", &content).unwrap_or_else(|| "".to_string());
            let description =
                extract_quoted(":description", &content).unwrap_or_else(|| "".to_string());
            let version =
                extract_quoted(":version", &content).unwrap_or_else(|| "1.0.0".to_string());

            if id.is_none() {
                if let Some(cb) = &self.debug_callback {
                    cb(format!("Skipping RTFS file without id: {}", path.display()));
                }
                continue;
            }

            let id = id.unwrap();

            // provider label e.g. :provider :http
            let provider_token = extract_keyword(":provider", &content).unwrap_or_default();

            let provider_meta = extract_provider_meta(&content);
            let mut metadata_map = extract_metadata(&content);

            // parse schemas
            let input_schema_opt = if let Some(s) = extract_keyword(":input-schema", &content) {
                let s_trim = s.trim();
                if s_trim == "nil" || s_trim == ":any" || s_trim == "nil," {
                    None
                } else {
                    // If the schema token is complex (starts with '[' or '('), we try to extract whole bracketed expr from content
                    // Simple heuristic: find the substring ":input-schema" and take remainder of that line
                    if let Some(pos) = content.find(":input-schema") {
                        if let Some(line_end) = content[pos..].find('\n') {
                            let line = content[pos..pos + line_end].to_string();
                            // remove key
                            if let Some(idx) = line.find(":input-schema") {
                                let remainder = line[idx + ":input-schema".len()..].trim();
                                let expr =
                                    remainder.trim().trim_end_matches(',').trim().to_string();
                                match TypeExpr::from_str(&expr) {
                                    Ok(texpr) => Some(texpr),
                                    Err(_) => None,
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
            } else {
                None
            };

            let output_schema_opt = if let Some(s) = extract_keyword(":output-schema", &content) {
                let s_trim = s.trim();
                if s_trim == "nil" || s_trim == ":any" || s_trim == "nil," {
                    None
                } else {
                    if let Some(pos) = content.find(":output-schema") {
                        if let Some(line_end) = content[pos..].find('\n') {
                            let line = content[pos..pos + line_end].to_string();
                            if let Some(idx) = line.find(":output-schema") {
                                let remainder = line[idx + ":output-schema".len()..].trim();
                                let expr =
                                    remainder.trim().trim_end_matches(',').trim().to_string();
                                match TypeExpr::from_str(&expr) {
                                    Ok(texpr) => Some(texpr),
                                    Err(_) => None,
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
            } else {
                None
            };

            // Build provider
            let provider = if provider_token.contains(":http") || provider_token == ":http" {
                let base_url = provider_meta.get("base_url").cloned().unwrap_or_default();
                let timeout_ms = provider_meta
                    .get("timeout_ms")
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(30000);
                ProviderType::Http(HttpCapability {
                    base_url,
                    auth_token: None,
                    timeout_ms,
                })
            } else if provider_token.contains(":mcp") || provider_token == ":mcp" {
                let server_url = provider_meta.get("server_url").cloned().unwrap_or_default();
                let tool_name = provider_meta.get("tool_name").cloned().unwrap_or_default();
                let timeout_ms = provider_meta
                    .get("timeout_ms")
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(5000);
                ProviderType::MCP(MCPCapability {
                    server_url,
                    tool_name,
                    timeout_ms,
                })
            } else if provider_token.contains(":a2a") || provider_token == ":a2a" {
                let agent_id = provider_meta.get("agent_id").cloned().unwrap_or_default();
                let endpoint = provider_meta.get("endpoint").cloned().unwrap_or_default();
                let protocol = provider_meta.get("protocol").cloned().unwrap_or_default();
                let timeout_ms = provider_meta
                    .get("timeout_ms")
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(5000);
                ProviderType::A2A(A2ACapability {
                    agent_id,
                    endpoint,
                    protocol,
                    timeout_ms,
                })
            } else if provider_token.contains(":remote_rtfs") || provider_token == ":remote_rtfs" {
                let endpoint = provider_meta.get("endpoint").cloned().unwrap_or_default();
                let timeout_ms = provider_meta
                    .get("timeout_ms")
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(5000);
                ProviderType::RemoteRTFS(RemoteRTFSCapability {
                    endpoint,
                    timeout_ms,
                    auth_token: None,
                })
            } else {
                // Unsupported provider for import
                if let Some(cb) = &self.debug_callback {
                    cb(format!(
                        "Skipping RTFS import for unsupported provider in {}",
                        id
                    ));
                }
                continue;
            };

            if let ProviderType::MCP(m) = &provider {
                metadata_map
                    .entry("mcp_server_url".to_string())
                    .or_insert_with(|| m.server_url.clone());
                metadata_map
                    .entry("mcp_tool_name".to_string())
                    .or_insert_with(|| m.tool_name.clone());
                if let Some(req) = provider_meta.get("requires_session").cloned() {
                    metadata_map
                        .entry("mcp_requires_session".to_string())
                        .or_insert(req);
                }
            }

            let manifest = CapabilityManifest {
                id: id.clone(),
                name: name.clone(),
                description: description.clone(),
                provider,
                version: version.clone(),
                input_schema: input_schema_opt,
                output_schema: output_schema_opt,
                attestation: None,
                provenance: None,
                permissions: vec![],
                effects: vec![],
                metadata: metadata_map,
                agent_metadata: None,
            };

            // Register
            {
                let mut caps = self.capabilities.write().await;
                caps.insert(id.clone(), manifest);
            }
            loaded += 1;
        }

        Ok(loaded)
    }
}
