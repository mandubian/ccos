//! Native Capability Provider - exposes CLI commands as RTFS-callable capabilities

#![allow(dead_code)]

use crate::capabilities::provider::{CapabilityDescriptor, CapabilityProvider, ExecutionContext};
use crate::capability_marketplace::types::NativeCapability;
use crate::ops;
use futures::future::{BoxFuture, FutureExt};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Get the default storage path for approval queue from config
#[allow(dead_code)]
fn get_default_storage_path() -> PathBuf {
    let config = rtfs::config::AgentConfig::from_env();
    let workspace_root = std::env::var("CCOS_WORKSPACE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    workspace_root.join(&config.storage.approvals_dir)
}

/// Native capability provider that exposes CLI operations as RTFS capabilities
#[derive(Debug)]
pub struct NativeCapabilityProvider {
    capabilities: HashMap<String, NativeCapability>,
}

impl Default for NativeCapabilityProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeCapabilityProvider {
    /// Create a new native capability provider
    pub fn new() -> Self {
        let mut capabilities = HashMap::new();

        // Register all CLI capabilities
        /*
        capabilities.insert(
            "ccos.cli.server.list".to_string(),
            create_server_list_capability(),
        );
        capabilities.insert(
            "ccos.cli.server.add".to_string(),
            create_server_add_capability(),
        );
        capabilities.insert(
            "ccos.cli.server.remove".to_string(),
            create_server_remove_capability(),
        );
        capabilities.insert(
            "ccos.cli.server.health".to_string(),
            create_server_health_capability(),
        );
        capabilities.insert(
            "ccos.cli.server.search".to_string(),
            create_server_search_capability(),
        );

        capabilities.insert(
            "ccos.cli.discovery.goal".to_string(),
            create_discovery_goal_capability(),
        );
        capabilities.insert(
            "ccos.cli.discovery.search".to_string(),
            create_discovery_search_capability(),
        );
        capabilities.insert(
            "ccos.cli.discovery.inspect".to_string(),
            create_discovery_inspect_capability(),
        );

        capabilities.insert(
            "ccos.cli.approval.pending".to_string(),
            create_approval_pending_capability(),
        );
        capabilities.insert(
            "ccos.cli.approval.approve".to_string(),
            create_approval_approve_capability(),
        );
        capabilities.insert(
            "ccos.cli.approval.reject".to_string(),
            create_approval_reject_capability(),
        );
        capabilities.insert(
            "ccos.cli.approval.timeout".to_string(),
            create_approval_timeout_capability(),
        );

        capabilities.insert(
            "ccos.cli.config.show".to_string(),
            create_config_show_capability(),
        );
        capabilities.insert(
            "ccos.cli.config.validate".to_string(),
            create_config_validate_capability(),
        );
        capabilities.insert(
            "ccos.cli.config.init".to_string(),
            create_config_init_capability(),
        );

        capabilities.insert(
            "ccos.cli.governance.check".to_string(),
            create_governance_check_capability(),
        );
        capabilities.insert(
            "ccos.cli.governance.audit".to_string(),
            create_governance_audit_capability(),
        );
        capabilities.insert(
            "ccos.cli.governance.constitution".to_string(),
            create_governance_constitution_capability(),
        );

        capabilities.insert(
            "ccos.cli.plan.create".to_string(),
            create_plan_create_capability(),
        );
        capabilities.insert(
            "ccos.cli.plan.execute".to_string(),
            create_plan_execute_capability(),
        );
        capabilities.insert(
            "ccos.cli.plan.validate".to_string(),
            create_plan_validate_capability(),
        );
        */

        // LLM capabilities
        capabilities.insert(
            "ccos.llm.generate".to_string(),
            create_llm_generate_capability(),
        );

        Self { capabilities }
    }

    /// Register a new native capability dynamically (Rust-level)
    pub fn register_native_capability(
        &mut self,
        id: String,
        handler: Arc<dyn Fn(&Value) -> BoxFuture<'static, RuntimeResult<Value>> + Send + Sync>,
        security_level: String,
    ) {
        let capability = NativeCapability {
            handler,
            security_level,
            metadata: HashMap::new(),
        };
        self.capabilities.insert(id, capability);
    }

    /// Register a new native capability with full metadata (Rust-level)
    pub fn register_native_capability_with_metadata(
        &mut self,
        id: String,
        name: String,
        description: String,
        handler: Arc<dyn Fn(&Value) -> BoxFuture<'static, RuntimeResult<Value>> + Send + Sync>,
        security_level: String,
    ) -> RuntimeResult<()> {
        let mut metadata = HashMap::new();
        metadata.insert("name".to_string(), name);
        metadata.insert("description".to_string(), description);

        let capability = NativeCapability {
            handler,
            security_level,
            metadata,
        };
        self.capabilities.insert(id, capability);
        Ok(())
    }

    /// Register a new native capability from RTFS function (RTFS-level)
    /// This is used when agents want to register new capabilities dynamically
    pub fn register_rtfs_capability(
        &mut self,
        id: String,
        rtfs_function: String,
        security_level: String,
    ) -> RuntimeResult<()> {
        // Parse the RTFS function and create a handler
        // This would typically use the RTFS evaluator to compile the function
        // For now, we'll create a simple handler that can call RTFS functions

        let rtfs_fn = rtfs_function.clone();
        let handler = Arc::new(
            move |inputs: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
                let inputs = inputs.clone();
                let rtfs_fn = rtfs_fn.clone();
                async move {
                    // In a real implementation, this would:
                    // 1. Parse the RTFS function
                    // 2. Create an execution context
                    // 3. Call the RTFS evaluator with the inputs
                    // 4. Return the result

                    // For now, return a placeholder result
                    Ok(Value::String(format!(
                        "RTFS function {} called with {:?}",
                        rtfs_fn, inputs
                    )))
                }
                .boxed()
            },
        );

        let capability = NativeCapability {
            handler,
            security_level,
            metadata: {
                let mut meta = HashMap::new();
                meta.insert("rtfs_function".to_string(), rtfs_function);
                meta.insert("source".to_string(), "rtfs_registration".to_string());
                meta
            },
        };

        self.capabilities.insert(id, capability);
        Ok(())
    }

    /// Register a new native capability from RTFS function with full metadata
    pub fn register_rtfs_capability_with_metadata(
        &mut self,
        id: String,
        name: String,
        description: String,
        rtfs_function: String,
        security_level: String,
    ) -> RuntimeResult<()> {
        let rtfs_fn = rtfs_function.clone();
        let handler = Arc::new(
            move |inputs: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
                let inputs = inputs.clone();
                let rtfs_fn = rtfs_fn.clone();
                async move {
                    Ok(Value::String(format!(
                        "RTFS function {} called with {:?}",
                        rtfs_fn, inputs
                    )))
                }
                .boxed()
            },
        );

        let mut metadata = HashMap::new();
        metadata.insert("name".to_string(), name);
        metadata.insert("description".to_string(), description);
        metadata.insert("rtfs_function".to_string(), rtfs_function);
        metadata.insert("source".to_string(), "rtfs_registration".to_string());

        let capability = NativeCapability {
            handler,
            security_level,
            metadata,
        };

        self.capabilities.insert(id, capability);
        Ok(())
    }

    /// Get a capability by ID
    pub fn get_capability(&self, id: &str) -> Option<&NativeCapability> {
        self.capabilities.get(id)
    }

    /// List all available native capabilities
    pub fn list_capabilities(&self) -> Vec<String> {
        self.capabilities.keys().cloned().collect()
    }
}

