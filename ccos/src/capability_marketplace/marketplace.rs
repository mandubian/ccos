use super::executors::CapabilityExecutor;
use super::executors::{
    A2AExecutor, ExecutorVariant, HttpExecutor, LocalExecutor, MCPExecutor, OpenApiExecutor,
    RegistryExecutor, SandboxedExecutor,
};
use super::mcp_discovery::{MCPDiscoveryProvider, MCPServerConfig};
use super::resource_monitor::ResourceMonitor;
use super::types::*;
use super::versioning::{compare_versions, detect_breaking_changes, VersionComparison};
use crate::capabilities::native_provider::NativeCapabilityProvider;
use crate::catalog::{CatalogService, CatalogSource};
use crate::streaming::{
    McpStreamingProvider, StreamConfig, StreamHandle, StreamType, StreamingProvider,
};
use crate::synthesis::schema_serializer::type_expr_to_rtfs_compact;
use chrono::Utc;
use futures::future::BoxFuture;
use rtfs::ast::{MapKey, TypeExpr};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::host_interface::HostInterface;
use rtfs::runtime::pure_host;
use rtfs::runtime::type_validator::{TypeCheckingConfig, TypeValidator, VerificationContext};
use rtfs::runtime::values::Value;
use serde::{Deserialize, Serialize};
use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Result of updating a capability
#[derive(Debug, Clone)]
pub struct UpdateResult {
    /// Whether the capability was updated (false if it was newly registered)
    pub updated: bool,
    /// Type of version change detected
    pub version_comparison: VersionComparison,
    /// List of breaking changes detected (empty if none)
    pub breaking_changes: Vec<String>,
    /// Previous version (None if newly registered)
    pub previous_version: Option<String>,
}

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
    Sandboxed {
        runtime: String,
        source: String,
        entry_point: Option<String>,
        provider: Option<String>,
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
            ProviderType::Sandboxed(s) => Some(SerializableProvider::Sandboxed {
                runtime: s.runtime.clone(),
                source: s.source.clone(),
                entry_point: s.entry_point.clone(),
                provider: s.provider.clone(),
            }),
            // Skip non-serializable providers
            ProviderType::Local(_)
            | ProviderType::Stream(_)
            | ProviderType::Registry(_)
            | ProviderType::Plugin(_)
            | ProviderType::Native(_) => None,
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
                auth_token: None,
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
            SerializableProvider::Sandboxed {
                runtime,
                source,
                entry_point,
                provider,
            } => ProviderType::Sandboxed(SandboxedCapability {
                runtime,
                source,
                entry_point,
                provider,
            }),
        }
    }
}

fn infer_catalog_source(manifest: &CapabilityManifest) -> CatalogSource {
    if let Some(source) = manifest.metadata.get("source") {
        let lowered = source.to_lowercase();
        if lowered.contains("user") {
            return CatalogSource::User;
        }
        if lowered.contains("generated") || lowered.contains("synthesized") {
            return CatalogSource::Generated;
        }
        if lowered.contains("system") {
            return CatalogSource::System;
        }
        if lowered.contains("discovered") {
            return CatalogSource::Discovered;
        }
    }

    match &manifest.provider {
        ProviderType::MCP(_) | ProviderType::OpenApi(_) | ProviderType::Registry(_) => {
            CatalogSource::Discovered
        }
        ProviderType::Local(_)
        | ProviderType::RemoteRTFS(_)
        | ProviderType::Stream(_)
        | ProviderType::Http(_)
        | ProviderType::Plugin(_)
        | ProviderType::A2A(_)
        | ProviderType::Sandboxed(_)
        | ProviderType::Native(_) => CatalogSource::Generated,
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
            domains: Vec::new(),
            categories: Vec::new(),
            effect_type: EffectType::Effectful,
        }
    }
}

impl CapabilityMarketplace {
    pub fn new(
        capability_registry: Arc<RwLock<crate::capabilities::registry::CapabilityRegistry>>,
    ) -> Self {
        Self::with_causal_chain(capability_registry, None)
    }

    pub fn with_causal_chain(
        capability_registry: Arc<RwLock<crate::capabilities::registry::CapabilityRegistry>>,
        causal_chain: Option<Arc<std::sync::Mutex<crate::causal_chain::CausalChain>>>,
    ) -> Self {
        Self::with_causal_chain_and_debug_callback(capability_registry, causal_chain, None)
    }

    pub fn with_causal_chain_and_debug_callback(
        capability_registry: Arc<RwLock<crate::capabilities::registry::CapabilityRegistry>>,
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
            catalog: Arc::new(RwLock::new(None)),
            rtfs_host_factory: Arc::new(std::sync::RwLock::new(None)),
        };
        marketplace.executor_registry.insert(
            TypeId::of::<MCPCapability>(),
            ExecutorVariant::MCP(MCPExecutor {
                session_pool: marketplace.session_pool.clone(),
            }),
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
        marketplace.executor_registry.insert(
            TypeId::of::<SandboxedCapability>(),
            ExecutorVariant::Sandboxed(SandboxedExecutor::new()),
        );
        marketplace.executor_registry.insert(
            TypeId::of::<NativeCapability>(),
            ExecutorVariant::Native(NativeCapabilityProvider::new()),
        );
        marketplace
    }

