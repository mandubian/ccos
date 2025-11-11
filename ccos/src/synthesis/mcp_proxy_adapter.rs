use crate::capability_marketplace::mcp_discovery::{MCPDiscoveryProvider, MCPTool};
use crate::capability_marketplace::types::CapabilityManifest;
use rtfs::runtime::error::RuntimeResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// MCP Proxy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPProxyConfig {
    /// MCP server endpoint URL
    pub server_url: String,
    /// Server name/identifier
    pub server_name: String,
    /// Authentication token if required
    pub auth_token: Option<String>,
    /// Request timeout in seconds
    pub timeout_seconds: u64,
    /// Whether to auto-discover tools on startup
    pub auto_discover: bool,
}

/// MCP Tool proxy information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPToolProxy {
    /// Original MCP tool
    pub tool: MCPTool,
    /// Proxy capability ID
    pub capability_id: String,
    /// Whether auth is required
    pub requires_auth: bool,
    /// MCP server configuration
    pub server_config: MCPProxyConfig,
}

/// MCP Proxy Adapter for exposing MCP tools as CCOS capabilities
pub struct MCPProxyAdapter {
    /// MCP discovery provider
    discovery_provider: MCPDiscoveryProvider,
    /// Server configuration
    config: MCPProxyConfig,
    /// Mock mode for testing
    mock_mode: bool,
}

impl MCPProxyAdapter {
    /// Create a new MCP proxy adapter
    pub fn new(config: MCPProxyConfig) -> RuntimeResult<Self> {
        let mcp_config = crate::capability_marketplace::mcp_discovery::MCPServerConfig {
            name: config.server_name.clone(),
            endpoint: config.server_url.clone(),
            auth_token: config.auth_token.clone(),
            timeout_seconds: config.timeout_seconds,
            protocol_version: "2024-11-05".to_string(),
        };

        let discovery_provider = MCPDiscoveryProvider::new(mcp_config)?;

        Ok(Self {
            discovery_provider,
            config,
            mock_mode: false,
        })
    }

    /// Create in mock mode for testing
    pub fn mock(config: MCPProxyConfig) -> RuntimeResult<Self> {
        let mcp_config = crate::capability_marketplace::mcp_discovery::MCPServerConfig {
            name: config.server_name.clone(),
            endpoint: config.server_url.clone(),
            auth_token: config.auth_token.clone(),
            timeout_seconds: config.timeout_seconds,
            protocol_version: "2024-11-05".to_string(),
        };

        let discovery_provider = MCPDiscoveryProvider::new(mcp_config)?;

        Ok(Self {
            discovery_provider,
            config,
            mock_mode: true,
        })
    }

    /// Discover MCP tools and create proxies
    pub async fn discover_and_proxy_tools(&self) -> RuntimeResult<Vec<MCPToolProxy>> {
        let tools = if self.mock_mode {
            self.get_mock_tools()
        } else {
            // Convert capabilities to MCP tools (placeholder)
            vec![]
        };

        let mut proxies = Vec::new();
        for tool in tools {
            let proxy = self.create_tool_proxy(tool)?;
            proxies.push(proxy);
        }

        Ok(proxies)
    }

    /// Create a proxy for a specific MCP tool
    pub fn create_tool_proxy(&self, tool: MCPTool) -> RuntimeResult<MCPToolProxy> {
        let capability_id = format!("mcp.{}.{}", self.config.server_name, tool.name);
        let requires_auth = self.detect_auth_requirement(&tool);

        Ok(MCPToolProxy {
            tool,
            capability_id,
            requires_auth,
            server_config: self.config.clone(),
        })
    }