/// Get rich description for a capability to help LLM decomposition.
/// These descriptions are explicit about what the tool does and does NOT do
/// to prevent semantic confusion during intent decomposition.
fn get_capability_description(id: &str) -> &'static str {
    match id {
        // Server management (MCP registry, NOT filesystem)
        "ccos.cli.server.list" => 
            "List all MCP servers registered in the CCOS registry. \
             Returns server names, endpoints, and status. NOT for listing files or directories.",
        "ccos.cli.server.add" => 
            "Register a new MCP server endpoint to the CCOS registry. \
             Requires: name, endpoint URL. NOT for creating files or directories.",
        "ccos.cli.server.remove" => 
            "Remove an MCP server from the CCOS registry by name. \
             Only unregisters the server, does NOT delete files or directories.",
        "ccos.cli.server.health" => 
            "Check health status of registered MCP servers. \
             Returns connectivity and response time metrics.",
        "ccos.cli.server.search" => 
            "Search the MCP registry for servers matching a query. \
             Searches by name, description, or capabilities.",
        
        // Discovery (introspection of capabilities, NOT filesystem)
        "ccos.cli.discovery.goal" => 
            "Find capabilities that match a natural language goal description. \
             Returns matching tools from the capability catalog.",
        "ccos.cli.discovery.search" => 
            "Search for capabilities in the registry by keyword or semantic query. \
             NOT for searching files or directories.",
        "ccos.cli.discovery.inspect" => 
            "Inspect details of a registered capability including its schema, \
             permissions, and metadata. NOT for inspecting filesystem paths.",
        
        // Approval workflow
        "ccos.cli.approval.pending" => 
            "List pending approval requests in the CCOS approval queue. \
             Shows effects, LLM prompts, and synthesis requests awaiting review.",
        "ccos.cli.approval.approve" => 
            "Approve a pending request by ID. Allows the pending action to proceed.",
        "ccos.cli.approval.reject" => 
            "Reject a pending request by ID with an optional reason.",
        "ccos.cli.approval.timeout" => 
            "List approval requests that have exceeded their timeout period.",
        
        // Configuration
        "ccos.cli.config.show" => 
            "Display current CCOS configuration including LLM profiles, storage paths, and feature flags.",
        "ccos.cli.config.validate" => 
            "Validate the CCOS configuration file for syntax and semantic errors.",
        "ccos.cli.config.init" => 
            "Initialize a new CCOS configuration file with default settings.",
        
        // Governance
        "ccos.cli.governance.check" => 
            "Check a plan against governance policies before execution.",
        "ccos.cli.governance.constitution" => 
            "Display the current governance constitution and policy rules.",
        "ccos.cli.governance.audit" => 
            "Generate an audit trail report of past governed executions.",
        
        // Plan management
        "ccos.cli.plan.create" => 
            "Create a new RTFS plan from a natural language goal.",
        "ccos.cli.plan.validate" => 
            "Validate an RTFS plan for syntax and capability availability.",
        "ccos.cli.plan.execute" => 
            "Execute an RTFS plan with governance checks and approval workflow.",
        
        // LLM
        "ccos.llm.generate" => 
            "Generate text using the configured LLM provider. \
             Requires: prompt. Returns generated text response.",
        
        // Default fallback
        _ => ""
    }
}

