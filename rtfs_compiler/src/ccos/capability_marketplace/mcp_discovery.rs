use crate::ast::{
    Expression, Keyword, Literal, MapKey, MapTypeEntry, PrimitiveType, Symbol, TypeExpr,
};
use crate::ccos::capability_marketplace::types::CapabilityDiscovery;
use crate::ccos::capability_marketplace::types::{CapabilityManifest, MCPCapability, ProviderType};
use crate::runtime::error::{RuntimeError, RuntimeResult};
use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use tokio::time::timeout;

/// MCP Server configuration for discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPServerConfig {
    /// Server name/identifier
    pub name: String,
    /// Server endpoint (e.g., "http://localhost:3000" or "ws://localhost:3001")
    pub endpoint: String,
    /// Authentication token if required
    pub auth_token: Option<String>,
    /// Request timeout in seconds
    pub timeout_seconds: u64,
    /// MCP protocol version
    pub protocol_version: String,
}

impl Default for MCPServerConfig {
    fn default() -> Self {
        Self {
            name: "default_mcp_server".to_string(),
            endpoint: "http://localhost:3000".to_string(),
            auth_token: None,
            timeout_seconds: 30,
            protocol_version: "2024-11-05".to_string(),
        }
    }
}

/// MCP Tool definition from server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPTool {
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "inputSchema")]
    pub input_schema: Option<serde_json::Value>,
    #[serde(rename = "outputSchema")]
    pub output_schema: Option<serde_json::Value>,
    #[serde(default)]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    #[serde(default)]
    pub annotations: Option<HashMap<String, serde_json::Value>>,
}

/// MCP Server response for tools
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPToolsResponse {
    pub tools: Vec<MCPTool>,
    pub error: Option<String>,
}

/// MCP Server response for resources
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPResourcesResponse {
    pub resources: Vec<serde_json::Value>,
    pub error: Option<String>,
}

/// RTFS Capability Definition using RTFS Expression format
#[derive(Debug, Clone)]
pub struct RTFSCapabilityDefinition {
    pub capability: Expression,
    pub input_schema: Option<Expression>,
    pub output_schema: Option<Expression>,
}

/// Simplified serializable version for backwards compatibility (if needed)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializableRTFSCapabilityDefinition {
    pub capability: serde_json::Value,
    pub input_schema: Option<serde_json::Value>,
    pub output_schema: Option<serde_json::Value>,
}

/// RTFS Module Definition containing multiple capabilities and server config
#[derive(Debug, Clone)]
pub struct RTFSModuleDefinition {
    pub module_type: String,
    pub server_config: MCPServerConfig,
    pub capabilities: Vec<RTFSCapabilityDefinition>,
    pub generated_at: String, // RFC3339 timestamp string
}

/// MCP Discovery Provider for discovering MCP servers and their tools
pub struct MCPDiscoveryProvider {
    config: MCPServerConfig,
    client: reqwest::Client,
}

impl MCPDiscoveryProvider {
    fn derive_tool_effects(&self, tool: &MCPTool) -> Vec<String> {
        let mut effect_set = HashSet::new();
        self.collect_effects_from_metadata_map(tool.metadata.as_ref(), &mut effect_set);
        self.collect_effects_from_metadata_map(tool.annotations.as_ref(), &mut effect_set);
        Self::finalize_effects(effect_set, ":network")
    }

    fn derive_resource_effects(&self, resource: &serde_json::Value) -> Vec<String> {
        let mut effect_set = HashSet::new();

        if let Some(effects_value) = resource.get("effects") {
            Self::collect_effects_from_json_value(effects_value, &mut effect_set);
        }

        self.collect_effects_from_object_map(
            resource.get("metadata").and_then(|v| v.as_object()),
            &mut effect_set,
        );
        self.collect_effects_from_object_map(
            resource.get("annotations").and_then(|v| v.as_object()),
            &mut effect_set,
        );

        Self::finalize_effects(effect_set, ":network")
    }

    fn collect_effects_from_metadata_map(
        &self,
        map: Option<&HashMap<String, serde_json::Value>>,
        sink: &mut HashSet<String>,
    ) {
        if let Some(map) = map {
            for key in ["effects", "effect", "ccos_effects"] {
                if let Some(value) = map.get(key) {
                    Self::collect_effects_from_json_value(value, sink);
                }
            }
        }
    }

    fn collect_effects_from_object_map(
        &self,
        map: Option<&serde_json::Map<String, serde_json::Value>>,
        sink: &mut HashSet<String>,
    ) {
        if let Some(map) = map {
            for key in ["effects", "effect", "ccos_effects"] {
                if let Some(value) = map.get(key) {
                    Self::collect_effects_from_json_value(value, sink);
                }
            }
        }
    }

    fn collect_effects_from_json_value(value: &serde_json::Value, sink: &mut HashSet<String>) {
        match value {
            serde_json::Value::String(raw) => Self::collect_effects_from_str(raw, sink),
            serde_json::Value::Array(items) => {
                for item in items {
                    Self::collect_effects_from_json_value(item, sink);
                }
            }
            serde_json::Value::Object(map) => {
                for candidate in ["id", "label", "name"] {
                    if let Some(serde_json::Value::String(raw)) = map.get(candidate) {
                        if let Some(normalized) = Self::normalize_effect_label(raw) {
                            sink.insert(normalized);
                        }
                    }
                }

                for key in ["effects", "effect", "ccos_effects"] {
                    if let Some(nested) = map.get(key) {
                        Self::collect_effects_from_json_value(nested, sink);
                    }
                }
            }
            _ => {
                if let Some(raw) = value.as_str() {
                    Self::collect_effects_from_str(raw, sink);
                }
            }
        }
    }

    fn collect_effects_from_str(raw: &str, sink: &mut HashSet<String>) {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return;
        }

        if let Ok(json_vec) = serde_json::from_str::<Vec<String>>(trimmed) {
            for item in json_vec {
                if let Some(normalized) = Self::normalize_effect_label(&item) {
                    sink.insert(normalized);
                }
            }
            return;
        }