    /// Convert MCP tool proxy to CCOS capability
    pub fn proxy_to_capability(&self, proxy: &MCPToolProxy) -> RuntimeResult<CapabilityManifest> {
        let description = proxy
            .tool
            .description
            .clone()
            .unwrap_or_else(|| format!("MCP tool: {}", proxy.tool.name));

        // Build parameters from MCP tool input schema
        let mut parameters_map = HashMap::new();
        if let Some(input_schema) = &proxy.tool.input_schema {
            parameters_map.extend(self.extract_parameters_from_schema(input_schema));
        }

        // Add auth_token parameter if auth is required
        let mut effects = vec![":network".to_string()];
        if proxy.requires_auth {
            effects.push(":auth".to_string());
            parameters_map.insert("auth_token".to_string(), ":string".to_string());
        }

        // Build metadata
        let mut metadata = HashMap::new();
        metadata.insert("mcp_server".to_string(), self.config.server_name.clone());
        metadata.insert("mcp_server_url".to_string(), self.config.server_url.clone());
        metadata.insert("mcp_tool_name".to_string(), proxy.tool.name.clone());
        metadata.insert("proxy_type".to_string(), "mcp_proxy".to_string());

        if proxy.requires_auth {
            metadata.insert("auth_required".to_string(), "true".to_string());
            metadata.insert("auth_providers".to_string(), "mcp".to_string());
        }

        // Add MCP tool metadata if available
        if let Some(tool_metadata) = &proxy.tool.metadata {
            if let Ok(metadata_json) = serde_json::to_string(tool_metadata) {
                metadata.insert("mcp_tool_metadata".to_string(), metadata_json);
            }
        }

        // Add MCP tool annotations if available
        if let Some(annotations) = &proxy.tool.annotations {
            if let Ok(annotations_json) = serde_json::to_string(annotations) {
                metadata.insert("mcp_tool_annotations".to_string(), annotations_json);
            }
        }

        Ok(CapabilityManifest {
            id: proxy.capability_id.clone(),
            name: proxy.tool.name.clone(),
            description,
            provider: crate::capability_marketplace::types::ProviderType::MCP(
                crate::capability_marketplace::types::MCPCapability {
                    server_url: self.config.server_url.clone(),
                    tool_name: proxy.tool.name.clone(),
                    timeout_ms: self.config.timeout_seconds * 1000,
                },
            ),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: Some(crate::capability_marketplace::types::CapabilityProvenance {
                source: "mcp_proxy_adapter".to_string(),
                version: Some("1.0.0".to_string()),
                content_hash: format!("mcp_proxy_{}_{}", self.config.server_name, proxy.tool.name),
                custody_chain: vec!["mcp_proxy_adapter".to_string()],
                registered_at: chrono::Utc::now(),
            }),
            permissions: vec![],
            effects,
            metadata,
            agent_metadata: None,
        })
    }

    /// Detect if MCP tool requires authentication
    fn detect_auth_requirement(&self, tool: &MCPTool) -> bool {
        // Check tool metadata for auth requirements
        if let Some(metadata) = &tool.metadata {
            // Look for auth-related keys
            for key in ["requires_auth", "authentication", "auth", "secure"] {
                if let Some(value) = metadata.get(key) {
                    if let Some(bool_val) = value.as_bool() {
                        return bool_val;
                    }
                    if let Some(str_val) = value.as_str() {
                        if str_val.to_lowercase() == "true" || str_val.to_lowercase() == "required"
                        {
                            return true;
                        }
                    }
                }
            }
        }

        // Check tool annotations for auth requirements
        if let Some(annotations) = &tool.annotations {
            for key in ["auth", "security", "authentication"] {
                if annotations.contains_key(key) {
                    return true;
                }
            }
        }

        // Heuristic: check tool name for auth-related keywords
        let name_lower = tool.name.to_lowercase();
        let auth_keywords = [
            "user", "profile", "account", "settings", "create", "update", "delete", "admin",
            "private", "secret", "write", "modify",
        ];

        auth_keywords
            .iter()
            .any(|keyword| name_lower.contains(keyword))
    }

    /// Extract parameters from MCP tool input schema
    fn extract_parameters_from_schema(
        &self,
        schema: &serde_json::Value,
    ) -> HashMap<String, String> {
        let mut parameters = HashMap::new();

        if let Some(properties) = schema.get("properties").and_then(|p| p.as_object()) {
            for (name, prop_schema) in properties {
                let param_type = self.json_type_to_rtfs_type(prop_schema);
                parameters.insert(name.clone(), param_type);
            }
        }

        parameters
    }