impl CapabilityProvider for NativeCapabilityProvider {
    fn provider_id(&self) -> &str {
        "ccos.native"
    }

    fn list_capabilities(&self) -> Vec<CapabilityDescriptor> {
        self.capabilities
            .iter()
            .map(|(id, cap)| {
                // Use centralized description, then metadata, then generic fallback
                let rich_desc = get_capability_description(id);
                let description = if !rich_desc.is_empty() {
                    rich_desc.to_string()
                } else {
                    cap.metadata
                        .get("description")
                        .cloned()
                        .unwrap_or_else(|| format!("CCOS CLI capability: {}", id))
                };
                CapabilityDescriptor {
                    id: id.clone(),
                    description,
                    capability_type: CapabilityDescriptor::constrained_function_type(
                        vec![CapabilityDescriptor::non_empty_string_type()],
                        CapabilityDescriptor::non_empty_string_type(),
                        None,
                    ),
                    security_requirements: crate::capabilities::provider::SecurityRequirements {
                        permissions: vec![],
                        requires_microvm: false,
                        resource_limits: crate::capabilities::provider::ResourceLimits {
                            max_memory: None,
                            max_cpu_time: None,
                            max_disk_space: None,
                        },
                        network_access: crate::capabilities::provider::NetworkAccess::None,
                    },
                    metadata: cap.metadata.clone(),
                }
            })
            .collect()
    }

    fn execute_capability(
        &self,
        capability_id: &str,
        inputs: &Value,
        _context: &ExecutionContext,
    ) -> RuntimeResult<Value> {
        if let Some(capability) = self.capabilities.get(capability_id) {
            let future = (capability.handler)(inputs);

            // Handle async execution from potentially sync context
            match tokio::runtime::Handle::try_current() {
                Ok(handle) => {
                    // We are in a runtime
                    tokio::task::block_in_place(|| handle.block_on(future))
                }
                Err(_) => {
                    // We are not in a runtime, create one
                    tokio::runtime::Runtime::new()
                        .map_err(|e| {
                            RuntimeError::Generic(format!("Failed to create runtime: {}", e))
                        })?
                        .block_on(future)
                }
            }
        } else {
            Err(RuntimeError::Generic(format!(
                "Native capability not found: {}",
                capability_id
            )))
        }
    }

    fn initialize(
        &mut self,
        _config: &crate::capabilities::provider::ProviderConfig,
    ) -> Result<(), String> {
        Ok(())
    }

    fn health_check(&self) -> crate::capabilities::provider::HealthStatus {
        crate::capabilities::provider::HealthStatus::Healthy
    }

    fn metadata(&self) -> crate::capabilities::provider::ProviderMetadata {
        crate::capabilities::provider::ProviderMetadata {
            name: "CCOS Native Capability Provider".to_string(),
            version: "1.0.0".to_string(),
            description: "Provides native CLI capabilities as RTFS-callable functions".to_string(),
            author: "CCOS Team".to_string(),
            license: Some("MIT".to_string()),
            dependencies: vec![],
        }
    }
}

/// RTFS-level interface for native capability provider
/// This allows RTFS code to access the native provider functionality
pub struct NativeProviderInterface {
    provider: std::sync::Arc<std::sync::Mutex<NativeCapabilityProvider>>,
}

