use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::runtime::values::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Represents a capability implementation
#[derive(Debug, Clone)]
pub struct CapabilityImpl {
    pub id: String,
    pub name: String,
    pub description: String,
    pub provider: CapabilityProvider,
    pub local: bool,
    pub endpoint: Option<String>,
}

/// Different types of capability providers
#[derive(Debug, Clone)]
pub enum CapabilityProvider {
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
}

/// Local capability implementation
#[derive(Clone)]
pub struct LocalCapability {
    pub handler: Arc<dyn Fn(&Value) -> RuntimeResult<Value> + Send + Sync>,
}

impl std::fmt::Debug for LocalCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalCapability")
            .field("handler", &"<function>")
            .finish()
    }
}

/// HTTP-based remote capability
#[derive(Debug, Clone)]
pub struct HttpCapability {
    pub base_url: String,
    pub auth_token: Option<String>,
    pub timeout_ms: u64,
}

/// MCP server capability
#[derive(Debug, Clone)]
pub struct MCPCapability {
    pub server_url: String,
    pub tool_name: String,
}

/// A2A communication capability
#[derive(Debug, Clone)]
pub struct A2ACapability {
    pub agent_id: String,
    pub endpoint: String,
}

/// Plugin-based capability
#[derive(Debug, Clone)]
pub struct PluginCapability {
    pub plugin_path: String,
    pub function_name: String,
}

/// The capability marketplace that manages all available capabilities
pub struct CapabilityMarketplace {
    capabilities: Arc<RwLock<HashMap<String, CapabilityImpl>>>,
    discovery_agents: Vec<Box<dyn CapabilityDiscovery>>,
}

impl CapabilityMarketplace {
    /// Create a new capability marketplace
    pub fn new() -> Self {
        Self {
            capabilities: Arc::new(RwLock::new(HashMap::new())),
            discovery_agents: Vec::new(),
        }
    }

    /// Register a local capability
    pub async fn register_local_capability(
        &self,
        id: String,
        name: String,
        description: String,
        handler: Arc<dyn Fn(&Value) -> RuntimeResult<Value> + Send + Sync>,
    ) -> Result<(), RuntimeError> {
        let capability = CapabilityImpl {
            id: id.clone(),
            name,
            description,
            provider: CapabilityProvider::Local(LocalCapability { handler }),
            local: true,
            endpoint: None,
        };

        let mut capabilities = self.capabilities.write().await;
        capabilities.insert(id, capability);
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
        let capability = CapabilityImpl {
            id: id.clone(),
            name,
            description,
            provider: CapabilityProvider::Http(HttpCapability {
                base_url,
                auth_token,
                timeout_ms: 5000,
            }),
            local: false,
            endpoint: None,
        };

        let mut capabilities = self.capabilities.write().await;
        capabilities.insert(id, capability);
        Ok(())
    }

    /// Get a capability by ID
    pub async fn get_capability(&self, id: &str) -> Option<CapabilityImpl> {
        let capabilities = self.capabilities.read().await;
        capabilities.get(id).cloned()
    }

    /// List all available capabilities
    pub async fn list_capabilities(&self) -> Vec<CapabilityImpl> {
        let capabilities = self.capabilities.read().await;
        capabilities.values().cloned().collect()
    }

    /// Execute a capability
    pub async fn execute_capability(&self, id: &str, inputs: &Value) -> RuntimeResult<Value> {
        let capability = self.get_capability(id).await
            .ok_or_else(|| RuntimeError::Generic(format!("Capability '{}' not found", id)))?;

        match &capability.provider {
            CapabilityProvider::Local(local) => {
                // Execute local capability synchronously
                (local.handler)(inputs)
            }
            CapabilityProvider::Http(http) => {
                // Execute HTTP capability asynchronously
                self.execute_http_capability(http, inputs).await
            }
            CapabilityProvider::MCP(mcp) => {
                // Execute MCP capability asynchronously
                self.execute_mcp_capability(mcp, inputs).await
            }
            CapabilityProvider::A2A(a2a) => {
                // Execute A2A capability asynchronously
                self.execute_a2a_capability(a2a, inputs).await
            }
            CapabilityProvider::Plugin(plugin) => {
                // Execute plugin capability
                self.execute_plugin_capability(plugin, inputs).await
            }
        }
    }

    /// Execute HTTP capability
    async fn execute_http_capability(&self, http: &HttpCapability, inputs: &Value) -> RuntimeResult<Value> {
        // Convert RTFS Value to JSON
        let json_inputs = serde_json::to_value(inputs)
            .map_err(|e| RuntimeError::Generic(format!("Failed to serialize inputs: {}", e)))?;

        // Make HTTP request
        let client = reqwest::Client::new();
        let response = client
            .post(&http.base_url)
            .header("Content-Type", "application/json")
            .json(&json_inputs)
            .timeout(std::time::Duration::from_millis(http.timeout_ms))
            .send()
            .await
            .map_err(|e| RuntimeError::Generic(format!("HTTP request failed: {}", e)))?;

        let json_response = response.json::<serde_json::Value>().await
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse response: {}", e)))?;

        // Convert JSON back to RTFS Value
        Self::json_to_rtfs_value(&json_response)
    }

    /// Execute MCP capability
    async fn execute_mcp_capability(&self, mcp: &MCPCapability, inputs: &Value) -> RuntimeResult<Value> {
        // TODO: Implement MCP client
        Err(RuntimeError::Generic("MCP capabilities not yet implemented".to_string()))
    }

    /// Execute A2A capability
    async fn execute_a2a_capability(&self, a2a: &A2ACapability, inputs: &Value) -> RuntimeResult<Value> {
        // TODO: Implement A2A client
        Err(RuntimeError::Generic("A2A capabilities not yet implemented".to_string()))
    }

    /// Execute plugin capability
    async fn execute_plugin_capability(&self, plugin: &PluginCapability, inputs: &Value) -> RuntimeResult<Value> {
        // TODO: Implement plugin execution
        Err(RuntimeError::Generic("Plugin capabilities not yet implemented".to_string()))
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
    pub async fn discover_capabilities(&self) -> Result<usize, RuntimeError> {
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
}

/// Trait for capability discovery agents
#[async_trait::async_trait]
pub trait CapabilityDiscovery: Send + Sync {
    async fn discover(&self) -> Result<Vec<CapabilityImpl>, RuntimeError>;
}

/// Default implementation with common local capabilities
impl Default for CapabilityMarketplace {
    fn default() -> Self {
        let marketplace = Self::new();
        
        // For now, return an empty marketplace to avoid async issues
        // Capabilities will be registered when needed
        marketplace
    }
}

impl Clone for CapabilityMarketplace {
    fn clone(&self) -> Self {
        Self {
            capabilities: Arc::clone(&self.capabilities),
            discovery_agents: Vec::new(), // Discovery agents are not cloned
        }
    }
} 