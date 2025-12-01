//! Native Capability Provider - exposes CLI commands as RTFS-callable capabilities

use crate::capabilities::provider::{CapabilityDescriptor, CapabilityProvider, ExecutionContext};
use crate::capability_marketplace::types::NativeCapability;
use crate::ops;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use std::collections::HashMap;
use std::sync::Arc;

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

        Self { capabilities }
    }

    /// Register a new native capability dynamically (Rust-level)
    pub fn register_native_capability(
        &mut self,
        id: String,
        handler: Arc<dyn Fn(&Value) -> RuntimeResult<Value> + Send + Sync>,
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
        handler: Arc<dyn Fn(&Value) -> RuntimeResult<Value> + Send + Sync>,
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
        let handler = Arc::new(move |inputs: &Value| -> RuntimeResult<Value> {
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
        });

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
        let handler = Arc::new(move |inputs: &Value| -> RuntimeResult<Value> {
            Ok(Value::String(format!(
                "RTFS function {} called with {:?}",
                rtfs_fn, inputs
            )))
        });

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

impl CapabilityProvider for NativeCapabilityProvider {
    fn provider_id(&self) -> &str {
        "ccos.native"
    }

    fn list_capabilities(&self) -> Vec<CapabilityDescriptor> {
        self.capabilities
            .iter()
            .map(|(id, _)| CapabilityDescriptor {
                id: id.clone(),
                description: format!("CCOS CLI capability: {}", id),
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
                metadata: HashMap::new(),
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
            (capability.handler)(inputs)
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
fn get_string_param(inputs: &Value, key: &str) -> Result<String, RuntimeError> {
    match inputs {
        Value::Map(map) => map
            .get(&rtfs::ast::MapKey::String(key.to_string()))
            .and_then(|v| v.as_string())
            .map(|s| s.to_string())
            .ok_or_else(|| RuntimeError::Generic(format!("Missing parameter: {}", key))),
        Value::String(s) if key == "value" => Ok(s.clone()),
        _ => Err(RuntimeError::Generic("Expected map input".to_string())),
    }
}

/// Helper to get an optional string from a Value map
fn get_optional_string_param(inputs: &Value, key: &str) -> Option<String> {
    match inputs {
        Value::Map(map) => map
            .get(&rtfs::ast::MapKey::String(key.to_string()))
            .and_then(|v| v.as_string())
            .map(|s| s.to_string()),
        _ => None,
    }
}

/// Helper to get a bool from a Value map
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
    let handler = Arc::new(|_inputs: &Value| -> RuntimeResult<Value> {
        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create runtime: {}", e)))?;
        let result = runtime.block_on(async { ops::server::list_servers().await });
        match result {
            Ok(output) => {
                let json = serde_json::to_string(&output)
                    .map_err(|e| RuntimeError::Generic(format!("Serialization error: {}", e)))?;
                Ok(Value::String(json))
            }
            Err(e) => Err(e),
        }
    });

    NativeCapability {
        handler,
        security_level: "low".to_string(),
        metadata: HashMap::new(),
    }
}

fn create_server_add_capability() -> NativeCapability {
    let handler = Arc::new(|inputs: &Value| -> RuntimeResult<Value> {
        let url = get_string_param(inputs, "url")?;
        let name = get_optional_string_param(inputs, "name");

        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create runtime: {}", e)))?;
        let result = runtime.block_on(async { ops::server::add_server(url, name).await });

        match result {
            Ok(server_id) => Ok(Value::String(server_id)),
            Err(e) => Err(e),
        }
    });

    NativeCapability {
        handler,
        security_level: "medium".to_string(),
        metadata: HashMap::new(),
    }
}

fn create_server_remove_capability() -> NativeCapability {
    let handler = Arc::new(|inputs: &Value| -> RuntimeResult<Value> {
        let name = get_string_param(inputs, "name")?;

        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create runtime: {}", e)))?;
        let result = runtime.block_on(async { ops::server::remove_server(name).await });

        match result {
            Ok(_) => Ok(Value::String("Server removed successfully".to_string())),
            Err(e) => Err(e),
        }
    });

    NativeCapability {
        handler,
        security_level: "high".to_string(),
        metadata: HashMap::new(),
    }
}

fn create_server_health_capability() -> NativeCapability {
    let handler = Arc::new(|inputs: &Value| -> RuntimeResult<Value> {
        let name = get_optional_string_param(inputs, "name");

        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create runtime: {}", e)))?;
        let result = runtime.block_on(async { ops::server::server_health(name).await });

        match result {
            Ok(health_info) => {
                let json = serde_json::to_string(&health_info)
                    .map_err(|e| RuntimeError::Generic(format!("Serialization error: {}", e)))?;
                Ok(Value::String(json))
            }
            Err(e) => Err(e),
        }
    });

    NativeCapability {
        handler,
        security_level: "low".to_string(),
        metadata: HashMap::new(),
    }
}

fn create_server_search_capability() -> NativeCapability {
    let handler = Arc::new(|inputs: &Value| -> RuntimeResult<Value> {
        let query = get_string_param(inputs, "query")?;
        let capability = get_optional_string_param(inputs, "capability");
        let llm = get_bool_param(inputs, "llm", false);
        let llm_model = get_optional_string_param(inputs, "llm_model");

        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create runtime: {}", e)))?;
        let result = runtime.block_on(async {
            ops::server::search_servers(query, capability, llm, llm_model).await
        });

        match result {
            Ok(search_results) => {
                let json = serde_json::to_string(&search_results)
                    .map_err(|e| RuntimeError::Generic(format!("Serialization error: {}", e)))?;
                Ok(Value::String(json))
            }
            Err(e) => Err(e),
        }
    });

    NativeCapability {
        handler,
        security_level: "low".to_string(),
        metadata: HashMap::new(),
    }
}

// Discovery capabilities

fn create_discovery_goal_capability() -> NativeCapability {
    let handler = Arc::new(|inputs: &Value| -> RuntimeResult<Value> {
        let goal = match inputs {
            Value::String(s) => s.clone(),
            _ => get_string_param(inputs, "goal")?,
        };

        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create runtime: {}", e)))?;
        let result = runtime.block_on(async { ops::discover::discover_by_goal(goal).await });

        match result {
            Ok(discovery_results) => {
                let json = serde_json::to_string(&discovery_results)
                    .map_err(|e| RuntimeError::Generic(format!("Serialization error: {}", e)))?;
                Ok(Value::String(json))
            }
            Err(e) => Err(e),
        }
    });

    NativeCapability {
        handler,
        security_level: "low".to_string(),
        metadata: HashMap::new(),
    }
}

fn create_discovery_search_capability() -> NativeCapability {
    let handler = Arc::new(|inputs: &Value| -> RuntimeResult<Value> {
        let query = match inputs {
            Value::String(s) => s.clone(),
            _ => get_string_param(inputs, "query")?,
        };

        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create runtime: {}", e)))?;
        let result = runtime.block_on(async { ops::discover::search_catalog(query).await });

        match result {
            Ok(search_results) => {
                let json = serde_json::to_string(&search_results)
                    .map_err(|e| RuntimeError::Generic(format!("Serialization error: {}", e)))?;
                Ok(Value::String(json))
            }
            Err(e) => Err(e),
        }
    });

    NativeCapability {
        handler,
        security_level: "low".to_string(),
        metadata: HashMap::new(),
    }
}

fn create_discovery_inspect_capability() -> NativeCapability {
    let handler = Arc::new(|inputs: &Value| -> RuntimeResult<Value> {
        let id = match inputs {
            Value::String(s) => s.clone(),
            _ => get_string_param(inputs, "id")?,
        };

        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create runtime: {}", e)))?;
        let result = runtime.block_on(async { ops::discover::inspect_capability(id).await });

        match result {
            Ok(details) => Ok(Value::String(details)),
            Err(e) => Err(e),
        }
    });

    NativeCapability {
        handler,
        security_level: "low".to_string(),
        metadata: HashMap::new(),
    }
}

// Approval capabilities

fn create_approval_pending_capability() -> NativeCapability {
    let handler = Arc::new(|_inputs: &Value| -> RuntimeResult<Value> {
        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create runtime: {}", e)))?;
        let result = runtime.block_on(async { ops::approval::list_pending().await });

        match result {
            Ok(output) => {
                let json = serde_json::to_string(&output)
                    .map_err(|e| RuntimeError::Generic(format!("Serialization error: {}", e)))?;
                Ok(Value::String(json))
            }
            Err(e) => Err(e),
        }
    });

    NativeCapability {
        handler,
        security_level: "low".to_string(),
        metadata: HashMap::new(),
    }
}

fn create_approval_approve_capability() -> NativeCapability {
    let handler = Arc::new(|inputs: &Value| -> RuntimeResult<Value> {
        let id = match inputs {
            Value::String(s) => s.clone(),
            _ => get_string_param(inputs, "id")?,
        };

        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create runtime: {}", e)))?;
        let result = runtime.block_on(async { ops::approval::approve_discovery(id).await });

        match result {
            Ok(_) => Ok(Value::String("Approval successful".to_string())),
            Err(e) => Err(e),
        }
    });

    NativeCapability {
        handler,
        security_level: "high".to_string(),
        metadata: HashMap::new(),
    }
}

fn create_approval_reject_capability() -> NativeCapability {
    let handler = Arc::new(|inputs: &Value| -> RuntimeResult<Value> {
        let id = match inputs {
            Value::String(s) => s.clone(),
            _ => get_string_param(inputs, "id")?,
        };

        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create runtime: {}", e)))?;
        let result = runtime.block_on(async { ops::approval::reject_discovery(id).await });

        match result {
            Ok(_) => Ok(Value::String("Rejection successful".to_string())),
            Err(e) => Err(e),
        }
    });

    NativeCapability {
        handler,
        security_level: "medium".to_string(),
        metadata: HashMap::new(),
    }
}

fn create_approval_timeout_capability() -> NativeCapability {
    let handler = Arc::new(|_inputs: &Value| -> RuntimeResult<Value> {
        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create runtime: {}", e)))?;
        let result = runtime.block_on(async { ops::approval::list_timeout().await });

        match result {
            Ok(output) => {
                let json = serde_json::to_string(&output)
                    .map_err(|e| RuntimeError::Generic(format!("Serialization error: {}", e)))?;
                Ok(Value::String(json))
            }
            Err(e) => Err(e),
        }
    });

    NativeCapability {
        handler,
        security_level: "low".to_string(),
        metadata: HashMap::new(),
    }
}

// Config capabilities

fn create_config_show_capability() -> NativeCapability {
    let handler = Arc::new(|inputs: &Value| -> RuntimeResult<Value> {
        let config_path = match inputs {
            Value::String(s) => std::path::PathBuf::from(s),
            _ => get_optional_string_param(inputs, "config_path")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from("agent_config.toml")),
        };

        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create runtime: {}", e)))?;
        let result = runtime.block_on(async { ops::config::show_config(config_path).await });

        match result {
            Ok(config_info) => {
                let json = serde_json::to_string(&config_info)
                    .map_err(|e| RuntimeError::Generic(format!("Serialization error: {}", e)))?;
                Ok(Value::String(json))
            }
            Err(e) => Err(e),
        }
    });

    NativeCapability {
        handler,
        security_level: "low".to_string(),
        metadata: HashMap::new(),
    }
}

fn create_config_validate_capability() -> NativeCapability {
    let handler = Arc::new(|inputs: &Value| -> RuntimeResult<Value> {
        let config_path = match inputs {
            Value::String(s) => std::path::PathBuf::from(s),
            _ => get_optional_string_param(inputs, "config_path")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from("agent_config.toml")),
        };

        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create runtime: {}", e)))?;
        let result = runtime.block_on(async { ops::config::validate_config(config_path).await });

        match result {
            Ok(config_info) => {
                let json = serde_json::to_string(&config_info)
                    .map_err(|e| RuntimeError::Generic(format!("Serialization error: {}", e)))?;
                Ok(Value::String(json))
            }
            Err(e) => Err(e),
        }
    });

    NativeCapability {
        handler,
        security_level: "low".to_string(),
        metadata: HashMap::new(),
    }
}

fn create_config_init_capability() -> NativeCapability {
    let handler = Arc::new(|inputs: &Value| -> RuntimeResult<Value> {
        let output_path = get_optional_string_param(inputs, "output")
            .unwrap_or_else(|| "agent_config.toml".to_string());
        let _force = get_bool_param(inputs, "force", false);

        // For now, just return success - actual init logic is in CLI
        Ok(Value::String(format!(
            "Config initialization requested for: {}",
            output_path
        )))
    });

    NativeCapability {
        handler,
        security_level: "critical".to_string(),
        metadata: HashMap::new(),
    }
}

// Governance capabilities

fn create_governance_check_capability() -> NativeCapability {
    let handler = Arc::new(|inputs: &Value| -> RuntimeResult<Value> {
        let action = match inputs {
            Value::String(s) => s.clone(),
            _ => get_string_param(inputs, "action")?,
        };

        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create runtime: {}", e)))?;
        let result = runtime.block_on(async { ops::governance::check_action(action).await });

        match result {
            Ok(allowed) => Ok(Value::Boolean(allowed)),
            Err(e) => Err(e),
        }
    });

    NativeCapability {
        handler,
        security_level: "low".to_string(),
        metadata: HashMap::new(),
    }
}

fn create_governance_audit_capability() -> NativeCapability {
    let handler = Arc::new(|_inputs: &Value| -> RuntimeResult<Value> {
        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create runtime: {}", e)))?;
        let result = runtime.block_on(async { ops::governance::view_audit().await });

        match result {
            Ok(audit_trail) => Ok(Value::String(audit_trail)),
            Err(e) => Err(e),
        }
    });

    NativeCapability {
        handler,
        security_level: "low".to_string(),
        metadata: HashMap::new(),
    }
}

fn create_governance_constitution_capability() -> NativeCapability {
    let handler = Arc::new(|_inputs: &Value| -> RuntimeResult<Value> {
        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create runtime: {}", e)))?;
        let result = runtime.block_on(async { ops::governance::view_constitution().await });

        match result {
            Ok(constitution) => Ok(Value::String(constitution)),
            Err(e) => Err(e),
        }
    });

    NativeCapability {
        handler,
        security_level: "critical".to_string(),
        metadata: HashMap::new(),
    }
}

// Plan capabilities

fn create_plan_create_capability() -> NativeCapability {
    let handler = Arc::new(|inputs: &Value| -> RuntimeResult<Value> {
        let goal = match inputs {
            Value::String(s) => s.clone(),
            _ => get_string_param(inputs, "goal")?,
        };

        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create runtime: {}", e)))?;
        let result = runtime.block_on(async { ops::plan::create_plan(goal).await });

        match result {
            Ok(plan) => Ok(Value::String(plan)),
            Err(e) => Err(e),
        }
    });

    NativeCapability {
        handler,
        security_level: "medium".to_string(),
        metadata: HashMap::new(),
    }
}

fn create_plan_execute_capability() -> NativeCapability {
    let handler = Arc::new(|inputs: &Value| -> RuntimeResult<Value> {
        let plan = match inputs {
            Value::String(s) => s.clone(),
            _ => get_string_param(inputs, "plan")?,
        };

        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create runtime: {}", e)))?;
        let result = runtime.block_on(async { ops::plan::execute_plan(plan).await });

        match result {
            Ok(result) => Ok(Value::String(result)),
            Err(e) => Err(e),
        }
    });

    NativeCapability {
        handler,
        security_level: "high".to_string(),
        metadata: HashMap::new(),
    }
}

fn create_plan_validate_capability() -> NativeCapability {
    let handler = Arc::new(|inputs: &Value| -> RuntimeResult<Value> {
        let plan = match inputs {
            Value::String(s) => s.clone(),
            _ => get_string_param(inputs, "plan")?,
        };

        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| RuntimeError::Generic(format!("Failed to create runtime: {}", e)))?;
        let result = runtime.block_on(async { ops::plan::validate_plan(plan).await });

        match result {
            Ok(valid) => Ok(Value::Boolean(valid)),
            Err(e) => Err(e),
        }
    });

    NativeCapability {
        handler,
        security_level: "low".to_string(),
        metadata: HashMap::new(),
    }
}