impl NativeProviderInterface {
    /// Create a new interface to the native provider
    pub fn new(provider: std::sync::Arc<std::sync::Mutex<NativeCapabilityProvider>>) -> Self {
        Self { provider }
    }

    /// Register a new capability from RTFS (exposed to RTFS runtime)
    pub fn register_capability(
        &self,
        id: String,
        rtfs_function: String,
        security_level: String,
    ) -> RuntimeResult<()> {
        let mut provider = self.provider.lock().unwrap();
        provider.register_rtfs_capability(id, rtfs_function, security_level)
    }

    /// List all available native capabilities (exposed to RTFS runtime)
    pub fn list_capabilities(&self) -> Vec<String> {
        let provider = self.provider.lock().unwrap();
        provider.list_capabilities()
    }

    /// Get capability info (exposed to RTFS runtime)
    pub fn get_capability_info(&self, id: &str) -> Option<serde_json::Value> {
        let provider = self.provider.lock().unwrap();
        if let Some(capability) = provider.get_capability(id) {
            Some(serde_json::json!({
                "id": id,
                "security_level": capability.security_level,
                "metadata": capability.metadata,
            }))
        } else {
            None
        }
    }
}

// ============================================================================
// Helper functions to create native capabilities
// ============================================================================

/// Helper to get a string from a Value map
/// Checks both String keys and Keyword keys for RTFS compatibility
fn get_string_param(inputs: &Value, key: &str) -> Result<String, RuntimeError> {
    match inputs {
        Value::Map(map) => {
            // Try String key first (from safe execution/HashMap conversion)
            if let Some(v) = map.get(&rtfs::ast::MapKey::String(key.to_string())) {
                if let Some(s) = v.as_string() {
                    return Ok(s.to_string());
                }
            }
            // Try Keyword key (from RTFS parsing: {:prompt "..."})
            if let Some(v) = map.get(&rtfs::ast::MapKey::Keyword(rtfs::ast::Keyword(
                key.to_string(),
            ))) {
                if let Some(s) = v.as_string() {
                    return Ok(s.to_string());
                }
            }
            Err(RuntimeError::Generic(format!("Missing parameter: {}", key)))
        }
        Value::String(s) if key == "value" => Ok(s.clone()),
        _ => Err(RuntimeError::Generic("Expected map input".to_string())),
    }
}

/// Helper to get an optional string from a Value map
/// Checks both String keys and Keyword keys for RTFS compatibility
fn get_optional_string_param(inputs: &Value, key: &str) -> Option<String> {
    match inputs {
        Value::Map(map) => {
            // Try String key first
            if let Some(v) = map.get(&rtfs::ast::MapKey::String(key.to_string())) {
                if let Some(s) = v.as_string() {
                    return Some(s.to_string());
                }
            }
            // Try Keyword key
            if let Some(v) = map.get(&rtfs::ast::MapKey::Keyword(rtfs::ast::Keyword(
                key.to_string(),
            ))) {
                if let Some(s) = v.as_string() {
                    return Some(s.to_string());
                }
            }
            None
        }
        _ => None,
    }
}

/// Helper to get any Value and serialize it to string (for LLM context)
/// Checks both String keys and Keyword keys for RTFS compatibility
fn get_optional_value_as_string(inputs: &Value, key: &str) -> Option<String> {
    match inputs {
        Value::Map(map) => {
            // Try String key first
            if let Some(v) = map.get(&rtfs::ast::MapKey::String(key.to_string())) {
                return Some(format!("{:?}", v));
            }
            // Try Keyword key
            if let Some(v) = map.get(&rtfs::ast::MapKey::Keyword(rtfs::ast::Keyword(
                key.to_string(),
            ))) {
                return Some(format!("{:?}", v));
            }
            None
        }
        _ => None,
    }
}

/// Helper to get a bool from a Value map
#[allow(dead_code)]
fn get_bool_param(inputs: &Value, key: &str, default: bool) -> bool {
    match inputs {
        Value::Map(map) => map
            .get(&rtfs::ast::MapKey::String(key.to_string()))
            .and_then(|v| match v {
                Value::Boolean(b) => Some(*b),
                Value::String(s) => s.parse::<bool>().ok(),
                _ => None,
            })
            .unwrap_or(default),
        _ => default,
    }
}

// Server capabilities

fn create_server_list_capability() -> NativeCapability {
    let handler = Arc::new(
        |_inputs: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
            async move {
                match ops::server::list_servers(get_default_storage_path()).await {
                    Ok(output) => {
                        let json = serde_json::to_string(&output).map_err(|e| {
                            RuntimeError::Generic(format!("Serialization error: {}", e))
                        })?;
                        Ok(Value::String(json))
                    }
                    Err(e) => Err(e),
                }
            }
            .boxed()
        },
    );

    NativeCapability {
        handler,
        security_level: "low".to_string(),
        metadata: HashMap::new(),
    }
}