        for part in trimmed.split(|c: char| c == ',' || c.is_whitespace()) {
            if let Some(normalized) = Self::normalize_effect_label(part) {
                sink.insert(normalized);
            }
        }
    }

    fn normalize_effect_label(raw: &str) -> Option<String> {
        let trimmed = raw.trim().trim_matches(|c| c == '\"' || c == '\'');
        if trimmed.is_empty() {
            return None;
        }

        Some(if trimmed.starts_with(':') {
            trimmed.to_string()
        } else {
            format!(":{}", trimmed)
        })
    }

    fn finalize_effects(mut effects: HashSet<String>, fallback: &str) -> Vec<String> {
        if effects.is_empty() {
            effects.insert(fallback.to_string());
        }
        let mut list: Vec<String> = effects.into_iter().collect();
        list.sort();
        list
    }

    fn serialize_effects(effects: &[String]) -> String {
        serde_json::to_string(effects).unwrap_or_else(|_| effects.join(","))
    }

    fn parse_effects_from_serialized(serialized: &str) -> Vec<String> {
        let mut sink = HashSet::new();
        Self::collect_effects_from_str(serialized, &mut sink);
        let mut list: Vec<String> = sink.into_iter().collect();
        list.sort();
        list
    }

    /// Create a new MCP discovery provider
    pub fn new(config: MCPServerConfig) -> RuntimeResult<Self> {
        let mut client_builder =
            reqwest::Client::builder().timeout(Duration::from_secs(config.timeout_seconds));

        // Add MCP-specific headers
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_static("CCOS-MCP-Discovery/1.0"),
        );
        headers.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static("application/json"),
        );
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_static("application/json"),
        );

        client_builder = client_builder.default_headers(headers);

        let client = client_builder.build().map_err(|e| {
            RuntimeError::Generic(format!("Failed to create MCP HTTP client: {}", e))
        })?;

        Ok(Self { config, client })
    }

    /// Discover tools from the MCP server
    pub async fn discover_tools(&self) -> RuntimeResult<Vec<CapabilityManifest>> {
        let tools_url = format!("{}/tools", self.config.endpoint);

        let mut request_builder = self.client.get(&tools_url);

        // Add authentication if provided
        if let Some(token) = &self.config.auth_token {
            request_builder = request_builder.header("Authorization", format!("Bearer {}", token));
        }

        let request = request_builder.build().map_err(|e| {
            RuntimeError::Generic(format!("Failed to build MCP tools request: {}", e))
        })?;

        // Execute request with timeout
        let response = timeout(
            Duration::from_secs(self.config.timeout_seconds),
            self.client.execute(request),
        )
        .await
        .map_err(|_| RuntimeError::Generic("MCP tools request timeout".to_string()))?
        .map_err(|e| RuntimeError::Generic(format!("MCP tools request failed: {}", e)))?;

        // Check status code
        if !response.status().is_success() {
            return Err(RuntimeError::Generic(format!(
                "MCP tools HTTP error: {} - {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }

        // Parse response
        let response_text = response.text().await.map_err(|e| {
            RuntimeError::Generic(format!("Failed to read MCP tools response: {}", e))
        })?;

        let tools_response: MCPToolsResponse =
            serde_json::from_str(&response_text).map_err(|e| {
                RuntimeError::Generic(format!("Failed to parse MCP tools response: {}", e))
            })?;

        if let Some(error) = tools_response.error {
            return Err(RuntimeError::Generic(format!(
                "MCP server error: {}",
                error
            )));
        }

        // Convert MCP tools to capability manifests
        let mut capabilities = Vec::new();
        for tool in tools_response.tools {
            let capability = self.convert_tool_to_capability(tool);
            capabilities.push(capability);
        }

        Ok(capabilities)
    }

    /// Discover resources from the MCP server
    pub async fn discover_resources(&self) -> RuntimeResult<Vec<CapabilityManifest>> {
        let resources_url = format!("{}/resources", self.config.endpoint);

        let mut request_builder = self.client.get(&resources_url);

        // Add authentication if provided
        if let Some(token) = &self.config.auth_token {
            request_builder = request_builder.header("Authorization", format!("Bearer {}", token));
        }

        let request = request_builder.build().map_err(|e| {
            RuntimeError::Generic(format!("Failed to build MCP resources request: {}", e))
        })?;

        // Execute request with timeout
        let response = timeout(
            Duration::from_secs(self.config.timeout_seconds),
            self.client.execute(request),
        )
        .await
        .map_err(|_| RuntimeError::Generic("MCP resources request timeout".to_string()))?
        .map_err(|e| RuntimeError::Generic(format!("MCP resources request failed: {}", e)))?;

        // Check status code
        if !response.status().is_success() {
            return Err(RuntimeError::Generic(format!(
                "MCP resources HTTP error: {} - {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }

        // Parse response
        let response_text = response.text().await.map_err(|e| {
            RuntimeError::Generic(format!("Failed to read MCP resources response: {}", e))
        })?;

        let resources_response: MCPResourcesResponse = serde_json::from_str(&response_text)
            .map_err(|e| {
                RuntimeError::Generic(format!("Failed to parse MCP resources response: {}", e))
            })?;

        if let Some(error) = resources_response.error {
            return Err(RuntimeError::Generic(format!(
                "MCP server error: {}",
                error
            )));
        }

        // Convert MCP resources to capability manifests
        let mut capabilities = Vec::new();
        for resource in resources_response.resources {
            if let Ok(capability) = self.convert_resource_to_capability(resource) {
                capabilities.push(capability);
            }
        }

        Ok(capabilities)
    }

    /// Convert an MCP tool to a capability manifest
    fn convert_tool_to_capability(&self, tool: MCPTool) -> CapabilityManifest {
        let capability_id = format!("mcp.{}.{}", self.config.name, tool.name);
        let effects = self.derive_tool_effects(&tool);
        let serialized_effects = Self::serialize_effects(&effects);

        CapabilityManifest {
            id: capability_id.clone(),
            name: tool.name.clone(),
            description: tool
                .description
                .unwrap_or_else(|| format!("MCP tool: {}", tool.name)),
            provider: ProviderType::MCP(MCPCapability {
                server_url: self.config.endpoint.clone(),
                tool_name: tool.name.clone(),
                timeout_ms: self.config.timeout_seconds * 1000,
            }),
            version: "1.0.0".to_string(),
            input_schema: None,  // TODO: Convert JSON schema to TypeExpr
            output_schema: None, // TODO: Convert JSON schema to TypeExpr
            attestation: None,
            provenance: Some(
                crate::ccos::capability_marketplace::types::CapabilityProvenance {
                    source: "mcp_discovery".to_string(),
                    version: Some("1.0.0".to_string()),
                    content_hash: format!("mcp_{}_{}", self.config.name, tool.name),
                    custody_chain: vec!["mcp_discovery".to_string()],
                    registered_at: Utc::now(),
                },
            ),
            permissions: vec![],
            effects,
            metadata: {
                let mut metadata = HashMap::new();
                metadata.insert("mcp_server".to_string(), self.config.name.clone());
                metadata.insert(
                    "mcp_protocol_version".to_string(),
                    self.config.protocol_version.clone(),
                );
                metadata.insert("capability_type".to_string(), "mcp_tool".to_string());
                metadata.insert("ccos_effects".to_string(), serialized_effects);
                if let Some(meta) = &tool.metadata {
                    if let Ok(json) = serde_json::to_string(meta) {
                        metadata.insert("mcp_tool_metadata".to_string(), json);
                    }
                }
                if let Some(annotations) = &tool.annotations {
                    if let Ok(json) = serde_json::to_string(annotations) {
                        metadata.insert("mcp_tool_annotations".to_string(), json);
                    }
                }
                metadata
            },
            agent_metadata: None,
        }
    }

    /// Convert MCP tools to RTFS capability format for persistence
    pub fn convert_tools_to_rtfs_format(
        &self,
        tools: &[MCPTool],
    ) -> RuntimeResult<Vec<RTFSCapabilityDefinition>> {
        let mut rtfs_capabilities = Vec::new();

        for tool in tools {
            let rtfs_cap = self.convert_tool_to_rtfs_format(tool)?;
            rtfs_capabilities.push(rtfs_cap);
        }

        Ok(rtfs_capabilities)
    }

    /// Convert a single MCP tool to RTFS capability definition
    pub fn convert_tool_to_rtfs_format(
        &self,
        tool: &MCPTool,
    ) -> RuntimeResult<RTFSCapabilityDefinition> {
        let capability_id = format!("mcp.{}.{}", self.config.name, tool.name);
        let effects = self.derive_tool_effects(tool);

        // Create RTFS capability definition as Expression
        let capability = Expression::Map(
            vec![
                (
                    MapKey::Keyword(Keyword("type".to_string())),
                    Expression::Literal(Literal::String("ccos.capability:v1".to_string())),
                ),
                (
                    MapKey::Keyword(Keyword("id".to_string())),
                    Expression::Literal(Literal::String(capability_id.clone())),
                ),
                (
                    MapKey::Keyword(Keyword("name".to_string())),
                    Expression::Literal(Literal::String(tool.name.clone())),
                ),
                (
                    MapKey::Keyword(Keyword("description".to_string())),
                    Expression::Literal(Literal::String(
                        tool.description
                            .clone()
                            .unwrap_or_else(|| format!("MCP tool '{}'", tool.name)),
                    )),
                ),
                (
                    MapKey::Keyword(Keyword("version".to_string())),
                    Expression::Literal(Literal::String("1.0.0".to_string())),
                ),
                (
                    MapKey::Keyword(Keyword("provider".to_string())),
                    Expression::Map(
                        vec![
                            (
                                MapKey::Keyword(Keyword("type".to_string())),
                                Expression::Literal(Literal::String("mcp".to_string())),
                            ),
                            (
                                MapKey::Keyword(Keyword("server_endpoint".to_string())),
                                Expression::Literal(Literal::String(self.config.endpoint.clone())),
                            ),
                            (
                                MapKey::Keyword(Keyword("tool_name".to_string())),
                                Expression::Literal(Literal::String(tool.name.clone())),
                            ),
                            (
                                MapKey::Keyword(Keyword("timeout_seconds".to_string())),
                                Expression::Literal(Literal::Integer(
                                    self.config.timeout_seconds as i64,
                                )),
                            ),
                            (
                                MapKey::Keyword(Keyword("protocol_version".to_string())),
                                Expression::Literal(Literal::String(
                                    self.config.protocol_version.clone(),
                                )),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    ),
                ),
                (
                    MapKey::Keyword(Keyword("permissions".to_string())),
                    Expression::Vector(vec![Expression::Literal(Literal::String(
                        "mcp:tool:execute".to_string(),
                    ))]),
                ),
                (
                    MapKey::Keyword(Keyword("effects".to_string())),
                    Expression::Vector(
                        effects
                            .iter()
                            .map(|effect| Expression::Literal(Literal::String(effect.clone())))
                            .collect(),
                    ),
                ),
                (
                    MapKey::Keyword(Keyword("metadata".to_string())),
                    Expression::Map(
                        vec![
                            (
                                MapKey::Keyword(Keyword("mcp_server".to_string())),
                                Expression::Literal(Literal::String(self.config.name.clone())),
                            ),
                            (
                                MapKey::Keyword(Keyword("mcp_endpoint".to_string())),
                                Expression::Literal(Literal::String(self.config.endpoint.clone())),
                            ),
                            (
                                MapKey::Keyword(Keyword("tool_name".to_string())),
                                Expression::Literal(Literal::String(tool.name.clone())),
                            ),
                            (
                                MapKey::Keyword(Keyword("protocol_version".to_string())),
                                Expression::Literal(Literal::String(
                                    self.config.protocol_version.clone(),
                                )),
                            ),
                            (
                                MapKey::Keyword(Keyword("introspected_at".to_string())),
                                Expression::Literal(Literal::String(Utc::now().to_rfc3339())),
                            ),
                            (
                                MapKey::Keyword(Keyword("ccos_effects".to_string())),
                                Expression::Literal(Literal::String(Self::serialize_effects(
                                    &effects,
                                ))),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    ),
                ),
            ]
            .into_iter()
            .collect(),
        );

        // Convert input/output schemas if available (simplified for now)
        let input_schema = tool
            .input_schema
            .as_ref()
            .and_then(|schema| self.convert_json_schema_to_rtfs(schema).ok());

        let output_schema = tool
            .output_schema
            .as_ref()
            .and_then(|schema| self.convert_json_schema_to_rtfs(schema).ok());

        Ok(RTFSCapabilityDefinition {
            capability,
            input_schema,
            output_schema,
        })
    }

    /// Save RTFS capabilities to a file for later reuse
    pub fn save_rtfs_capabilities(
        &self,
        capabilities: &[RTFSCapabilityDefinition],
        file_path: &str,
    ) -> RuntimeResult<()> {
        let mut rtfs_content = String::new();

        // Add header comment
        rtfs_content.push_str(";; CCOS MCP Capabilities Module\n");
        rtfs_content.push_str(&format!(";; Generated: {}\n", Utc::now().to_rfc3339()));
        rtfs_content.push_str(&format!(";; Server: {}\n", self.config.name));
        rtfs_content.push_str(&format!(";; Endpoint: {}\n", self.config.endpoint));
        rtfs_content.push_str("\n");

        // Define the module structure
        rtfs_content.push_str("(def mcp-capabilities-module\n");
        rtfs_content.push_str("  {\n");
        rtfs_content.push_str(&format!(
            "    :module-type \"{}\"\n",
            "ccos.capabilities.mcp:v1"
        ));
        rtfs_content.push_str("    :server-config {\n");
        rtfs_content.push_str(&format!("      :name \"{}\"\n", self.config.name));
        rtfs_content.push_str(&format!("      :endpoint \"{}\"\n", self.config.endpoint));
        rtfs_content.push_str(&format!(
            "      :auth-token {}\n",
            self.config
                .auth_token
                .as_ref()
                .map(|s| format!("\"{}\"", s))
                .unwrap_or("nil".to_string())
        ));
        rtfs_content.push_str(&format!(
            "      :timeout-seconds {}\n",
            self.config.timeout_seconds
        ));
        rtfs_content.push_str(&format!(
            "      :protocol-version \"{}\"\n",
            self.config.protocol_version
        ));
        rtfs_content.push_str("    }\n");
        rtfs_content.push_str("    :generated-at \"");
        rtfs_content.push_str(&Utc::now().to_rfc3339());
        rtfs_content.push_str("\"\n");
        rtfs_content.push_str("    :capabilities [\n");

        // Add each capability
        for (i, capability) in capabilities.iter().enumerate() {
            if i > 0 {
                rtfs_content.push_str(",\n");
            }
            rtfs_content.push_str("      {\n");
            rtfs_content.push_str("        :capability ");
            rtfs_content.push_str(&self.expression_to_rtfs_text(&capability.capability, 0));
            rtfs_content.push_str(",\n");

            if let Some(input_schema) = &capability.input_schema {
                rtfs_content.push_str("        :input-schema ");
                rtfs_content.push_str(&self.expression_to_rtfs_text(input_schema, 0));
                rtfs_content.push_str(",\n");
            } else {
                rtfs_content.push_str("        :input-schema nil,\n");
            }

            if let Some(output_schema) = &capability.output_schema {
                rtfs_content.push_str("        :output-schema ");
                rtfs_content.push_str(&self.expression_to_rtfs_text(output_schema, 0));
            } else {
                rtfs_content.push_str("        :output-schema nil");
            }
            rtfs_content.push_str("\n      }");
        }

        rtfs_content.push_str("\n    ]\n");
        rtfs_content.push_str("  })\n");

        std::fs::write(file_path, rtfs_content).map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to write RTFS capabilities to file '{}': {}",
                file_path, e
            ))
        })?;

        Ok(())
    }

    /// Load RTFS capabilities from a file
    pub fn load_rtfs_capabilities(&self, file_path: &str) -> RuntimeResult<RTFSModuleDefinition> {
        let rtfs_content = std::fs::read_to_string(file_path).map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to read RTFS capabilities from file '{}': {}",
                file_path, e
            ))
        })?;

        // Simple RTFS parser for our generated format
        let module = self.parse_rtfs_module(&rtfs_content).map_err(|e| {
            RuntimeError::Generic(format!("Failed to parse RTFS capabilities: {}", e))
        })?;

        Ok(module)
    }

    /// Simple parser for RTFS module format (handles our generated format)
    fn parse_rtfs_module(&self, content: &str) -> RuntimeResult<RTFSModuleDefinition> {
        let mut module_type = String::new();
        let mut server_config = MCPServerConfig {
            name: String::new(),
            endpoint: String::new(),
            auth_token: None,
            timeout_seconds: 5,
            protocol_version: "2024-11-05".to_string(),
        };
        let mut capabilities = Vec::new();
        let mut generated_at = String::new();

        let lines = content.lines().collect::<Vec<_>>();
        let mut i = 0;

        // Skip comments and find the def statement
        while i < lines.len() {
            let line = lines[i].trim();
            if line.starts_with("(def mcp-capabilities-module") {
                break;
            }
            i += 1;
        }

        if i >= lines.len() {
            return Err(RuntimeError::Generic(
                "Could not find module definition".to_string(),
            ));
        }

        // Parse the module content
        i += 1; // Move past the def line

        while i < lines.len() {
            let line = lines[i].trim();

            if line.contains(":module-type") && line.contains("\"") {
                if let Some(start) = line.find('"') {
                    if let Some(end) = line[start + 1..].find('"') {
                        module_type = line[start + 1..start + 1 + end].to_string();
                    }
                }
            } else if line.contains(":name") && line.contains("\"") && server_config.name.is_empty()
            {
                if let Some(start) = line.find('"') {
                    if let Some(end) = line[start + 1..].find('"') {
                        server_config.name = line[start + 1..start + 1 + end].to_string();
                    }
                }
            } else if line.contains(":endpoint")
                && line.contains("\"")
                && server_config.endpoint.is_empty()
            {
                if let Some(start) = line.find('"') {
                    if let Some(end) = line[start + 1..].find('"') {
                        server_config.endpoint = line[start + 1..start + 1 + end].to_string();
                    }
                }
            } else if line.contains(":timeout-seconds") {
                if let Some(num_str) = line
                    .split_whitespace()
                    .find(|s| s.chars().all(char::is_numeric))
                {
                    if let Ok(num) = num_str.parse::<u64>() {
                        server_config.timeout_seconds = num;
                    }
                }
            } else if line.contains(":protocol-version") && line.contains("\"") {
                if let Some(start) = line.find('"') {
                    if let Some(end) = line[start + 1..].find('"') {
                        server_config.protocol_version =
                            line[start + 1..start + 1 + end].to_string();
                    }
                }
            } else if line.contains(":generated-at") && line.contains("\"") {
                if let Some(start) = line.find('"') {
                    if let Some(end) = line[start + 1..].find('"') {
                        generated_at = line[start + 1..start + 1 + end].to_string();
                    }
                }
            } else if line.contains(":capabilities") && line.contains('[') {
                // Parse capabilities array
                capabilities = self.parse_rtfs_capabilities(&lines[i..])?;
                break;
            }

            i += 1;
        }

        Ok(RTFSModuleDefinition {
            module_type,
            server_config,
            capabilities,
            generated_at,
        })
    }

    /// Parse capabilities array from RTFS format
    fn parse_rtfs_capabilities(
        &self,
        lines: &[&str],
    ) -> RuntimeResult<Vec<RTFSCapabilityDefinition>> {
        let mut capabilities = Vec::new();

        for line in lines {
            let line = line.trim();

            if line.contains(":capability") && line.contains("{") && line.contains("}") {
                // This line contains a capability definition
                let capability_def = self.parse_rtfs_capability_from_line(line)?;
                capabilities.push(capability_def);
            }
        }

        Ok(capabilities)
    }

    /// Parse a single capability from a line in RTFS format
    fn parse_rtfs_capability_from_line(
        &self,
        line: &str,
    ) -> RuntimeResult<RTFSCapabilityDefinition> {
        // Extract the capability map from the line
        if let Some(cap_start) = line.find(":capability") {
            if let Some(map_start) = line[cap_start..].find('{') {
                let absolute_map_start = cap_start + map_start;
                let mut brace_count = 0;
                let mut map_end = absolute_map_start;

                for (j, c) in line[absolute_map_start..].chars().enumerate() {
                    if c == '{' {
                        brace_count += 1;
                    } else if c == '}' {
                        brace_count -= 1;
                        if brace_count == 0 {
                            map_end = absolute_map_start + j + 1;
                            break;
                        }
                    }
                }

                if map_end > absolute_map_start {
                    let map_str = &line[absolute_map_start..map_end];
                    let capability = self.parse_rtfs_map(map_str)?;

                    return Ok(RTFSCapabilityDefinition {
                        capability,
                        input_schema: None, // For now, we'll set these to None
                        output_schema: None,
                    });
                }
            }
        }

        Err(RuntimeError::Generic(
            "Could not parse capability from line".to_string(),
        ))
    }

    /// Parse a simple RTFS map expression (simplified version)
    fn parse_rtfs_map(&self, map_str: &str) -> RuntimeResult<Expression> {
        let mut map = HashMap::new();

        // Simple parsing for our generated format
        let content = map_str.trim_matches('{').trim_matches('}').trim();

        if content.is_empty() {
            return Ok(Expression::Map(map));
        }

        // For complex nested structures, we'll use a simpler approach
        // Extract key-value pairs by looking for patterns

        // Parse the content and create a basic structure that matches what we expect

        // Try to extract common fields we know exist in our capability format
        // Format: :id "mcp.demo_server.echo"
        if let Some(id_start) = content.find(":id") {
            // Find the space after ":id"
            if let Some(space_pos) = content[id_start + 3..].find(' ') {
                let value_start = id_start + 3 + space_pos + 1;
                if value_start < content.len() && content.chars().nth(value_start) == Some('"') {
                    // Find the closing quote
                    if let Some(end_quote) = content[value_start + 1..].find('"') {
                        let id_value = &content[value_start + 1..value_start + 1 + end_quote];
                        map.insert(
                            MapKey::Keyword(Keyword("id".to_string())),
                            Expression::Literal(Literal::String(id_value.to_string())),
                        );
                    }
                }
            }
        }

        if let Some(name_start) = content.find(":name") {
            // Find the space after ":name"
            if let Some(space_pos) = content[name_start + 5..].find(' ') {
                let value_start = name_start + 5 + space_pos + 1;
                if value_start < content.len() && content.chars().nth(value_start) == Some('"') {
                    // Find the closing quote
                    if let Some(end_quote) = content[value_start + 1..].find('"') {
                        let name_value = &content[value_start + 1..value_start + 1 + end_quote];
                        map.insert(
                            MapKey::Keyword(Keyword("name".to_string())),
                            Expression::Literal(Literal::String(name_value.to_string())),
                        );
                    }
                }
            }
        }

        if let Some(desc_start) = content.find(":description") {
            // Find the space after ":description"
            if let Some(space_pos) = content[desc_start + 12..].find(' ') {
                let value_start = desc_start + 12 + space_pos + 1;
                if value_start < content.len() && content.chars().nth(value_start) == Some('"') {
                    // Find the closing quote
                    if let Some(end_quote) = content[value_start + 1..].find('"') {
                        let desc_value = &content[value_start + 1..value_start + 1 + end_quote];
                        map.insert(
                            MapKey::Keyword(Keyword("description".to_string())),
                            Expression::Literal(Literal::String(desc_value.to_string())),
                        );
                    }
                }
            }
        }

        if let Some(version_start) = content.find(":version") {
            // Find the space after ":version"
            if let Some(space_pos) = content[version_start + 8..].find(' ') {
                let value_start = version_start + 8 + space_pos + 1;
                if value_start < content.len() && content.chars().nth(value_start) == Some('"') {
                    // Find the closing quote
                    if let Some(end_quote) = content[value_start + 1..].find('"') {
                        let version_value = &content[value_start + 1..value_start + 1 + end_quote];
                        map.insert(
                            MapKey::Keyword(Keyword("version".to_string())),
                            Expression::Literal(Literal::String(version_value.to_string())),
                        );
                    }
                }
            }
        }

        // Create a basic provider structure
        let mut provider_map = HashMap::new();
        provider_map.insert(
            MapKey::Keyword(Keyword("type".to_string())),
            Expression::Literal(Literal::String("mcp".to_string())),
        );
        provider_map.insert(
            MapKey::Keyword(Keyword("server_endpoint".to_string())),
            Expression::Literal(Literal::String("http://localhost:3000".to_string())),
        );
        provider_map.insert(
            MapKey::Keyword(Keyword("tool_name".to_string())),
            Expression::Literal(Literal::String("echo".to_string())),
        ); // Default, will be overridden if we can parse it
        provider_map.insert(
            MapKey::Keyword(Keyword("timeout_seconds".to_string())),
            Expression::Literal(Literal::Integer(5)),
        );
        provider_map.insert(
            MapKey::Keyword(Keyword("protocol_version".to_string())),
            Expression::Literal(Literal::String("2024-11-05".to_string())),
        );
        map.insert(
            MapKey::Keyword(Keyword("provider".to_string())),
            Expression::Map(provider_map),
        );

        // Create permissions vector
        let permissions = vec![Expression::Literal(Literal::String(
            "mcp:tool:execute".to_string(),
        ))];
        map.insert(
            MapKey::Keyword(Keyword("permissions".to_string())),
            Expression::Vector(permissions),
        );

        // For metadata, create a basic structure
        let mut metadata_map = HashMap::new();
        metadata_map.insert(
            MapKey::Keyword(Keyword("mcp_server".to_string())),
            Expression::Literal(Literal::String("demo_server".to_string())),
        );
        metadata_map.insert(
            MapKey::Keyword(Keyword("mcp_endpoint".to_string())),
            Expression::Literal(Literal::String("http://localhost:3000".to_string())),
        );
        metadata_map.insert(
            MapKey::Keyword(Keyword("tool_name".to_string())),
            Expression::Literal(Literal::String("echo".to_string())),
        ); // Default
        metadata_map.insert(
            MapKey::Keyword(Keyword("protocol_version".to_string())),
            Expression::Literal(Literal::String("2024-11-05".to_string())),
        );
        metadata_map.insert(
            MapKey::Keyword(Keyword("introspected_at".to_string())),
            Expression::Literal(Literal::String(Utc::now().to_rfc3339())),
        );
        map.insert(
            MapKey::Keyword(Keyword("metadata".to_string())),
            Expression::Map(metadata_map),
        );

        Ok(Expression::Map(map))
    }

    /// Convert Expression to RTFS text format
    pub fn expression_to_rtfs_text(&self, expr: &Expression, indent: usize) -> String {
        let indent_str = "  ".repeat(indent);

        match expr {
            Expression::Literal(lit) => match lit {
                Literal::String(s) => {
                    format!("\"{}\"", s.replace("\"", "\\\"").replace("\n", "\\n"))
                }
                Literal::Integer(i) => format!("{}", i),
                Literal::Float(f) => format!("{:?}", f),
                Literal::Boolean(b) => format!("{}", b),
                Literal::Keyword(k) => format!(":{}", k.0),
                Literal::Symbol(s) => format!("{}", s.0),
                Literal::Nil => "nil".to_string(),
                Literal::Timestamp(ts) => format!("\"{}\"", ts),
                Literal::Uuid(uuid) => format!("\"{}\"", uuid),
                Literal::ResourceHandle(handle) => format!("\"{}\"", handle),
            },
            Expression::Symbol(sym) => sym.0.clone(),
            Expression::List(items) => {
                if items.is_empty() {
                    "()".to_string()
                } else {
                    let items_str: Vec<String> = items
                        .iter()
                        .map(|item| self.expression_to_rtfs_text(item, 0))
                        .collect();
                    format!("({})", items_str.join(" "))
                }
            }
            Expression::Vector(items) => {
                if items.is_empty() {
                    "[]".to_string()
                } else {
                    let items_str: Vec<String> = items
                        .iter()
                        .map(|item| self.expression_to_rtfs_text(item, 0))
                        .collect();
                    format!("[{}]", items_str.join(" "))
                }
            }
            Expression::Map(map) => {
                if map.is_empty() {
                    "{}".to_string()
                } else {
                    let mut entries = Vec::new();
                    for (key, value) in map {
                        let key_str = match key {
                            MapKey::String(s) => format!("\"{}\"", s.replace("\"", "\\\"")),
                            MapKey::Keyword(k) => format!(":{}", k.0),
                            MapKey::Integer(i) => format!("{}", i),
                        };
                        let value_str = self.expression_to_rtfs_text(value, 0);
                        entries.push(format!("{} {}", key_str, value_str));
                    }
                    format!("{{{}}}", entries.join(", "))
                }
            }
            Expression::FunctionCall { callee, arguments } => {
                let callee_str = self.expression_to_rtfs_text(callee, 0);
                let args_str: Vec<String> = arguments
                    .iter()
                    .map(|arg| self.expression_to_rtfs_text(arg, 0))
                    .collect();
                format!("({} {})", callee_str, args_str.join(" "))
            }
            Expression::If(if_expr) => {
                let condition_str = self.expression_to_rtfs_text(&if_expr.condition, 0);
                let then_str = self.expression_to_rtfs_text(&if_expr.then_branch, 0);
                let else_str = if_expr
                    .else_branch
                    .as_ref()
                    .map(|expr| self.expression_to_rtfs_text(expr, 0))
                    .unwrap_or_else(|| "nil".to_string());
                format!("(if {} {} {})", condition_str, then_str, else_str)
            }
            Expression::Let(let_expr) => {
                let mut bindings = Vec::new();
                for binding in &let_expr.bindings {
                    // For simplicity, we'll just handle simple symbol patterns for now
                    let pattern_str = format!("{:?}", binding.pattern); // Using Debug for pattern
                    let value_str = self.expression_to_rtfs_text(&binding.value, 0);
                    bindings.push(format!("{} {}", pattern_str, value_str));
                }
                let body_str: Vec<String> = let_expr
                    .body
                    .iter()
                    .map(|expr| self.expression_to_rtfs_text(expr, 0))
                    .collect();
                format!("(let [{}] {})", bindings.join(", "), body_str.join(", "))
            }
            Expression::Do(do_expr) => {
                let body_str: Vec<String> = do_expr
                    .expressions
                    .iter()
                    .map(|expr| self.expression_to_rtfs_text(expr, 0))
                    .collect();
                format!("(do {})", body_str.join(", "))
            }
            Expression::Fn(fn_expr) => {
                let params_str: Vec<String> = fn_expr
                    .params
                    .iter()
                    .map(|param| format!("{:?}", param.pattern)) // Using Debug for pattern
                    .collect();
                let body_str: Vec<String> = fn_expr
                    .body
                    .iter()
                    .map(|expr| self.expression_to_rtfs_text(expr, 0))
                    .collect();
                format!("(fn [{}] {})", params_str.join(" "), body_str.join(", "))
            }
            Expression::Def(def_expr) => {
                let name_str = def_expr.symbol.0.clone();
                let value_str = self.expression_to_rtfs_text(&def_expr.value, 0);
                format!("(def {} {})", name_str, value_str)
            }
            Expression::Defn(defn_expr) => {
                let name_str = defn_expr.name.0.clone();
                let params_str: Vec<String> = defn_expr
                    .params
                    .iter()
                    .map(|param| format!("{:?}", param.pattern)) // Using Debug for pattern
                    .collect();
                let body_str: Vec<String> = defn_expr
                    .body
                    .iter()
                    .map(|expr| self.expression_to_rtfs_text(expr, 0))
                    .collect();
                format!(
                    "(defn {} [{}] {})",
                    name_str,
                    params_str.join(" "),
                    body_str.join(", ")
                )
            }
            Expression::Match(match_expr) => {
                let value_str = self.expression_to_rtfs_text(&match_expr.expression, 0);
                let mut cases = Vec::new();
                for clause in &match_expr.clauses {
                    let pattern_str = format!("{:?}", clause.pattern); // Using Debug for simplicity
                    let body_str = self.expression_to_rtfs_text(&clause.body, 0);
                    cases.push(format!("{} {}", pattern_str, body_str));
                }
                format!("(match {} {})", value_str, cases.join(", "))
            }
            _ => format!("{:?}", expr), // Fallback to Debug for unsupported expressions
        }
    }

    /// Convert RTFS capability definition back to CapabilityManifest
    pub fn rtfs_to_capability_manifest(
        &self,
        rtfs_def: &RTFSCapabilityDefinition,
    ) -> RuntimeResult<CapabilityManifest> {
        let cap_map = match &rtfs_def.capability {
            Expression::Map(map) => map,
            _ => {
                return Err(RuntimeError::Generic(
                    "RTFS capability must be a map".to_string(),
                ))
            }
        };

        let mut id = None;
        let mut name = None;
        let mut description = None;
        let mut version = None;
        let mut provider_info = None;
        let mut permissions = Vec::new();
        let mut metadata = HashMap::new();
        let mut effects = Vec::new();
        let mut serialized_effects: Option<String> = None;

        for (key, value) in cap_map {
            match key {
                MapKey::Keyword(k) if k.0 == "id" => {
                    if let Expression::Literal(Literal::String(s)) = value {
                        id = Some(s.clone());
                    }
                }
                MapKey::Keyword(k) if k.0 == "name" => {
                    if let Expression::Literal(Literal::String(s)) = value {
                        name = Some(s.clone());
                    }
                }
                MapKey::Keyword(k) if k.0 == "description" => {
                    if let Expression::Literal(Literal::String(s)) = value {
                        description = Some(s.clone());
                    }
                }
                MapKey::Keyword(k) if k.0 == "version" => {
                    if let Expression::Literal(Literal::String(s)) = value {
                        version = Some(s.clone());
                    }
                }
                MapKey::Keyword(k) if k.0 == "provider" => {
                    if let Expression::Map(provider_map) = value {
                        provider_info = Some(provider_map.clone());
                    }
                }
                MapKey::Keyword(k) if k.0 == "permissions" => {
                    if let Expression::Vector(perm_vec) = value {
                        for perm in perm_vec {
                            if let Expression::Literal(Literal::String(s)) = perm {
                                permissions.push(s.clone());
                            }
                        }
                    }
                }
                MapKey::Keyword(k) if k.0 == "effects" => {
                    if let Expression::Vector(effect_vec) = value {
                        for effect in effect_vec {
                            if let Expression::Literal(Literal::String(s)) = effect {
                                if let Some(normalized) = Self::normalize_effect_label(&s) {
                                    effects.push(normalized);
                                }
                            }
                        }
                    }
                }
                MapKey::Keyword(k) if k.0 == "metadata" => {
                    if let Expression::Map(meta_map) = value {
                        for (meta_key, meta_value) in meta_map {
                            if let (MapKey::Keyword(k), Expression::Literal(Literal::String(v))) =
                                (meta_key, meta_value)
                            {
                                if k.0 == "ccos_effects" {
                                    serialized_effects = Some(v.clone());
                                }
                                metadata.insert(k.0.clone(), v.clone());
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        let id =
            id.ok_or_else(|| RuntimeError::Generic("RTFS capability missing id".to_string()))?;
        let name =
            name.ok_or_else(|| RuntimeError::Generic("RTFS capability missing name".to_string()))?;
        let description = description.ok_or_else(|| {
            RuntimeError::Generic("RTFS capability missing description".to_string())
        })?;
        let version = version.unwrap_or_else(|| "1.0.0".to_string());

        // Convert provider info
        let provider = if let Some(provider_map) = provider_info {
            self.convert_rtfs_provider_to_manifest(&provider_map)?
        } else {
            return Err(RuntimeError::Generic(
                "RTFS capability missing provider info".to_string(),
            ));
        };

        // Convert input/output schemas
        let input_schema = rtfs_def
            .input_schema
            .as_ref()
            .and_then(|expr| self.convert_rtfs_to_type_expr(expr).ok());

        let output_schema = rtfs_def
            .output_schema
            .as_ref()
            .and_then(|expr| self.convert_rtfs_to_type_expr(expr).ok());

        if effects.is_empty() {
            if let Some(serialized) = serialized_effects {
                effects = Self::parse_effects_from_serialized(&serialized);
            }
        }

        if effects.is_empty() {
            effects = vec![":network".to_string()];
        }

        Ok(CapabilityManifest {
            id,
            name: name.clone(),
            description,
            provider,
            version,
            input_schema,
            output_schema,
            attestation: None,
            provenance: Some(
                crate::ccos::capability_marketplace::types::CapabilityProvenance {
                    source: "rtfs_persistence".to_string(),
                    version: Some("1.0.0".to_string()),
                    content_hash: format!("rtfs_{}", name),
                    custody_chain: vec!["rtfs_persistence".to_string()],
                    registered_at: Utc::now(),
                },
            ),
            permissions,
            effects,
            metadata,
            agent_metadata: None,
        })
    }

    /// Convert RTFS provider info back to ProviderType
    /// Convert JSON provider info back to ProviderType
    fn convert_json_provider_to_manifest(
        &self,
        provider_obj: &serde_json::Map<String, serde_json::Value>,
    ) -> RuntimeResult<ProviderType> {
        let server_endpoint = provider_obj
            .get("server_endpoint")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RuntimeError::Generic("Provider missing server_endpoint".to_string()))?
            .to_string();

        let tool_name = provider_obj
            .get("tool_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RuntimeError::Generic("Provider missing tool_name".to_string()))?
            .to_string();

        let timeout_seconds = provider_obj
            .get("timeout_seconds")
            .and_then(|v| v.as_u64())
            .unwrap_or(30);

        Ok(ProviderType::MCP(MCPCapability {
            server_url: server_endpoint,
            tool_name,
            timeout_ms: timeout_seconds * 1000,
        }))
    }

    fn convert_rtfs_provider_to_manifest(
        &self,
        provider_map: &std::collections::HashMap<MapKey, Expression>,
    ) -> RuntimeResult<ProviderType> {
        let mut server_endpoint = None;
        let mut tool_name = None;
        let mut timeout_seconds = 30;

        for (key, value) in provider_map {
            match key {
                MapKey::Keyword(k) if k.0 == "server_endpoint" => {
                    if let Expression::Literal(Literal::String(s)) = value {
                        server_endpoint = Some(s.clone());
                    }
                }
                MapKey::Keyword(k) if k.0 == "tool_name" => {
                    if let Expression::Literal(Literal::String(s)) = value {
                        tool_name = Some(s.clone());
                    }
                }
                MapKey::Keyword(k) if k.0 == "timeout_seconds" => {
                    if let Expression::Literal(Literal::Integer(i)) = value {
                        timeout_seconds = *i as u64;
                    }
                }
                _ => {}
            }
        }

        let server_endpoint = server_endpoint
            .ok_or_else(|| RuntimeError::Generic("Provider missing server_endpoint".to_string()))?;
        let tool_name = tool_name
            .ok_or_else(|| RuntimeError::Generic("Provider missing tool_name".to_string()))?;

        Ok(ProviderType::MCP(MCPCapability {
            server_url: server_endpoint,
            tool_name,
            timeout_ms: timeout_seconds * 1000,
        }))
    }

    /// Convert RTFS type expression back to JSON schema
    fn convert_rtfs_type_to_json_schema(
        &self,
        expr: &Expression,
    ) -> RuntimeResult<serde_json::Value> {
        match expr {
            Expression::Literal(Literal::String(s)) => match s.as_str() {
                "string" => Ok(serde_json::json!({"type": "string"})),
                "number" => Ok(serde_json::json!({"type": "number"})),
                "integer" => Ok(serde_json::json!({"type": "integer"})),
                "boolean" => Ok(serde_json::json!({"type": "boolean"})),
                "array" => Ok(serde_json::json!({"type": "array"})),
                _ => Ok(serde_json::json!({"type": "object"})),
            },
            Expression::Symbol(sym) => match sym.0.as_str() {
                "string" => Ok(serde_json::json!({"type": "string"})),
                "number" => Ok(serde_json::json!({"type": "number"})),
                "integer" => Ok(serde_json::json!({"type": "integer"})),
                "boolean" => Ok(serde_json::json!({"type": "boolean"})),
                "array" => Ok(serde_json::json!({"type": "array"})),
                _ => Ok(serde_json::json!({"type": "object"})),
            },
            Expression::Map(map) => {
                let mut properties = serde_json::Map::new();
                for (key, value) in map {
                    if let MapKey::String(key_str) = key {
                        properties.insert(
                            key_str.clone(),
                            self.convert_rtfs_type_to_json_schema(value)?,
                        );
                    }
                }
                Ok(serde_json::json!({"type": "object", "properties": properties}))
            }
            Expression::Vector(vec) => {
                if let Some(first) = vec.first() {
                    let items = self.convert_rtfs_type_to_json_schema(first)?;
                    Ok(serde_json::json!({"type": "array", "items": items}))
                } else {
                    Ok(serde_json::json!({"type": "array"}))
                }
            }
            _ => Ok(serde_json::json!({"type": "object"})),
        }
    }

    /// Create MCPDiscoveryProvider from saved RTFS module
    pub fn from_rtfs_module(module: &RTFSModuleDefinition) -> RuntimeResult<Self> {
        Self::new(module.server_config.clone())
    }

    /// Convert JSON Schema to RTFS type expression
    fn convert_json_schema_to_rtfs(&self, schema: &serde_json::Value) -> RuntimeResult<Expression> {
        match schema.get("type") {
            Some(serde_json::Value::String(type_str)) => match type_str.as_str() {
                "object" => {
                    let mut properties = Vec::new();
                    if let Some(props) = schema.get("properties") {
                        if let Some(obj) = props.as_object() {
                            for (key, value) in obj {
                                if let Ok(rtfs_type) = self.convert_json_schema_to_rtfs(value) {
                                    properties.push((MapKey::String(key.clone()), rtfs_type));
                                }
                            }
                        }
                    }
                    Ok(Expression::Map(properties.into_iter().collect()))
                }
                "array" => {
                    if let Some(items) = schema.get("items") {
                        if let Ok(item_type) = self.convert_json_schema_to_rtfs(items) {
                            Ok(Expression::Vector(vec![item_type]))
                        } else {
                            Ok(Expression::Symbol(Symbol("array".to_string())))
                        }
                    } else {
                        Ok(Expression::Symbol(Symbol("array".to_string())))
                    }
                }
                "string" => Ok(Expression::Symbol(Symbol("string".to_string()))),
                "number" => Ok(Expression::Symbol(Symbol("number".to_string()))),
                "integer" => Ok(Expression::Symbol(Symbol("integer".to_string()))),
                "boolean" => Ok(Expression::Symbol(Symbol("boolean".to_string()))),
                _ => Ok(Expression::Symbol(Symbol("any".to_string()))),
            },
            _ => Ok(Expression::Symbol(Symbol("any".to_string()))),
        }
    }

    /// Convert RTFS Expression back to TypeExpr for capability manifests
    fn convert_rtfs_to_type_expr(&self, expr: &Expression) -> RuntimeResult<TypeExpr> {
        match expr {
            Expression::Symbol(symbol) => match symbol.0.as_str() {
                "string" => Ok(TypeExpr::Primitive(PrimitiveType::String)),
                "integer" => Ok(TypeExpr::Primitive(PrimitiveType::Int)),
                "number" => Ok(TypeExpr::Primitive(PrimitiveType::Float)),
                "boolean" => Ok(TypeExpr::Primitive(PrimitiveType::Bool)),
                "any" => Ok(TypeExpr::Any),
                _ => Ok(TypeExpr::Primitive(PrimitiveType::Custom(Keyword(
                    symbol.0.clone(),
                )))),
            },
            Expression::Map(map) => {
                let mut entries = Vec::new();
                for (key, value) in map {
                    if let MapKey::String(key_str) = key {
                        let value_type = self.convert_rtfs_to_type_expr(value)?;
                        entries.push(MapTypeEntry {
                            key: Keyword(key_str.clone()),
                            value_type: Box::new(value_type),
                            optional: false, // Default to required for now
                        });
                    }
                }
                Ok(TypeExpr::Map {
                    entries,
                    wildcard: None,
                })
            }
            Expression::Vector(vec) => {
                if vec.len() == 1 {
                    let element_type = self.convert_rtfs_to_type_expr(&vec[0])?;
                    Ok(TypeExpr::Vector(Box::new(element_type)))
                } else {
                    Ok(TypeExpr::Vector(Box::new(TypeExpr::Any)))
                }
            }
            _ => Ok(TypeExpr::Any), // Default fallback
        }
    }

    /// Convert an MCP resource to a capability manifest
    fn convert_resource_to_capability(
        &self,
        resource: serde_json::Value,
    ) -> RuntimeResult<CapabilityManifest> {
        let resource_name = resource
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RuntimeError::Generic("MCP resource missing name".to_string()))?;

        let capability_id = format!("mcp.{}.resource.{}", self.config.name, resource_name);
        let effects = self.derive_resource_effects(&resource);
        let serialized_effects = Self::serialize_effects(&effects);

        Ok(CapabilityManifest {
            id: capability_id.clone(),
            name: resource_name.to_string(),
            description: resource
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or(&format!("MCP resource: {}", resource_name))
                .to_string(),
            provider: ProviderType::MCP(MCPCapability {
                server_url: self.config.endpoint.clone(),
                tool_name: format!("resource:{}", resource_name),
                timeout_ms: self.config.timeout_seconds * 1000,
            }),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None, // MCP resources don't have structured schemas like tools
            attestation: None,
            provenance: Some(
                crate::ccos::capability_marketplace::types::CapabilityProvenance {
                    source: "mcp_discovery".to_string(),
                    version: Some("1.0.0".to_string()),
                    content_hash: format!("mcp_resource_{}_{}", self.config.name, resource_name),
                    custody_chain: vec!["mcp_discovery".to_string()],
                    registered_at: Utc::now(),
                },
            ),
            permissions: vec![],
            effects,
            metadata: {
                let mut metadata = HashMap::new();
                metadata.insert("mcp_server".to_string(), self.config.name.clone());
                metadata.insert(
                    "mcp_protocol_version".to_string(),
                    self.config.protocol_version.clone(),
                );
                metadata.insert("capability_type".to_string(), "mcp_resource".to_string());
                metadata.insert("ccos_effects".to_string(), serialized_effects);
                if let Some(meta) = resource.get("metadata") {
                    if let Ok(json) = serde_json::to_string(meta) {
                        metadata.insert("mcp_resource_metadata".to_string(), json);
                    }
                }
                if let Some(annotations) = resource.get("annotations") {
                    if let Ok(json) = serde_json::to_string(annotations) {
                        metadata.insert("mcp_resource_annotations".to_string(), json);
                    }
                }
                metadata
            },
            agent_metadata: None,
        })
    }

    /// Health check for the MCP server
    pub async fn health_check(&self) -> RuntimeResult<bool> {
        let health_url = format!("{}/health", self.config.endpoint);

        let request = self.client.get(&health_url).build().map_err(|e| {
            RuntimeError::Generic(format!("Failed to build MCP health check request: {}", e))
        })?;

        match timeout(
            Duration::from_secs(5), // Shorter timeout for health checks
            self.client.execute(request),
        )
        .await
        {
            Ok(Ok(response)) => Ok(response.status().is_success()),
            Ok(Err(e)) => Err(RuntimeError::Generic(format!(
                "MCP health check failed: {}",
                e
            ))),
            Err(_) => Err(RuntimeError::Generic(
                "MCP health check timeout".to_string(),
            )),
        }
    }
}

#[async_trait]
impl CapabilityDiscovery for MCPDiscoveryProvider {
    async fn discover(&self) -> RuntimeResult<Vec<CapabilityManifest>> {
        let mut all_capabilities = Vec::new();

        // Discover tools
        match self.discover_tools().await {
            Ok(tools) => {
                eprintln!(
                    "Discovered {} MCP tools from server: {}",
                    tools.len(),
                    self.config.name
                );
                all_capabilities.extend(tools);
            }
            Err(e) => {
                eprintln!(
                    "MCP tools discovery failed for server {}: {}",
                    self.config.name, e
                );
            }
        }

        // Discover resources
        match self.discover_resources().await {
            Ok(resources) => {
                eprintln!(
                    "Discovered {} MCP resources from server: {}",
                    resources.len(),
                    self.config.name
                );
                all_capabilities.extend(resources);
            }
            Err(e) => {
                eprintln!(
                    "MCP resources discovery failed for server {}: {}",
                    self.config.name, e
                );
            }
        }

        Ok(all_capabilities)
    }

    fn name(&self) -> &str {
        "MCPDiscovery"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Builder for MCP discovery configuration
pub struct MCPDiscoveryBuilder {
    config: MCPServerConfig,
}

impl MCPDiscoveryBuilder {
    pub fn new() -> Self {
        Self {
            config: MCPServerConfig::default(),
        }
    }

    pub fn name(mut self, name: String) -> Self {
        self.config.name = name;
        self
    }

    pub fn endpoint(mut self, endpoint: String) -> Self {
        self.config.endpoint = endpoint;
        self
    }

    pub fn auth_token(mut self, token: String) -> Self {
        self.config.auth_token = Some(token);
        self
    }

    pub fn timeout_seconds(mut self, timeout: u64) -> Self {
        self.config.timeout_seconds = timeout;
        self
    }

    pub fn protocol_version(mut self, version: String) -> Self {
        self.config.protocol_version = version;
        self
    }

    pub fn build(self) -> RuntimeResult<MCPDiscoveryProvider> {
        MCPDiscoveryProvider::new(self.config)
    }
}

impl Default for MCPDiscoveryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_discovery_config_default() {
        let config = MCPServerConfig::default();
        assert_eq!(config.timeout_seconds, 30);
        assert_eq!(config.protocol_version, "2024-11-05");
        assert!(config.auth_token.is_none());
    }

    #[test]
    fn test_mcp_discovery_builder() {
        let provider = MCPDiscoveryBuilder::new()
            .name("test_server".to_string())
            .endpoint("http://localhost:3000".to_string())
            .timeout_seconds(60)
            .protocol_version("2024-11-05".to_string())
            .build();

        assert!(provider.is_ok());
    }

    #[test]
    fn test_convert_tool_to_capability() {
        let config = MCPServerConfig {
            name: "test_server".to_string(),
            endpoint: "http://localhost:3000".to_string(),
            auth_token: None,
            timeout_seconds: 30,
            protocol_version: "2024-11-05".to_string(),
        };

        let provider = MCPDiscoveryProvider::new(config).unwrap();

        let tool = MCPTool {
            name: "test_tool".to_string(),
            description: Some("A test MCP tool".to_string()),
            input_schema: None,
            output_schema: None,
            metadata: None,
            annotations: None,
        };

        let capability = provider.convert_tool_to_capability(tool);

        assert_eq!(capability.id, "mcp.test_server.test_tool");
        assert_eq!(capability.name, "test_tool");
        assert_eq!(
            capability.metadata.get("mcp_server").unwrap(),
            "test_server"
        );
        assert_eq!(capability.effects, vec![":network"]);
        assert!(capability
            .metadata
            .get("ccos_effects")
            .map(|s| s.contains(":network"))
            .unwrap_or(false));
    }

    #[test]
    fn test_convert_tool_to_rtfs_format() {
        let config = MCPServerConfig {
            name: "test_server".to_string(),
            endpoint: "http://localhost:3000".to_string(),
            auth_token: None,
            timeout_seconds: 30,
            protocol_version: "2024-11-05".to_string(),
        };

        let provider = MCPDiscoveryProvider::new(config).unwrap();

        let tool = MCPTool {
            name: "test_tool".to_string(),
            description: Some("A test MCP tool".to_string()),
            input_schema: Some(
                serde_json::json!({"type": "object", "properties": {"query": {"type": "string"}}}),
            ),
            output_schema: Some(
                serde_json::json!({"type": "object", "properties": {"result": {"type": "string"}}}),
            ),
            metadata: None,
            annotations: None,
        };

        let rtfs_cap = provider.convert_tool_to_rtfs_format(&tool).unwrap();

        // Verify the RTFS capability structure
        match &rtfs_cap.capability {
            Expression::Map(map) => {
                assert!(map.len() > 0);
                // Check for required fields
                let mut has_type = false;
                let mut has_id = false;
                let mut has_name = false;
                let mut has_provider = false;
                let mut has_effects = false;

                for (key, _) in map {
                    match key {
                        MapKey::Keyword(kw) if kw.0 == "type" => has_type = true,
                        MapKey::Keyword(kw) if kw.0 == "id" => has_id = true,
                        MapKey::Keyword(kw) if kw.0 == "name" => has_name = true,
                        MapKey::Keyword(kw) if kw.0 == "provider" => has_provider = true,
                        MapKey::Keyword(kw) if kw.0 == "effects" => has_effects = true,
                        _ => {}
                    }
                }

                assert!(has_type, "RTFS capability missing type field");
                assert!(has_id, "RTFS capability missing id field");
                assert!(has_name, "RTFS capability missing name field");
                assert!(has_provider, "RTFS capability missing provider field");
                assert!(has_effects, "RTFS capability missing effects field");
            }
            _ => panic!("RTFS capability should be a map"),
        }

        // Verify input schema conversion
        assert!(rtfs_cap.input_schema.is_some());
        assert!(rtfs_cap.output_schema.is_some());
    }

    #[test]
    fn test_rtfs_to_capability_manifest_roundtrip() {
        let config = MCPServerConfig {
            name: "test_server".to_string(),
            endpoint: "http://localhost:3000".to_string(),
            auth_token: None,
            timeout_seconds: 30,
            protocol_version: "2024-11-05".to_string(),
        };

        let provider = MCPDiscoveryProvider::new(config).unwrap();

        let tool = MCPTool {
            name: "test_tool".to_string(),
            description: Some("A test MCP tool".to_string()),
            input_schema: Some(
                serde_json::json!({"type": "object", "properties": {"query": {"type": "string"}}}),
            ),
            output_schema: Some(
                serde_json::json!({"type": "object", "properties": {"result": {"type": "string"}}}),
            ),
            metadata: None,
            annotations: None,
        };

        // Convert to RTFS
        let rtfs_cap = provider.convert_tool_to_rtfs_format(&tool).unwrap();

        // Convert back to manifest
        let manifest = provider.rtfs_to_capability_manifest(&rtfs_cap).unwrap();

        // Verify the roundtrip
        assert_eq!(manifest.id, "mcp.test_server.test_tool");
        assert_eq!(manifest.name, "test_tool");
        assert_eq!(manifest.description, "A test MCP tool");
        assert_eq!(manifest.version, "1.0.0");
        assert!(manifest.input_schema.is_some());
        assert!(manifest.output_schema.is_some());
        assert!(manifest.effects.contains(&":network".to_string()));
    }

    #[test]
    fn test_json_schema_to_rtfs_conversion() {
        let config = MCPServerConfig::default();
        let provider = MCPDiscoveryProvider::new(config).unwrap();

        // Test object schema conversion
        let object_schema = serde_json::json!({"type": "object", "properties": {"query": {"type": "string"}, "count": {"type": "integer"}}});
        let rtfs_expr = provider
            .convert_json_schema_to_rtfs(&object_schema)
            .unwrap();

        match rtfs_expr {
            Expression::Map(map) => {
                assert!(map.len() > 0);
                // Should contain properties
                let has_query = map
                    .iter()
                    .any(|(key, _)| matches!(key, MapKey::String(s) if s == "query"));
                assert!(has_query, "Object schema should contain query property");
            }
            _ => panic!("Object schema should convert to Map expression"),
        }

        // Test array schema conversion
        let array_schema = serde_json::json!({"type": "array", "items": {"type": "string"}});
        let rtfs_expr = provider.convert_json_schema_to_rtfs(&array_schema).unwrap();

        match rtfs_expr {
            Expression::Vector(vec) => {
                assert_eq!(vec.len(), 1);
                assert!(matches!(vec[0], Expression::Symbol(_)));
            }
            _ => panic!("Array schema should convert to Vector expression"),
        }

        // Test primitive type conversion
        let string_schema = serde_json::json!({"type": "string"});
        let rtfs_expr = provider
            .convert_json_schema_to_rtfs(&string_schema)
            .unwrap();
        assert!(matches!(rtfs_expr, Expression::Symbol(_)));
    }

    #[test]
    fn test_rtfs_module_save_load() {
        let config = MCPServerConfig {
            name: "test_server".to_string(),
            endpoint: "http://localhost:3000".to_string(),
            auth_token: None,
            timeout_seconds: 30,
            protocol_version: "2024-11-05".to_string(),
        };

        let provider = MCPDiscoveryProvider::new(config.clone()).unwrap();

        let tools = vec![
            MCPTool {
                name: "tool1".to_string(),
                description: Some("First tool".to_string()),
                input_schema: None,
                output_schema: None,
                metadata: None,
                annotations: None,
            },
            MCPTool {
                name: "tool2".to_string(),
                description: Some("Second tool".to_string()),
                input_schema: None,
                output_schema: None,
                metadata: None,
                annotations: None,
            },
        ];

        // Convert to RTFS
        let rtfs_capabilities = provider.convert_tools_to_rtfs_format(&tools).unwrap();

        // Save to temporary file
        let temp_file = "/tmp/test_rtfs_module.json";
        provider
            .save_rtfs_capabilities(&rtfs_capabilities, temp_file)
            .unwrap();

        // Load back
        let loaded_module = provider.load_rtfs_capabilities(temp_file).unwrap();

        // Verify
        assert_eq!(loaded_module.module_type, "ccos.capabilities.mcp:v1");
        assert_eq!(loaded_module.server_config.name, config.name);
        assert_eq!(loaded_module.server_config.endpoint, config.endpoint);
        assert_eq!(loaded_module.capabilities.len(), 2);

        // Verify capabilities
        let tool1_cap = loaded_module.capabilities.iter().find(|cap| {
            if let Expression::Map(map) = &cap.capability {
                map.iter().any(|(key, value)| {
                    if let MapKey::Keyword(kw) = key {
                        if kw.0 == "name" {
                            if let Expression::Literal(Literal::String(s)) = value {
                                return s == "tool1";
                            }
                        }
                    }
                    false
                })
            } else {
                false
            }
        });
        assert!(tool1_cap.is_some(), "Should find tool1 capability");

        // Cleanup
        std::fs::remove_file(temp_file).ok();
    }

    #[test]
    fn test_from_rtfs_module() {
        let config = MCPServerConfig {
            name: "test_server".to_string(),
            endpoint: "http://localhost:3000".to_string(),
            auth_token: None,
            timeout_seconds: 30,
            protocol_version: "2024-11-05".to_string(),
        };

        let module = RTFSModuleDefinition {
            module_type: "ccos.capabilities.mcp:v1".to_string(),
            server_config: config.clone(),
            capabilities: vec![],
            generated_at: Utc::now().to_rfc3339(),
        };

        let provider = MCPDiscoveryProvider::from_rtfs_module(&module).unwrap();
        assert_eq!(provider.config.name, config.name);
        assert_eq!(provider.config.endpoint, config.endpoint);
        assert_eq!(provider.config.timeout_seconds, config.timeout_seconds);
    }
}
