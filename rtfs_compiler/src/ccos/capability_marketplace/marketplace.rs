use super::executors::{A2AExecutor, ExecutorVariant, HttpExecutor, LocalExecutor, MCPExecutor};
use super::resource_monitor::ResourceMonitor;
use super::types::*;
// Temporarily disabled to fix resource monitoring tests
// use super::network_discovery::{NetworkDiscoveryProvider, NetworkDiscoveryBuilder};
// use super::mcp_discovery::{MCPDiscoveryProvider, MCPDiscoveryBuilder, MCPServerConfig};
// use super::a2a_discovery::{A2ADiscoveryProvider, A2ADiscoveryBuilder, A2AAgentConfig};
use crate::ast::{MapKey, TypeExpr};
use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::runtime::security::RuntimeContext;
use crate::runtime::streaming::{StreamType, StreamingProvider};
use crate::runtime::type_validator::{TypeCheckingConfig, TypeValidator, VerificationContext};
use crate::runtime::values::Value;
use chrono::Utc;
use std::any::TypeId;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

impl CapabilityMarketplace {
    pub fn new(
        capability_registry: Arc<
            RwLock<crate::runtime::capabilities::registry::CapabilityRegistry>,
        >,
    ) -> Self {
        Self::with_causal_chain(capability_registry, None)
    }

    pub fn with_causal_chain(
        capability_registry: Arc<
            RwLock<crate::runtime::capabilities::registry::CapabilityRegistry>,
        >,
        causal_chain: Option<Arc<std::sync::Mutex<crate::ccos::causal_chain::CausalChain>>>,
    ) -> Self {
        Self::with_causal_chain_and_debug_callback(capability_registry, causal_chain, None)
    }

    pub fn with_causal_chain_and_debug_callback(
        capability_registry: Arc<
            RwLock<crate::runtime::capabilities::registry::CapabilityRegistry>,
        >,
        causal_chain: Option<Arc<std::sync::Mutex<crate::ccos::causal_chain::CausalChain>>>,
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
        marketplace
    }

    /// Set a debug callback function to receive debug messages instead of printing to stderr
    pub fn set_debug_callback<F>(&mut self, callback: F)
    where
        F: Fn(String) + Send + Sync + 'static,
    {
        self.debug_callback = Some(Arc::new(callback));
    }

    /// Create marketplace with resource monitoring enabled
    pub fn with_resource_monitoring(
        capability_registry: Arc<
            RwLock<crate::runtime::capabilities::registry::CapabilityRegistry>,
        >,
        causal_chain: Option<Arc<std::sync::Mutex<crate::ccos::causal_chain::CausalChain>>>,
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
        crate::runtime::stdlib::register_default_capabilities(self).await?;

        // Load built-in capabilities from the capability registry
        let registry = self.capability_registry.read().await;

        // Get all registered capabilities from the registry
        for capability_id in registry.list_capabilities() {
            let capability_id = capability_id.to_string();
            let _capability_opt = registry.get_capability(&capability_id);
            let capability_id_clone = capability_id.clone();
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
                provider: ProviderType::Local(LocalCapability {
                    handler: Arc::new(move |_| {
                        // For now, just return an error indicating the capability needs to be executed via registry
                        Err(RuntimeError::Generic(format!(
                            "Capability '{}' should be executed via registry, not marketplace",
                            capability_id_clone
                        )))
                    }),
                }),
                version: "1.0.0".to_string(),
                input_schema: None,
                output_schema: None,
                attestation: None,
                provenance: Some(provenance),
                permissions: vec![],
                metadata: HashMap::new(),
            };

            let mut caps = self.capabilities.write().await;
            caps.insert(capability_id.clone(), manifest);
        }

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
    ) -> Result<(), RuntimeError> {
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
            metadata: HashMap::new(),
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
    async fn emit_capability_audit_event(
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
                "capability_registered" => crate::ccos::types::ActionType::CapabilityRegistered,
                "capability_removed" => crate::ccos::types::ActionType::CapabilityRemoved,
                "capability_updated" => crate::ccos::types::ActionType::CapabilityUpdated,
                "capability_discovery_completed" => {
                    crate::ccos::types::ActionType::CapabilityDiscoveryCompleted
                }
                _ => crate::ccos::types::ActionType::CapabilityCall, // fallback
            };

            let action = crate::ccos::types::Action {
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
                        crate::runtime::values::Value::String(capability_id.to_string()),
                    );
                    meta.insert(
                        "event_type".to_string(),
                        crate::runtime::values::Value::String(event_type.to_string()),
                    );
                    for (k, v) in event_data {
                        meta.insert(k, crate::runtime::values::Value::String(v));
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
            metadata: HashMap::new(),
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
            metadata: HashMap::new(),
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
            metadata: HashMap::new(),
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
            metadata: HashMap::new(),
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
            metadata: HashMap::new(),
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
            metadata: HashMap::new(),
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
            metadata: HashMap::new(),
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
            metadata: HashMap::new(),
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
            metadata: HashMap::new(),
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
            metadata: HashMap::new(),
        };
        let mut caps = self.capabilities.write().await;
        caps.insert(id, capability);
        Ok(())
    }

    pub async fn start_stream_with_config(
        &self,
        capability_id: &str,
        params: &Value,
        config: &crate::runtime::streaming::StreamConfig,
    ) -> RuntimeResult<crate::runtime::streaming::StreamHandle> {
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
        config: &crate::runtime::streaming::StreamConfig,
    ) -> RuntimeResult<crate::runtime::streaming::StreamHandle> {
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

    /// Execute a capability with enhanced metadata support
    pub async fn execute_capability_enhanced(
        &self,
        id: &str,
        inputs: &Value,
        _metadata: Option<&crate::runtime::execution_outcome::CallMetadata>,
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
            // For marketplace fallback, use a controlled runtime context
            let runtime_context = RuntimeContext::controlled(vec![id.to_string()]);
            return registry.execute_capability_with_microvm(id, args, Some(&runtime_context));
        };

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
                ProviderType::Plugin(_) => std::any::TypeId::of::<PluginCapability>(),
                ProviderType::RemoteRTFS(_) => std::any::TypeId::of::<RemoteRTFSCapability>(),
                ProviderType::Stream(_) => std::any::TypeId::of::<StreamCapabilityImpl>(),
            }) {
            executor.execute(&manifest.provider, inputs).await
        } else {
            match &manifest.provider {
                ProviderType::Local(local) => (local.handler)(inputs),
                ProviderType::Http(http) => self.execute_http_capability(http, inputs).await,
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
                MapKey::Keyword(crate::ast::Keyword(key[1..].to_string()))
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
                ProviderType::MCP(_) => "mcp",
                ProviderType::A2A(_) => "a2a",
                ProviderType::Plugin(_) => "plugin",
                ProviderType::RemoteRTFS(_) => "remote_rtfs",
                ProviderType::Stream(_) => "stream",
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
                ProviderType::MCP(_) => "mcp",
                ProviderType::A2A(_) => "a2a",
                ProviderType::Plugin(_) => "plugin",
                ProviderType::RemoteRTFS(_) => "remote_rtfs",
                ProviderType::Stream(_) => "stream",
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
}