fn create_server_add_capability() -> NativeCapability {
    let handler = Arc::new(
        |inputs: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
            let inputs = inputs.clone();
            async move {
                let url = get_string_param(&inputs, "url")?;
                let name = get_optional_string_param(&inputs, "name");

                match ops::server::add_server(get_default_storage_path(), url, name).await {
                    Ok(server_id) => Ok(Value::String(server_id)),
                    Err(e) => Err(e),
                }
            }
            .boxed()
        },
    );

    NativeCapability {
        handler,
        security_level: "medium".to_string(),
        metadata: HashMap::new(),
    }
}

fn create_server_remove_capability() -> NativeCapability {
    let handler = Arc::new(
        |inputs: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
            let inputs = inputs.clone();
            async move {
                let name = get_string_param(&inputs, "name")?;

                match ops::server::remove_server(get_default_storage_path(), &name).await {
                    Ok(_) => Ok(Value::String("Server removed successfully".to_string())),
                    Err(e) => Err(e),
                }
            }
            .boxed()
        },
    );

    NativeCapability {
        handler,
        security_level: "high".to_string(),
        metadata: HashMap::new(),
    }
}

fn create_server_health_capability() -> NativeCapability {
    let handler = Arc::new(
        |inputs: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
            let inputs = inputs.clone();
            async move {
                let name = get_optional_string_param(&inputs, "name");

                match ops::server::server_health(get_default_storage_path(), name).await {
                    Ok(health_info) => {
                        let json = serde_json::to_string(&health_info).map_err(|e| {
                            RuntimeError::Generic(format!("Serialization error: {}", e))
                        })?;
                        Ok(Value::String(json))
                    }
                    Err(e) => Err(e),
                }
            }
            .boxed()
        },
    );

    NativeCapability {
        handler,
        security_level: "low".to_string(),
        metadata: HashMap::new(),
    }
}

fn create_server_search_capability() -> NativeCapability {
    let handler = Arc::new(
        |inputs: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
            let inputs = inputs.clone();
            async move {
                let query = get_string_param(&inputs, "query")?;
                let capability = get_optional_string_param(&inputs, "capability");
                let llm = get_bool_param(&inputs, "llm", false);
                let llm_model = get_optional_string_param(&inputs, "llm_model");

                match ops::server::search_servers(query, capability, llm, llm_model).await {
                    Ok(search_results) => {
                        let json = serde_json::to_string(&search_results).map_err(|e| {
                            RuntimeError::Generic(format!("Serialization error: {}", e))
                        })?;
                        Ok(Value::String(json))
                    }
                    Err(e) => Err(e),
                }
            }
            .boxed()
        },
    );

    NativeCapability {
        handler,
        security_level: "low".to_string(),
        metadata: HashMap::new(),
    }
}

// Discovery capabilities

fn create_discovery_goal_capability() -> NativeCapability {
    let handler = Arc::new(
        |inputs: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
            let inputs = inputs.clone();
            async move {
                let goal = match inputs {
                    Value::String(s) => s.clone(),
                    _ => get_string_param(&inputs, "goal")?,
                };

                match ops::discover::discover_by_goal(get_default_storage_path(), goal).await {
                    Ok(discovery_results) => {
                        let json = serde_json::to_string(&discovery_results).map_err(|e| {
                            RuntimeError::Generic(format!("Serialization error: {}", e))
                        })?;
                        Ok(Value::String(json))
                    }
                    Err(e) => Err(e),
                }
            }
            .boxed()
        },
    );

    NativeCapability {
        handler,
        security_level: "low".to_string(),
        metadata: HashMap::new(),
    }
}

fn create_discovery_search_capability() -> NativeCapability {
    let handler = Arc::new(
        |inputs: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
            let inputs = inputs.clone();
            async move {
                let query = match inputs {
                    Value::String(s) => s.clone(),
                    _ => get_string_param(&inputs, "query")?,
                };

                match ops::discover::search_catalog(query).await {
                    Ok(search_results) => {
                        let json = serde_json::to_string(&search_results).map_err(|e| {
                            RuntimeError::Generic(format!("Serialization error: {}", e))
                        })?;
                        Ok(Value::String(json))
                    }
                    Err(e) => Err(e),
                }
            }
            .boxed()
        },
    );

    NativeCapability {
        handler,
        security_level: "low".to_string(),
        metadata: HashMap::new(),
    }
}