    /// Attach a catalog service for capability indexing
    pub async fn set_catalog_service(&self, catalog: Arc<CatalogService>) {
        let mut guard = self.catalog.write().await;
        *guard = Some(catalog);
    }

    /// Get the attached catalog service
    pub async fn get_catalog(&self) -> Option<Arc<CatalogService>> {
        self.catalog.read().await.clone()
    }

    /// Configure the Host factory used to execute RTFS capabilities (default: PureHost).
    pub fn set_rtfs_host_factory(
        &self,
        factory: Arc<dyn Fn() -> Arc<dyn HostInterface + Send + Sync> + Send + Sync>,
    ) {
        if let Ok(mut guard) = self.rtfs_host_factory.write() {
            *guard = Some(factory);
        }
    }

    pub fn get_rtfs_host_factory(
        &self,
    ) -> Arc<dyn Fn() -> Arc<dyn HostInterface + Send + Sync> + Send + Sync> {
        self.rtfs_host_factory
            .read()
            .ok()
            .and_then(|g| g.clone())
            .unwrap_or_else(|| {
                Arc::new(|| {
                    let host: Arc<dyn HostInterface + Send + Sync> =
                        Arc::new(pure_host::PureHost::new());
                    host
                })
            })
    }

    /// Trigger a full capability re-ingestion into the catalog
    pub async fn refresh_catalog_index(&self) {
        let maybe_catalog = {
            let guard = self.catalog.read().await;
            guard.clone()
        };

        if let Some(catalog) = maybe_catalog {
            catalog.ingest_marketplace(self).await;
        }
    }

    async fn index_capability_in_catalog(&self, manifest: &CapabilityManifest) {
        let maybe_catalog = {
            let guard = self.catalog.read().await;
            guard.clone()
        };

        if let Some(catalog) = maybe_catalog {
            let source = infer_catalog_source(manifest);
            catalog.register_capability(manifest, source).await;
        }
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
        capability_registry: Arc<RwLock<crate::capabilities::registry::CapabilityRegistry>>,
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
    pub async fn bootstrap(
        &self,
        marketplace_arc: Arc<CapabilityMarketplace>,
    ) -> RuntimeResult<()> {
        // Register default capabilities first
        crate::capabilities::register_default_capabilities(self).await?;

        // Load previously discovered capabilities from RTFS files
        // This enables offline operation without re-querying MCP servers
        match self
            .load_discovered_capabilities::<std::path::PathBuf>(None)
            .await
        {
            Ok(count) => {
                if count > 0 {
                    if let Some(cb) = &self.debug_callback {
                        cb(format!(
                            "Loaded {} previously discovered capabilities from RTFS files",
                            count
                        ));
                    } else {
                        ccos_println!("📦 Loaded {} previously discovered capabilities", count);
                    }
                }
            }
            Err(e) => {
                // Non-fatal: log and continue
                if let Some(cb) = &self.debug_callback {
                    cb(format!(
                        "Note: Could not load discovered capabilities: {}",
                        e
                    ));
                }
            }
        }

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
                domains: Vec::new(),
                categories: Vec::new(),
            effect_type: EffectType::Effectful,
            };

            let mut caps = self.capabilities.write().await;
            caps.insert(capability_id.clone(), manifest);
        }
        */