    /// Convert JSON schema type to RTFS keyword type
    fn json_type_to_rtfs_type(&self, schema: &serde_json::Value) -> String {
        if let Some(type_str) = schema.get("type").and_then(|t| t.as_str()) {
            match type_str {
                "string" => ":string".to_string(),
                "number" | "integer" => ":number".to_string(),
                "boolean" => ":boolean".to_string(),
                "array" => ":list".to_string(),
                "object" => ":map".to_string(),
                _ => ":any".to_string(),
            }
        } else {
            ":any".to_string()
        }
    }

    /// Generate RTFS implementation code for MCP tool proxy
    pub fn generate_proxy_implementation(&self, proxy: &MCPToolProxy) -> RuntimeResult<String> {
        let mut code = String::new();

        // Build input parameters
        code.push_str("(let input_params {\n");
        if let Some(input_schema) = &proxy.tool.input_schema {
            if let Some(properties) = input_schema.get("properties").and_then(|p| p.as_object()) {
                for (name, _) in properties {
                    code.push_str(&format!("  :{} {}\n", name, name));
                }
            }
        }
        code.push_str("})\n");

        // Add auth if required
        if proxy.requires_auth {
            code.push_str("(let auth (call :ccos.auth.inject {:provider \"mcp\" :type :bearer :token auth_token}))\n");
        }

        // Make MCP tool call
        code.push_str("(let response (call :mcp.tool.execute {\n");
        code.push_str(&format!("  :server_url \"{}\"\n", self.config.server_url));
        code.push_str(&format!("  :tool_name \"{}\"\n", proxy.tool.name));
        code.push_str("  :input input_params");
        if proxy.requires_auth {
            code.push_str("\n  :auth_token auth");
        }
        code.push_str("\n}))\n");

        // Parse and return response
        code.push_str("(call :json.parse response)");

        Ok(code)
    }