fn create_discovery_inspect_capability() -> NativeCapability {
    let handler = Arc::new(
        |inputs: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
            let inputs = inputs.clone();
            async move {
                let id = match inputs {
                    Value::String(s) => s.clone(),
                    _ => get_string_param(&inputs, "id")?,
                };

                match ops::discover::inspect_capability(id).await {
                    Ok(details) => Ok(Value::String(details)),
                    Err(e) => Err(e),
                }
            }
            .boxed()
        },
    );

    NativeCapability {
        handler,
        security_level: "low".to_string(),
        metadata: HashMap::new(),
    }
}

// Approval capabilities

fn create_approval_pending_capability() -> NativeCapability {
    let handler = Arc::new(
        |_inputs: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
            async move {
                match ops::approval::list_pending(get_default_storage_path()).await {
                    Ok(output) => {
                        let json = serde_json::to_string(&output).map_err(|e| {
                            RuntimeError::Generic(format!("Serialization error: {}", e))
                        })?;
                        Ok(Value::String(json))
                    }
                    Err(e) => Err(e),
                }
            }
            .boxed()
        },
    );

    NativeCapability {
        handler,
        security_level: "low".to_string(),
        metadata: HashMap::new(),
    }
}

fn create_approval_approve_capability() -> NativeCapability {
    let handler = Arc::new(
        |inputs: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
            let inputs = inputs.clone();
            async move {
                let (id, reason) = match inputs {
                    Value::String(s) => (s.clone(), None),
                    _ => (
                        get_string_param(&inputs, "id")?,
                        get_string_param(&inputs, "reason").ok(),
                    ),
                };

                match ops::approval::approve_discovery(get_default_storage_path(), id, reason).await
                {
                    Ok(_) => Ok(Value::String("Approval successful".to_string())),
                    Err(e) => Err(e),
                }
            }
            .boxed()
        },
    );

    NativeCapability {
        handler,
        security_level: "high".to_string(),
        metadata: HashMap::new(),
    }
}

fn create_approval_reject_capability() -> NativeCapability {
    let handler = Arc::new(
        |inputs: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
            let inputs = inputs.clone();
            async move {
                let (id, reason) = match inputs {
                    Value::String(s) => (s.clone(), "Rejected via capability".to_string()),
                    _ => (
                        get_string_param(&inputs, "id")?,
                        get_string_param(&inputs, "reason")
                            .unwrap_or_else(|_| "Rejected via capability".to_string()),
                    ),
                };

                match ops::approval::reject_discovery(get_default_storage_path(), id, reason).await
                {
                    Ok(_) => Ok(Value::String("Rejection successful".to_string())),
                    Err(e) => Err(e),
                }
            }
            .boxed()
        },
    );

    NativeCapability {
        handler,
        security_level: "medium".to_string(),
        metadata: HashMap::new(),
    }
}

fn create_approval_timeout_capability() -> NativeCapability {
    let handler = Arc::new(
        |_inputs: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
            async move {
                match ops::approval::list_timeout(get_default_storage_path()).await {
                    Ok(output) => {
                        let json = serde_json::to_string(&output).map_err(|e| {
                            RuntimeError::Generic(format!("Serialization error: {}", e))
                        })?;
                        Ok(Value::String(json))
                    }
                    Err(e) => Err(e),
                }
            }
            .boxed()
        },
    );

    NativeCapability {
        handler,
        security_level: "low".to_string(),
        metadata: HashMap::new(),
    }
}

// Config capabilities

fn create_config_show_capability() -> NativeCapability {
    let handler = Arc::new(
        |inputs: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
            let inputs = inputs.clone();
            async move {
                let config_path = match inputs {
                    Value::String(s) => std::path::PathBuf::from(s),
                    _ => get_optional_string_param(&inputs, "config_path")
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(|| std::path::PathBuf::from("agent_config.toml")),
                };

                match ops::config::show_config(config_path).await {
                    Ok(config_info) => {
                        let json = serde_json::to_string(&config_info).map_err(|e| {
                            RuntimeError::Generic(format!("Serialization error: {}", e))
                        })?;
                        Ok(Value::String(json))
                    }
                    Err(e) => Err(e),
                }
            }
            .boxed()
        },
    );

    NativeCapability {
        handler,
        security_level: "low".to_string(),
        metadata: HashMap::new(),
    }
}