        // Run discovery agents to find additional capabilities
        for agent in &self.discovery_agents {
            match agent.discover(Some(marketplace_arc.clone())).await {
                Ok(discovered_capabilities) => {
                    let mut caps = self.capabilities.write().await;
                    for capability in discovered_capabilities {
                        caps.insert(capability.id.clone(), capability);
                    }
                }
                Err(e) => {
                    // Log discovery errors but don't fail bootstrap
                    ccos_eprintln!("Discovery agent failed: {:?}", e);
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
    */

    /// Add an MCP discovery provider
    pub fn add_mcp_discovery(&mut self, config: MCPServerConfig) -> RuntimeResult<()> {
        let provider =
            MCPDiscoveryProvider::new_with_rtfs_host_factory(config, self.get_rtfs_host_factory())?;
        self.discovery_agents.push(Box::new(provider));
        Ok(())
    }

    /*
    /// Add an MCP discovery provider using the builder pattern
    pub fn add_mcp_discovery_builder(&mut self, builder: MCPDiscoveryBuilder) -> RuntimeResult<()> {
        let provider = builder.build()?;
        self.discovery_agents.push(Box::new(provider));
        Ok(())
    }
    */

    /*
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
    */

    /// Discover capabilities from all configured network sources
    pub async fn discover_from_network(&self) -> RuntimeResult<Vec<CapabilityManifest>> {
        let mut all_capabilities = Vec::new();

        for agent in &self.discovery_agents {
            if agent.name() == "NetworkDiscovery" {
                match agent.discover(None).await {
                    Ok(capabilities) => {
                        ccos_eprintln!(
                            "Discovered {} capabilities from network source",
                            capabilities.len()
                        );
                        all_capabilities.extend(capabilities);
                    }
                    Err(e) => {
                        ccos_eprintln!("Network discovery failed: {}", e);
                        // Continue with other discovery agents even if one fails
                    }
                }
            }
        }

        Ok(all_capabilities)
    }

    /*
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
            domains: Vec::new(),
            categories: Vec::new(),
            effect_type: EffectType::Effectful,
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
        self.register_local_capability_with_effects(id, name, description, handler, vec![])
            .await
    }

    /// Register a local capability with explicit effects
    pub async fn register_local_capability_with_effects(
        &self,
        id: String,
        name: String,
        description: String,
        handler: Arc<dyn Fn(&Value) -> RuntimeResult<Value> + Send + Sync>,
        effects: Vec<String>,
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
            effects,
            metadata: HashMap::new(),
            agent_metadata: None,
            domains: Vec::new(),
            categories: Vec::new(),
            effect_type: EffectType::Effectful,
        };

        let catalog_manifest = manifest.clone();

        // Register the capability
        {
            let mut caps = self.capabilities.write().await;
            caps.insert(id.clone(), manifest);
        }

        self.index_capability_in_catalog(&catalog_manifest).await;

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

        // Check if already registered to avoid duplicates
        {
            let caps = self.capabilities.read().await;
            if caps.contains_key(&id) {
                // Already registered - skip to avoid duplicates
                log::debug!(
                    "Capability {} already registered, skipping duplicate registration",
                    id
                );
                return Ok(());
            }
        }

        let catalog_manifest = manifest.clone();

        // Register the capability
        {
            let mut caps = self.capabilities.write().await;
            caps.insert(id.clone(), manifest);
        }

        self.index_capability_in_catalog(&catalog_manifest).await;

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

            self.refresh_catalog_index().await;
        }

        Ok(())
    }

    /// Update a capability with version tracking and breaking change detection
    ///
    /// This method:
    /// - Compares versions using semantic versioning
    /// - Detects breaking changes
    /// - Tracks version history
    /// - Updates last_updated timestamp
    /// - Emits audit events for version updates
    pub async fn update_capability(
        &self,
        new_manifest: CapabilityManifest,
        force: bool,
    ) -> RuntimeResult<UpdateResult> {
        let id = new_manifest.id.clone();
        let mut caps = self.capabilities.write().await;

        let existing = caps.get(&id).cloned();

        match existing {
            Some(existing_manifest) => {
                // Compare versions
                let version_comparison =
                    match compare_versions(&existing_manifest.version, &new_manifest.version) {
                        Ok(comp) => comp,
                        Err(e) => {
                            // If version parsing fails, treat as equal and log warning
                            if let Some(cb) = &self.debug_callback {
                                cb(format!(
                                "Warning: Failed to parse versions for {}: {}. Treating as update.",
                                id, e
                            ));
                            }
                            VersionComparison::Equal
                        }
                    };

                // Detect breaking changes
                let breaking_changes =
                    detect_breaking_changes(&existing_manifest, &new_manifest).unwrap_or_default();

                // Check if update should be allowed
                let is_breaking = !breaking_changes.is_empty()
                    || matches!(version_comparison, VersionComparison::MajorUpdate);

                if is_breaking && !force {
                    return Err(RuntimeError::Generic(format!(
                        "Breaking changes detected for capability '{}': {:?}. Use force=true to update anyway.",
                        id, breaking_changes
                    )));
                }

                // Prepare updated manifest with version metadata
                let previous_version = existing_manifest.version.clone();
                let mut updated_manifest = new_manifest;
                updated_manifest = updated_manifest
                    .with_previous_version(previous_version.clone())
                    .add_to_version_history(previous_version.clone())
                    .set_last_updated();

                // Update the capability
                caps.insert(id.clone(), updated_manifest.clone());

                // Prepare audit event data
                let mut audit_data = HashMap::new();
                audit_data.insert("old_version".to_string(), previous_version.clone());
                audit_data.insert("new_version".to_string(), updated_manifest.version.clone());
                audit_data.insert(
                    "version_comparison".to_string(),
                    format!("{:?}", version_comparison),
                );
                audit_data.insert(
                    "breaking_changes_count".to_string(),
                    breaking_changes.len().to_string(),
                );
                if !breaking_changes.is_empty() {
                    audit_data.insert(
                        "breaking_changes".to_string(),
                        serde_json::to_string(&breaking_changes).unwrap_or_default(),
                    );
                }

                // Emit audit event
                self.emit_capability_audit_event("capability_updated", &id, Some(audit_data))
                    .await?;

                // Refresh catalog
                drop(caps);
                self.refresh_catalog_index().await;

                Ok(UpdateResult {
                    updated: true,
                    version_comparison,
                    breaking_changes,
                    previous_version: Some(previous_version),
                })
            }
            None => {
                // Capability doesn't exist, register it as new
                let new_manifest = new_manifest.set_last_updated();
                caps.insert(id.clone(), new_manifest.clone());
                drop(caps);

                // Register in catalog
                self.index_capability_in_catalog(&new_manifest).await;

                // Emit audit event
                self.emit_capability_audit_event("capability_registered", &id, None)
                    .await?;

                Ok(UpdateResult {
                    updated: false, // Was registered, not updated
                    version_comparison: VersionComparison::Equal,
                    breaking_changes: Vec::new(),
                    previous_version: None,
                })
            }
        }
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

        // Log the audit event using callback or fallback to debug log
        let audit_message = format!("CAPABILITY_AUDIT: {:?}", event_data);
        if let Some(callback) = &self.debug_callback {
            callback(audit_message);
        } else {
            log::debug!("{}", audit_message);
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
                session_id: None,
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
                    ccos_eprintln!(
                        "Failed to record capability audit event in Causal Chain: {:?}",
                        e
                    );
                }
            } else {
                ccos_eprintln!("Failed to acquire lock on Causal Chain");
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
            domains: Vec::new(),
            categories: Vec::new(),
            effect_type: EffectType::Effectful,
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
            domains: Vec::new(),
            categories: Vec::new(),
            effect_type: EffectType::Effectful,
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
            domains: Vec::new(),
            categories: Vec::new(),
            effect_type: EffectType::Effectful,
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
            domains: Vec::new(),
            categories: Vec::new(),
            effect_type: EffectType::Effectful,
        };
        let mut caps = self.capabilities.write().await;
        caps.insert(id, capability);
        Ok(())
    }

    /// Register a native capability dynamically
    pub async fn register_native_capability(
        &self,
        id: String,
        name: String,
        description: String,
        handler: Arc<dyn Fn(&Value) -> BoxFuture<'static, RuntimeResult<Value>> + Send + Sync>,
        security_level: String,
    ) -> Result<(), RuntimeError> {
        let provenance = CapabilityProvenance {
            source: "native".to_string(),
            version: Some("1.0.0".to_string()),
            content_hash: self
                .compute_content_hash(&format!("{}{}{}{}", id, name, description, security_level)),
            custody_chain: vec!["native_registration".to_string()],
            registered_at: chrono::Utc::now(),
        };

        let mut metadata = HashMap::new();
        metadata.insert("security_level".to_string(), security_level.clone());

        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::Native(NativeCapability {
                handler,
                security_level: security_level.clone(),
                metadata: HashMap::new(),
            }),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            effects: vec![],
            metadata,
            agent_metadata: None,
            domains: Vec::new(),
            categories: Vec::new(),
            effect_type: EffectType::Effectful,
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
                auth_token: None,
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
            domains: Vec::new(),
            categories: Vec::new(),
            effect_type: EffectType::Effectful,
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
                auth_token: None,
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
            domains: Vec::new(),
            categories: Vec::new(),
            effect_type: EffectType::Effectful,
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
            domains: Vec::new(),
            categories: Vec::new(),
            effect_type: EffectType::Effectful,
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
            domains: Vec::new(),
            categories: Vec::new(),
            effect_type: EffectType::Effectful,
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
            domains: Vec::new(),
            categories: Vec::new(),
            effect_type: EffectType::Effectful,
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
            domains: Vec::new(),
            categories: Vec::new(),
            effect_type: EffectType::Effectful,
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
            domains: Vec::new(),
            categories: Vec::new(),
            effect_type: EffectType::Effectful,
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

    pub async fn update_capability_output_schema(
        &self,
        id: &str,
        new_schema: TypeExpr,
    ) -> RuntimeResult<()> {
        let mut capabilities = self.capabilities.write().await;
        if let Some(manifest) = capabilities.get_mut(id) {
            manifest.output_schema = Some(new_schema);
            manifest.metadata.insert(
                "ccos_sampled_output_schema".to_string(),
                Utc::now().to_rfc3339(),
            );
            Ok(())
        } else {
            Err(RuntimeError::Generic(format!(
                "Capability '{}' not found",
                id
            )))
        }
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
        metadata: Option<&rtfs::runtime::execution_outcome::CallMetadata>,
    ) -> RuntimeResult<Value> {
        // Delegate to the existing method, but preserve metadata for missing-capability context
        // so downstream resolvers can see any grounding/context hints.
        self.execute_capability_with_metadata(id, inputs, metadata)
            .await
    }

    // execute_effect_request removed - unified into execute_capability_enhanced

    pub async fn execute_capability(&self, id: &str, inputs: &Value) -> RuntimeResult<Value> {
        self.execute_capability_with_metadata(id, inputs, None)
            .await
    }

    async fn execute_capability_with_metadata(
        &self,
        id: &str,
        inputs: &Value,
        metadata: Option<&rtfs::runtime::execution_outcome::CallMetadata>,
    ) -> RuntimeResult<Value> {
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
                    ccos_eprintln!(
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
            // If capability not registered locally, try to trigger missing capability resolution
            // via the registry (which holds the resolver)
            {
                let registry = self.capability_registry.read().await;
                if let Some(resolver) = registry.get_missing_capability_resolver() {
                    let mut context = std::collections::HashMap::new();
                    context.insert("source".to_string(), "marketplace_miss".to_string());
                    if let Some(meta) = metadata {
                        for (k, v) in &meta.context {
                            // Truncate to avoid prompt bloat
                            let trimmed: String = v.chars().take(400).collect();
                            context.insert(
                                k.clone(),
                                if trimmed.len() < v.len() {
                                    format!("{}... [truncated]", trimmed)
                                } else {
                                    trimmed
                                },
                            );
                        }
                    }

                    // Extract args for context if possible (best effort)
                    let args = match inputs {
                        Value::Vector(v) => v.clone(),
                        Value::List(l) => l.clone(),
                        _ => vec![],
                    };

                    if let Err(e) =
                        resolver.handle_missing_capability(id.to_string(), args, context)
                    {
                        ccos_eprintln!(
                            "Warning: Failed to queue missing capability '{}': {}",
                            id,
                            e
                        );
                    }
                }
            }

            return Err(RuntimeError::UnknownCapability(id.to_string()));
        };

        let normalized_inputs = Self::normalize_input_envelope(inputs);
        let inputs_ref = normalized_inputs.as_ref().unwrap_or(inputs);

        // Check for session management requirements (generic, metadata-driven)
        // This works for ANY provider that declares session needs via metadata
        // Also auto-detect MCP provider types which inherently need session management
        let requires_session = manifest
            .metadata
            .iter()
            .any(|(k, v)| k.ends_with("_requires_session") && (v == "true" || v == "auto"));

        // MCP capabilities always need session management for proper auth token handling
        let is_mcp_provider = matches!(&manifest.provider, ProviderType::MCP(_));

        // Prepare boundary verification context
        let boundary_context = VerificationContext::capability_boundary(id);
        let type_config = TypeCheckingConfig::default();

        if requires_session || is_mcp_provider {
            // Delegate to session pool for session-managed execution
            let pool_opt = {
                let guard = self.session_pool.read().await;
                guard.clone() // Clone the Arc<SessionPoolManager>
            };

            if let Some(pool) = pool_opt {
                let args = match inputs_ref {
                    Value::List(list) => list.clone(),
                    Value::Vector(vec) => vec.clone(),
                    other => vec![other.clone()],
                };

                // Session pool will:
                // 1. Detect provider type from metadata (mcp_, graphql_, etc.)
                // 2. Route to appropriate SessionHandler
                // 3. Handler initializes/reuses session
                // 4. Handler executes with session (auth, headers, etc.)
                // 5. Returns result
                // IMPORTANT: enforce schema validation for session-managed calls too.
                // This is especially relevant for MCP where capabilities are often dynamic.
                if let Some(input_schema) = &manifest.input_schema {
                    self.type_validator
                        .validate_with_config(
                            inputs_ref,
                            input_schema,
                            &type_config,
                            &boundary_context,
                        )
                        .map_err(|e| {
                            RuntimeError::Generic(format!("Input validation failed: {}", e))
                        })?;
                }

                let exec_result = pool.execute_with_session(id, &manifest.metadata, &args)?;

                if let Some(output_schema) = &manifest.output_schema {
                    self.type_validator
                        .validate_with_config(
                            &exec_result,
                            output_schema,
                            &type_config,
                            &boundary_context,
                        )
                        .map_err(|e| {
                            RuntimeError::Generic(format!("Output validation failed: {}", e))
                        })?;
                }

                return Ok(exec_result);
            } else {
                ccos_eprintln!("⚠️  Session management required but no session pool configured");
                ccos_eprintln!(
                    "   Falling through to normal execution (will likely fail with 401)"
                );
            }
        }

        // Validate inputs if a schema is provided
        if let Some(input_schema) = &manifest.input_schema {
            self.type_validator
                .validate_with_config(inputs_ref, input_schema, &type_config, &boundary_context)
                .map_err(|e| RuntimeError::Generic(format!("Input validation failed: {}", e)))?;
        }

        // Execute via executor registry or provider fallback
        let session_pool = self.session_pool.read().await.clone();
        let context = super::executors::ExecutionContext::new(id, &manifest.metadata, session_pool);
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
                ProviderType::Native(_) => std::any::TypeId::of::<NativeCapability>(),
                ProviderType::Sandboxed(_) => std::any::TypeId::of::<SandboxedCapability>(),
            }) {
            executor
                .execute(&manifest.provider, inputs_ref, &context)
                .await
        } else {
            match &manifest.provider {
                ProviderType::Local(local) => (local.handler)(inputs_ref),
                ProviderType::Http(http) => self.execute_http_capability(http, inputs_ref).await,
                ProviderType::OpenApi(_) => {
                    let executor = OpenApiExecutor;
                    executor
                        .execute(&manifest.provider, inputs_ref, &context)
                        .await
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
                    self.execute_stream_capability(stream_impl, inputs_ref)
                        .await
                }
                ProviderType::Registry(_) => Err(RuntimeError::Generic(
                    "Registry provider missing executor".to_string(),
                )),
                ProviderType::Native(native) => (native.handler)(inputs_ref).await,
                ProviderType::Sandboxed(_) => Err(RuntimeError::Generic(
                    "Sandboxed capability executor not found".to_string(),
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

    fn normalize_input_envelope(inputs: &Value) -> Option<Value> {
        match inputs {
            Value::List(list) if list.len() == 1 => match &list[0] {
                Value::Map(map) => Some(Value::Map(map.clone())),
                _ => None,
            },
            _ => None,
        }
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

    /// Convert JSON to RTFS Value (public API wrapper for backward compatibility)
    ///
    /// This delegates to the shared utility in `ccos::utils::value_conversion`.

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
                ProviderType::Native(_) => "native",
                ProviderType::Sandboxed(_) => "sandboxed",
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
                ProviderType::Native(_) => "native",
                ProviderType::Sandboxed(_) => "sandboxed",
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
                | ProviderType::Plugin(_)
                | ProviderType::Native(_)
                | ProviderType::Sandboxed(_) => {
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
                .map(type_expr_to_rtfs_compact)
                .unwrap_or_else(|| ":any".to_string());
            let output_schema_str = cap
                .output_schema
                .as_ref()
                .map(type_expr_to_rtfs_compact)
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

    /// Export capabilities to a single RTFS module file.
    /// This writes multiple `(capability ...)` expressions into one file.
    pub async fn export_capabilities_to_rtfs_module_file<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> RuntimeResult<usize> {
        let caps = self.capabilities.read().await;
        let mut written = 0usize;
        let mut rtfs_content = String::new();

        rtfs_content.push_str(";; CCOS Capability Module Export\n");
        rtfs_content.push_str(&format!(";; Generated at: {}\n\n", chrono::Utc::now()));
        rtfs_content.push_str("(do\n");

        for cap in caps.values() {
            // Convert to SerializableManifest first to filter out non-serializable ones
            let serializable = match Option::<SerializableManifest>::from(cap) {
                Some(s) => s,
                None => {
                    if let Some(cb) = &self.debug_callback {
                        cb(format!(
                            "Skipping non-serializable provider for capability {}",
                            cap.id
                        ));
                    }
                    continue;
                }
            };

            // Re-convert to manifest to ensure it matches what we want to serialize
            // (SerializableManifest is an intermediate step that ensures provider is compatible)
            let manifest: CapabilityManifest = serializable.into();

            // Try to find the implementation code or generate a default one
            let implementation_code = manifest
                .metadata
                .get("rtfs_implementation")
                .cloned()
                .or_else(|| {
                    // Try to reconstruct implementation from provider metadata
                    match &manifest.provider {
                        crate::capability_marketplace::types::ProviderType::Http(http) => Some(format!(
                            "(fn [input] (call :http.request :method \"POST\" :url \"{}\" :body input))",
                            http.base_url
                        )),
                        crate::capability_marketplace::types::ProviderType::MCP(mcp) => Some(format!(
                            "(fn [input] (call :ccos.capabilities.mcp.call :server-url \"{}\" :tool-name \"{}\" :input input))",
                            mcp.server_url, mcp.tool_name
                        )),
                        _ => None,
                    }
                })
                .unwrap_or_else(|| {
                    // Fallback stub
                    "(fn [input] (error \"Implementation not available in export\"))".to_string()
                });

            let cap_rtfs = crate::synthesis::missing_capability_resolver::MissingCapabilityResolver::manifest_to_rtfs(
                &manifest,
                &implementation_code,
            );

            rtfs_content.push_str(&cap_rtfs);
            rtfs_content.push_str("\n\n");
            written += 1;
        }

        rtfs_content.push_str(")\n"); // Close the (do ...) block

        std::fs::write(&path, rtfs_content).map_err(|e| {
            RuntimeError::Generic(format!("Failed to write RTFS module export file: {}", e))
        })?;

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

    /// Load all discovered capabilities from the standard capabilities directory.
    /// This scans `capabilities/` (in workspace root) recursively for `.rtfs` files and loads them.
    ///
    /// The directory structure is expected to be:
    /// ```text
    /// capabilities/
    /// ├── mcp/
    /// │   ├── github/
    /// │   │   └── capabilities.rtfs
    /// │   └── slack/
    /// │       └── capabilities.rtfs
    /// └── other/
    ///     └── capabilities.rtfs
    /// ```
    ///
    /// # Arguments
    /// * `base_dir` - Optional base directory. Defaults to "capabilities" (in workspace root) or
    ///   the value of CCOS_CAPABILITY_STORAGE environment variable.
    ///
    /// # Returns
    /// The number of capabilities loaded.
    pub async fn load_discovered_capabilities<P: AsRef<Path>>(
        &self,
        base_dir: Option<P>,
    ) -> RuntimeResult<usize> {
        let dir = base_dir
            .map(|p| p.as_ref().to_path_buf())
            .unwrap_or_else(|| {
                std::env::var("CCOS_CAPABILITY_STORAGE")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|_| crate::utils::fs::get_workspace_root().join("capabilities"))
            });

        let mut total = 0usize;

        if dir.exists() {
            total += self
                .import_capabilities_from_rtfs_dir_recursive(&dir)
                .await?;
        } else if let Some(cb) = &self.debug_callback {
            cb(format!(
                "Discovered capabilities directory does not exist: {}",
                dir.display()
            ));
        }

        // Also load synthesized capabilities
        total += self.load_synthesized_capabilities().await?;

        Ok(total)
    }

    /// Load synthesized capabilities from the synthesized capabilities directory.
    ///
    /// Synthesized capabilities are inline RTFS code generated by the planner for
    /// transformations (group-by, filter, etc.) that have been saved for reuse.
    ///
    /// The directory structure is:
    /// ```text
    /// capabilities/synthesized/
    /// ├── group-issues-by-author-abc123/
    /// │   └── capability.rtfs
    /// └── filter-high-priority-def456/
    ///     └── capability.rtfs
    /// ```
    pub async fn load_synthesized_capabilities(&self) -> RuntimeResult<usize> {
        let dir = crate::synthesis::core::synthesized_capability_storage::get_synthesized_capability_storage_path();

        if !dir.exists() {
            return Ok(0);
        }

        if let Some(cb) = &self.debug_callback {
            cb(format!(
                "Loading synthesized capabilities from: {}",
                dir.display()
            ));
        }

        self.import_capabilities_from_rtfs_dir_recursive(&dir).await
    }

    /// Recursively import capabilities from RTFS files in a directory and its subdirectories.
    ///
    /// This method walks the directory tree and loads all `.rtfs` files it finds.
    pub async fn import_capabilities_from_rtfs_dir_recursive<P: AsRef<Path>>(
        &self,
        dir: P,
    ) -> RuntimeResult<usize> {
        let dir_path = dir.as_ref();

        if !dir_path.exists() {
            return Err(RuntimeError::Generic(format!(
                "Directory does not exist: {}",
                dir_path.display()
            )));
        }

        let mut total_loaded = 0usize;
        let mut dirs_to_process = vec![dir_path.to_path_buf()];

        while let Some(current_dir) = dirs_to_process.pop() {
            let entries = match std::fs::read_dir(&current_dir) {
                Ok(e) => e,
                Err(e) => {
                    if let Some(cb) = &self.debug_callback {
                        cb(format!(
                            "Failed to read directory {}: {}",
                            current_dir.display(),
                            e
                        ));
                    }
                    continue;
                }
            };

            for entry in entries {
                let entry = match entry {
                    Ok(e) => e,
                    Err(e) => {
                        if let Some(cb) = &self.debug_callback {
                            cb(format!(
                                "Failed to read entry in {}: {}",
                                current_dir.display(),
                                e
                            ));
                        }
                        continue;
                    }
                };

                let path = entry.path();

                // If it's a directory, add it to the queue for processing
                if path.is_dir() {
                    dirs_to_process.push(path);
                    continue;
                }

                // Skip non-rtfs files
                if path
                    .extension()
                    .and_then(|s| s.to_str())
                    .map_or(true, |ext| ext != "rtfs")
                {
                    continue;
                }

                // Load the RTFS file
                match self.import_single_rtfs_file(&path).await {
                    Ok(count) => {
                        total_loaded += count;
                        // Only log to debug callback, not eprintln
                        if let Some(cb) = &self.debug_callback {
                            cb(format!(
                                "Loaded {} capabilities from {}",
                                count,
                                path.display()
                            ));
                        }
                    }
                    Err(e) => {
                        ccos_eprintln!("❌ Failed to load {}: {}", path.display(), e);
                        if let Some(cb) = &self.debug_callback {
                            cb(format!("Failed to load {}: {}", path.display(), e));
                        }
                    }
                }
            }
        }

        Ok(total_loaded)
    }

    /// Import capabilities from a single RTFS file.
    pub async fn import_single_rtfs_file<P: AsRef<Path>>(&self, path: P) -> RuntimeResult<usize> {
        let path = path.as_ref();

        let path_str = path
            .to_str()
            .ok_or_else(|| RuntimeError::Generic(format!("Non-UTF8 path: {}", path.display())))?;

        let parser = MCPDiscoveryProvider::new_with_rtfs_host_factory(
            MCPServerConfig::default(),
            self.get_rtfs_host_factory(),
        )
        .map_err(|e| RuntimeError::Generic(format!("Failed to initialize RTFS parser: {}", e)))?;

        let module = parser.load_rtfs_capabilities(path_str)?;
        let mut loaded = 0usize;

        for cap_def in module.capabilities {
            match parser.rtfs_to_capability_manifest(&cap_def) {
                Ok(manifest) => {
                    // Use update_capability for proper version tracking
                    match self.update_capability(manifest, false).await {
                        Ok(result) => {
                            if result.updated {
                                if let Some(cb) = &self.debug_callback {
                                    cb(format!(
                                        "Updated capability: {} (version comparison: {:?})",
                                        result
                                            .previous_version
                                            .as_ref()
                                            .unwrap_or(&"unknown".to_string()),
                                        result.version_comparison
                                    ));
                                }
                            }
                            loaded += 1;
                        }
                        Err(e) => {
                            // If update fails due to breaking changes, log and skip
                            if let Some(cb) = &self.debug_callback {
                                cb(format!(
                                    "Skipping capability update due to breaking changes: {}",
                                    e
                                ));
                            }
                        }
                    }
                }
                Err(err) => {
                    if let Some(cb) = &self.debug_callback {
                        cb(format!(
                            "Failed to convert RTFS capability in {}: {}",
                            path.display(),
                            err
                        ));
                    }
                }
            }
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

        let entries = std::fs::read_dir(dir_path).map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to read RTFS dir {}: {}",
                dir_path.display(),
                e
            ))
        })?;

        let parser = MCPDiscoveryProvider::new_with_rtfs_host_factory(
            MCPServerConfig::default(),
            self.get_rtfs_host_factory(),
        )
        .map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to initialize RTFS parser for {}: {}",
                dir_path.display(),
                e
            ))
        })?;

        let mut loaded = 0usize;

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    if let Some(cb) = &self.debug_callback {
                        cb(format!(
                            "Failed to read entry in {}: {}",
                            dir_path.display(),
                            e
                        ));
                    }
                    continue;
                }
            };

            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if path
                .extension()
                .and_then(|s| s.to_str())
                .map_or(true, |ext| ext != "rtfs")
            {
                continue;
            }
            // Skip directory listing files (capabilities.rtfs) - these are not individual capabilities
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .map_or(false, |name| name == "capabilities.rtfs")
            {
                if let Some(cb) = &self.debug_callback {
                    cb(format!(
                        "Skipping directory listing file: {}",
                        path.display()
                    ));
                }
                continue;
            }

            let path_str = match path.to_str() {
                Some(s) => s,
                None => {
                    if let Some(cb) = &self.debug_callback {
                        cb(format!(
                            "Skipping RTFS entry with non-UTF8 path {}",
                            path.display()
                        ));
                    }
                    continue;
                }
            };

            match parser.load_rtfs_capabilities(path_str) {
                Ok(module) => {
                    for cap_def in module.capabilities {
                        match parser.rtfs_to_capability_manifest(&cap_def) {
                            Ok(manifest) => {
                                let mut caps = self.capabilities.write().await;
                                caps.insert(manifest.id.clone(), manifest);
                                loaded += 1;
                            }
                            Err(err) => {
                                if let Some(cb) = &self.debug_callback {
                                    cb(format!(
                                        "Failed to convert RTFS capability in {}: {}",
                                        path.display(),
                                        err
                                    ));
                                }
                            }
                        }
                    }
                }
                Err(err) => {
                    if let Some(cb) = &self.debug_callback {
                        cb(format!(
                            "Failed to parse RTFS file {}: {}",
                            path.display(),
                            err
                        ));
                    }
                }
            }
        }

        Ok(loaded)
    }

    /// Add native capability provider to the marketplace
    pub fn add_native_provider(&mut self, native_provider: NativeCapabilityProvider) {
        // Store the native provider in the executor registry
        self.executor_registry.insert(
            TypeId::of::<NativeCapability>(),
            ExecutorVariant::Native(native_provider),
        );
    }

    /// Get the native capability provider if available
    pub fn get_native_provider(&self) -> Option<&NativeCapabilityProvider> {
        self.executor_registry
            .get(&TypeId::of::<NativeCapability>())
            .and_then(|variant| match variant {
                ExecutorVariant::Native(provider) => Some(provider),
                _ => None,
            })
    }

    /// Register a native capability through the native provider
    pub fn register_native_capability_via_provider(
        &mut self,
        id: String,
        name: String,
        description: String,
        handler: Arc<dyn Fn(&Value) -> BoxFuture<'static, RuntimeResult<Value>> + Send + Sync>,
        security_level: String,
    ) -> RuntimeResult<()> {
        if let Some(native_provider) = self.get_native_provider_mut() {
            native_provider.register_native_capability_with_metadata(
                id,
                name,
                description,
                handler,
                security_level,
            )
        } else {
            Err(RuntimeError::Generic(
                "Native capability provider not available".to_string(),
            ))
        }
    }

    /// Register an RTFS capability through the native provider
    pub fn register_rtfs_capability_via_provider(
        &mut self,
        id: String,
        name: String,
        description: String,
        rtfs_function: String,
        security_level: String,
    ) -> RuntimeResult<()> {
        if let Some(native_provider) = self.get_native_provider_mut() {
            native_provider.register_rtfs_capability_with_metadata(
                id,
                name,
                description,
                rtfs_function,
                security_level,
            )
        } else {
            Err(RuntimeError::Generic(
                "Native capability provider not available".to_string(),
            ))
        }
    }

    /// Get mutable reference to native capability provider
    fn get_native_provider_mut(&mut self) -> Option<&mut NativeCapabilityProvider> {
        self.executor_registry
            .get_mut(&TypeId::of::<NativeCapability>())
            .and_then(|variant| match variant {
                ExecutorVariant::Native(provider) => Some(provider),
                _ => None,
            })
    }
}