    /// Get mock MCP tools for testing
    fn get_mock_tools(&self) -> Vec<MCPTool> {
        vec![
            MCPTool {
                name: "echo".to_string(),
                description: Some("Echo back the input".to_string()),
                input_schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "message": {"type": "string"}
                    }
                })),
                output_schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "echo": {"type": "string"}
                    }
                })),
                metadata: Some(HashMap::from([(
                    "category".to_string(),
                    serde_json::Value::String("utility".to_string()),
                )])),
                annotations: None,
            },
            MCPTool {
                name: "create_user".to_string(),
                description: Some("Create a new user account".to_string()),
                input_schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "username": {"type": "string"},
                        "email": {"type": "string"}
                    }
                })),
                output_schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "user_id": {"type": "string"},
                        "status": {"type": "string"}
                    }
                })),
                metadata: Some(HashMap::from([
                    ("requires_auth".to_string(), serde_json::Value::Bool(true)),
                    (
                        "category".to_string(),
                        serde_json::Value::String("user_management".to_string()),
                    ),
                ])),
                annotations: Some(HashMap::from([(
                    "auth".to_string(),
                    serde_json::Value::String("required".to_string()),
                )])),
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_proxy_adapter_creation() {
        let config = MCPProxyConfig {
            server_url: "http://localhost:3000".to_string(),
            server_name: "test_server".to_string(),
            auth_token: None,
            timeout_seconds: 30,
            auto_discover: true,
        };

        let adapter = MCPProxyAdapter::new(config.clone()).unwrap();
        assert_eq!(adapter.config.server_url, "http://localhost:3000");
        assert_eq!(adapter.config.server_name, "test_server");
    }

    #[test]
    fn test_detect_auth_requirement() {
        let config = MCPProxyConfig {
            server_url: "http://localhost:3000".to_string(),
            server_name: "test_server".to_string(),
            auth_token: None,
            timeout_seconds: 30,
            auto_discover: true,
        };

        let adapter = MCPProxyAdapter::mock(config).unwrap();

        // Tool with auth in metadata
        let tool_with_auth = MCPTool {
            name: "create_user".to_string(),
            description: Some("Create user".to_string()),
            input_schema: None,
            output_schema: None,
            metadata: Some(HashMap::from([(
                "requires_auth".to_string(),
                serde_json::Value::Bool(true),
            )])),
            annotations: None,
        };
        assert!(adapter.detect_auth_requirement(&tool_with_auth));

        // Tool without auth
        let tool_without_auth = MCPTool {
            name: "echo".to_string(),
            description: Some("Echo tool".to_string()),
            input_schema: None,
            output_schema: None,
            metadata: None,
            annotations: None,
        };
        assert!(!adapter.detect_auth_requirement(&tool_without_auth));

        // Tool with auth-related name
        let tool_with_auth_name = MCPTool {
            name: "get_user_profile".to_string(),
            description: Some("Get profile".to_string()),
            input_schema: None,
            output_schema: None,
            metadata: None,
            annotations: None,
        };
        assert!(adapter.detect_auth_requirement(&tool_with_auth_name));
    }

    #[test]
    fn test_json_type_to_rtfs_type() {
        let config = MCPProxyConfig {
            server_url: "http://localhost:3000".to_string(),
            server_name: "test_server".to_string(),
            auth_token: None,
            timeout_seconds: 30,
            auto_discover: true,
        };

        let adapter = MCPProxyAdapter::mock(config).unwrap();

        let string_schema = serde_json::json!({"type": "string"});
        assert_eq!(adapter.json_type_to_rtfs_type(&string_schema), ":string");

        let number_schema = serde_json::json!({"type": "number"});
        assert_eq!(adapter.json_type_to_rtfs_type(&number_schema), ":number");

        let boolean_schema = serde_json::json!({"type": "boolean"});
        assert_eq!(adapter.json_type_to_rtfs_type(&boolean_schema), ":boolean");
    }

    #[tokio::test]
    async fn test_discover_and_proxy_tools() {
        let config = MCPProxyConfig {
            server_url: "http://localhost:3000".to_string(),
            server_name: "test_server".to_string(),
            auth_token: None,
            timeout_seconds: 30,
            auto_discover: true,
        };

        let adapter = MCPProxyAdapter::mock(config).unwrap();
        let proxies = adapter.discover_and_proxy_tools().await.unwrap();

        assert!(!proxies.is_empty());
        assert!(proxies.iter().any(|p| p.tool.name == "echo"));
        assert!(proxies.iter().any(|p| p.tool.name == "create_user"));
    }

    #[test]
    fn test_proxy_to_capability() {
        let config = MCPProxyConfig {
            server_url: "http://localhost:3000".to_string(),
            server_name: "test_server".to_string(),
            auth_token: None,
            timeout_seconds: 30,
            auto_discover: true,
        };

        let adapter = MCPProxyAdapter::mock(config).unwrap();

        let tool = MCPTool {
            name: "echo".to_string(),
            description: Some("Echo tool".to_string()),
            input_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "message": {"type": "string"}
                }
            })),
            output_schema: None,
            metadata: None,
            annotations: None,
        };

        let proxy = adapter.create_tool_proxy(tool).unwrap();
        let capability = adapter.proxy_to_capability(&proxy).unwrap();

        assert!(capability.id.contains("mcp.test_server.echo"));
        assert_eq!(capability.name, "echo");
        assert!(!capability.effects.contains(&":auth".to_string()));
    }

    #[test]
    fn test_generate_proxy_implementation() {
        let config = MCPProxyConfig {
            server_url: "http://localhost:3000".to_string(),
            server_name: "test_server".to_string(),
            auth_token: None,
            timeout_seconds: 30,
            auto_discover: true,
        };

        let adapter = MCPProxyAdapter::mock(config).unwrap();

        let tool = MCPTool {
            name: "echo".to_string(),
            description: Some("Echo tool".to_string()),
            input_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "message": {"type": "string"}
                }
            })),
            output_schema: None,
            metadata: None,
            annotations: None,
        };

        let proxy = adapter.create_tool_proxy(tool).unwrap();
        let code = adapter.generate_proxy_implementation(&proxy).unwrap();

        assert!(code.contains("(call :mcp.tool.execute"));
        assert!(code.contains(":tool_name \"echo\""));
        assert!(code.contains(":message message"));
    }
}