fn create_config_validate_capability() -> NativeCapability {
    let handler = Arc::new(
        |inputs: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
            let inputs = inputs.clone();
            async move {
                let config_path = match inputs {
                    Value::String(s) => std::path::PathBuf::from(s),
                    _ => get_optional_string_param(&inputs, "config_path")
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(|| std::path::PathBuf::from("agent_config.toml")),
                };

                match ops::config::validate_config(config_path).await {
                    Ok(config_info) => {
                        let json = serde_json::to_string(&config_info).map_err(|e| {
                            RuntimeError::Generic(format!("Serialization error: {}", e))
                        })?;
                        Ok(Value::String(json))
                    }
                    Err(e) => Err(e),
                }
            }
            .boxed()
        },
    );

    NativeCapability {
        handler,
        security_level: "low".to_string(),
        metadata: HashMap::new(),
    }
}

fn create_config_init_capability() -> NativeCapability {
    let handler = Arc::new(
        |inputs: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
            let inputs = inputs.clone();
            async move {
                let output_path = get_optional_string_param(&inputs, "output")
                    .unwrap_or_else(|| "agent_config.toml".to_string());
                let _force = get_bool_param(&inputs, "force", false);

                // For now, just return success - actual init logic is in CLI
                Ok(Value::String(format!(
                    "Config initialization requested for: {}",
                    output_path
                )))
            }
            .boxed()
        },
    );

    NativeCapability {
        handler,
        security_level: "critical".to_string(),
        metadata: HashMap::new(),
    }
}

// Governance capabilities

fn create_governance_check_capability() -> NativeCapability {
    let handler = Arc::new(
        |inputs: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
            let inputs = inputs.clone();
            async move {
                let action = match inputs {
                    Value::String(s) => s.clone(),
                    _ => get_string_param(&inputs, "action")?,
                };

                match ops::governance::check_action(action).await {
                    Ok(allowed) => Ok(Value::Boolean(allowed)),
                    Err(e) => Err(e),
                }
            }
            .boxed()
        },
    );

    NativeCapability {
        handler,
        security_level: "low".to_string(),
        metadata: HashMap::new(),
    }
}

fn create_governance_audit_capability() -> NativeCapability {
    let handler = Arc::new(
        |_inputs: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
            async move {
                match ops::governance::view_audit().await {
                    Ok(audit_trail) => Ok(Value::String(audit_trail)),
                    Err(e) => Err(e),
                }
            }
            .boxed()
        },
    );

    NativeCapability {
        handler,
        security_level: "low".to_string(),
        metadata: HashMap::new(),
    }
}

fn create_governance_constitution_capability() -> NativeCapability {
    let handler = Arc::new(
        |_inputs: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
            async move {
                match ops::governance::view_constitution().await {
                    Ok(constitution) => Ok(Value::String(constitution)),
                    Err(e) => Err(e),
                }
            }
            .boxed()
        },
    );

    NativeCapability {
        handler,
        security_level: "critical".to_string(),
        metadata: HashMap::new(),
    }
}

// Plan capabilities

fn create_plan_create_capability() -> NativeCapability {
    let handler = Arc::new(
        |inputs: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
            let inputs = inputs.clone();
            async move {
                let goal = match inputs {
                    Value::String(s) => s.clone(),
                    _ => get_string_param(&inputs, "goal")?,
                };

                // Use spawn_blocking to run in a separate thread with its own runtime
                // This ensures Send requirements are met
                let goal_clone = goal.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let rt = tokio::runtime::Runtime::new().map_err(|e| {
                        RuntimeError::Generic(format!("Failed to create runtime: {}", e))
                    })?;
                    rt.block_on(ops::plan::create_plan(goal_clone))
                })
                .await;

                match result {
                    Ok(Ok(plan)) => Ok(Value::String(plan)),
                    Ok(Err(e)) => Err(e),
                    Err(e) => Err(RuntimeError::Generic(format!("Task join error: {}", e))),
                }
            }
            .boxed()
        },
    );

    NativeCapability {
        handler,
        security_level: "medium".to_string(),
        metadata: HashMap::new(),
    }
}

fn create_plan_execute_capability() -> NativeCapability {
    let handler = Arc::new(
        |inputs: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
            let inputs = inputs.clone();
            async move {
                let plan = match inputs {
                    Value::String(s) => s.clone(),
                    _ => get_string_param(&inputs, "plan")?,
                };

                // Use spawn_blocking to run in a separate thread with its own runtime
                // This ensures Send requirements are met
                let plan_clone = plan.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let rt = tokio::runtime::Runtime::new().map_err(|e| {
                        RuntimeError::Generic(format!("Failed to create runtime: {}", e))
                    })?;
                    rt.block_on(ops::plan::execute_plan(plan_clone))
                })
                .await;

                match result {
                    Ok(Ok(result)) => Ok(Value::String(result)),
                    Ok(Err(e)) => Err(e),
                    Err(e) => Err(RuntimeError::Generic(format!("Task join error: {}", e))),
                }
            }
            .boxed()
        },
    );

    NativeCapability {
        handler,
        security_level: "high".to_string(),
        metadata: HashMap::new(),
    }
}

fn create_plan_validate_capability() -> NativeCapability {
    let handler = Arc::new(
        |inputs: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
            let inputs = inputs.clone();
            async move {
                let plan = match inputs {
                    Value::String(s) => s.clone(),
                    _ => get_string_param(&inputs, "plan")?,
                };

                match ops::plan::validate_plan(plan).await {
                    Ok(valid) => Ok(Value::Boolean(valid)),
                    Err(e) => Err(e),
                }
            }
            .boxed()
        },
    );

    NativeCapability {
        handler,
        security_level: "low".to_string(),
        metadata: HashMap::new(),
    }
}

// LLM capabilities

fn create_llm_generate_capability() -> NativeCapability {
    let handler = Arc::new(
        |inputs: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
            let inputs = inputs.clone();
            async move {
                let prompt = get_string_param(&inputs, "prompt")?;
                // Accept both :context and :data (fallback)
                let context = get_optional_string_param(&inputs, "context")
                    .or_else(|| get_optional_string_param(&inputs, "data"))
                    .or_else(|| {
                        // If :data is a non-string value, serialize it
                        get_optional_value_as_string(&inputs, "data")
                    });
                let max_tokens = get_optional_u32_param(&inputs, "max_tokens");
                let temperature = get_optional_f32_param(&inputs, "temperature");

                let request = ops::llm::LlmGenerateRequest {
                    prompt,
                    context,
                    max_tokens,
                    temperature,
                };

                match ops::llm::llm_generate(request).await {
                    Ok(response) => {
                        if response.approval_required {
                            // Return a structured response indicating approval needed
                            Ok(Value::Map({
                                let mut map = std::collections::HashMap::new();
                                map.insert(
                                    rtfs::ast::MapKey::String("approval_required".to_string()),
                                    Value::Boolean(true),
                                );
                                if let Some(reason) = response.approval_reason {
                                    map.insert(
                                        rtfs::ast::MapKey::String("reason".to_string()),
                                        Value::String(reason),
                                    );
                                }
                                map
                            }))
                        } else {
                            Ok(Value::String(response.text))
                        }
                    }
                    Err(e) => Err(e),
                }
            }
            .boxed()
        },
    );

    let mut metadata = HashMap::new();
    metadata.insert("name".to_string(), "LLM Generate".to_string());
    metadata.insert(
        "description".to_string(),
        "LLM text generation for summarization, analysis, explanation, translation. Use for summarizing data, extracting insights, generating human-readable text from structured data.".to_string(),
    );
    metadata.insert(
        "keywords".to_string(),
        "summarize summary analyze analysis explain translate generate text".to_string(),
    );

    NativeCapability {
        handler,
        security_level: "medium".to_string(),
        metadata,
    }
}

/// Helper to get an optional u32 from a Value map
fn get_optional_u32_param(inputs: &Value, key: &str) -> Option<u32> {
    match inputs {
        Value::Map(map) => map
            .get(&rtfs::ast::MapKey::String(key.to_string()))
            .and_then(|v| match v {
                Value::Integer(i) => Some(*i as u32),
                Value::Float(f) => Some(*f as u32),
                Value::String(s) => s.parse().ok(),
                _ => None,
            }),
        _ => None,
    }
}

/// Helper to get an optional f32 from a Value map
fn get_optional_f32_param(inputs: &Value, key: &str) -> Option<f32> {
    match inputs {
        Value::Map(map) => map
            .get(&rtfs::ast::MapKey::String(key.to_string()))
            .and_then(|v| match v {
                Value::Float(f) => Some(*f as f32),
                Value::Integer(i) => Some(*i as f32),
                Value::String(s) => s.parse().ok(),
                _ => None,
            }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtfs::runtime::values::Value;

    #[tokio::test]
    async fn test_native_provider_integration() {
        let provider = NativeCapabilityProvider::new();

        // Test listing capabilities
        let capabilities = provider.list_capabilities();
        assert!(!capabilities.is_empty());
        assert!(capabilities.iter().any(|c| c == "ccos.cli.server.list"));

        // Test executing a capability (config show)
        // We use config show because it's safe and doesn't require complex inputs
        let inputs = Value::Map(HashMap::new());

        // Execute capability via async executor path (emulating ExecutorVariant)
        if let Some(capability) = provider.get_capability("ccos.cli.config.show") {
            let result = (capability.handler)(&inputs).await;
            assert!(result.is_ok());
            let value = result.unwrap();
            match value {
                Value::String(s) => {
                    assert!(s.contains("config_path"));
                }
                _ => panic!("Expected string result"),
            }
        } else {
            panic!("Capability not found");
        }
    }
}
