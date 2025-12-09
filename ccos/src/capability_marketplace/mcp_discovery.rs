use crate::capability_marketplace::types::CapabilityDiscovery;
use crate::capability_marketplace::types::{
    CapabilityManifest, LocalCapability, MCPCapability, ProviderType,
};
use crate::mcp::types::DiscoveredMCPTool;
use crate::synthesis::mcp_introspector::MCPIntrospector;
use async_trait::async_trait;
use chrono::Utc;
use log::debug;
use rtfs::ast::{
    Expression, Keyword, Literal, MapKey, MapTypeEntry, PrimitiveType, Symbol, TopLevel, TypeExpr,
};
use rtfs::runtime::environment::Environment;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::evaluator::Evaluator;
use rtfs::runtime::module_runtime::ModuleRegistry;
use rtfs::runtime::host_interface::HostInterface;
use rtfs::runtime::pure_host;
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::{Runtime, TreeWalkingStrategy};
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
    pub input_schema: Option<TypeExpr>,
    pub output_schema: Option<TypeExpr>,
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
///
/// This is a thin adapter implementing the `CapabilityDiscovery` trait
/// that delegates to the unified `MCPDiscoveryService` for all discovery operations.
pub struct MCPDiscoveryProvider {
    config: MCPServerConfig,
    /// Unified discovery service - always used for discovery
    discovery_service: std::sync::Arc<crate::mcp::core::MCPDiscoveryService>,
    /// Factory used to create a Host for executing RTFS capabilities
    rtfs_host_factory: std::sync::Arc<
        dyn Fn() -> std::sync::Arc<dyn HostInterface + Send + Sync> + Send + Sync,
    >,
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
    ///
    /// Internally creates an `MCPDiscoveryService` for all discovery operations.
    pub fn new(config: MCPServerConfig) -> RuntimeResult<Self> {
        Self::new_with_rtfs_host_factory(config, std::sync::Arc::new(|| {
            let host: std::sync::Arc<dyn HostInterface + Send + Sync> =
                std::sync::Arc::new(pure_host::PureHost::new());
            host
        }))
    }

    /// Create a new MCP discovery provider with a custom RTFS Host factory
    pub fn new_with_rtfs_host_factory(
        config: MCPServerConfig,
        rtfs_host_factory: std::sync::Arc<
            dyn Fn() -> std::sync::Arc<dyn HostInterface + Send + Sync> + Send + Sync,
        >,
    ) -> RuntimeResult<Self> {
        // Build auth headers if token provided
        let auth_headers = config.auth_token.as_ref().map(|token| {
            let mut headers = HashMap::new();
            headers.insert("Authorization".to_string(), format!("Bearer {}", token));
            headers
        });

        // Create the unified discovery service
        let discovery_service = std::sync::Arc::new(
            crate::mcp::core::MCPDiscoveryService::with_auth_headers(auth_headers),
        );

        Ok(Self {
            config,
            discovery_service,
            rtfs_host_factory,
        })
    }

    /// Create a new MCP discovery provider with an existing discovery service
    ///
    /// Useful when you want to share a discovery service across multiple providers.
    pub fn with_discovery_service(
        config: MCPServerConfig,
        discovery_service: std::sync::Arc<crate::mcp::core::MCPDiscoveryService>,
    ) -> Self {
        Self::with_discovery_service_and_host(
            config,
            discovery_service,
            std::sync::Arc::new(|| {
                let host: std::sync::Arc<dyn HostInterface + Send + Sync> =
                    std::sync::Arc::new(pure_host::PureHost::new());
                host
            }),
        )
    }

    /// Create a new MCP discovery provider with an existing discovery service and Host factory
    pub fn with_discovery_service_and_host(
        config: MCPServerConfig,
        discovery_service: std::sync::Arc<crate::mcp::core::MCPDiscoveryService>,
        rtfs_host_factory: std::sync::Arc<
            dyn Fn() -> std::sync::Arc<dyn HostInterface + Send + Sync> + Send + Sync,
        >,
    ) -> Self {
        Self {
            config,
            discovery_service,
            rtfs_host_factory,
        }
    }

    /// Discover tools from the MCP server
    ///
    /// This method delegates to the unified `MCPDiscoveryService` for all discovery.
    pub async fn discover_tools(&self) -> RuntimeResult<Vec<CapabilityManifest>> {
        let options = crate::mcp::types::DiscoveryOptions {
            introspect_output_schemas: false, // Can be made configurable
            use_cache: true,
            register_in_marketplace: false,
            export_to_rtfs: false,
            export_directory: None,
            auth_headers: self.build_auth_headers(),
            ..Default::default()
        };

        let discovered_tools = self
            .discovery_service
            .discover_tools(&self.config, &options)
            .await?;

        // Convert to manifests
        let mut capabilities = Vec::new();
        for tool in discovered_tools {
            let manifest = self.discovery_service.tool_to_manifest(&tool, &self.config);
            capabilities.push(manifest);
        }

        Ok(capabilities)
    }

    /// Discover resources from the MCP server
    ///
    /// This method delegates to the unified `MCPDiscoveryService` for resource discovery.
    pub async fn discover_resources(&self) -> RuntimeResult<Vec<CapabilityManifest>> {
        let resources = self
            .discovery_service
            .discover_resources(&self.config)
            .await?;

        // Convert MCP resources to capability manifests
        let mut capabilities = Vec::new();
        for resource in resources {
            if let Ok(capability) = self.convert_resource_to_capability(resource) {
                capabilities.push(capability);
            }
        }

        Ok(capabilities)
    }

    /// Patch input schema for list_issues to fix known API inconsistencies
    fn patch_list_issues_schema(&self, mut input_schema: Option<TypeExpr>) -> Option<TypeExpr> {
        if let Some(TypeExpr::Map {
            entries,
            wildcard: _,
        }) = &mut input_schema
        {
            for entry in entries {
                let key = entry.key.0.as_str();
                match key {
                    "state" => {
                        // Force state to be Optional(Enum(OPEN, CLOSED))
                        // Discard whatever was there if it was wrong (e.g. containing ALL)
                        entry.value_type =
                            Box::new(TypeExpr::Optional(Box::new(TypeExpr::Enum(vec![
                                Literal::String("OPEN".to_string()),
                                Literal::String("CLOSED".to_string()),
                            ]))));
                    }
                    "direction" => {
                        // Force direction to be Optional(Enum(ASC, DESC))
                        entry.value_type =
                            Box::new(TypeExpr::Optional(Box::new(TypeExpr::Enum(vec![
                                Literal::String("ASC".to_string()),
                                Literal::String("DESC".to_string()),
                            ]))));
                    }
                    "orderBy" => {
                        // Force orderBy to be Optional(Enum(CREATED_AT, UPDATED_AT, COMMENTS))
                        entry.value_type =
                            Box::new(TypeExpr::Optional(Box::new(TypeExpr::Enum(vec![
                                Literal::String("CREATED_AT".to_string()),
                                Literal::String("UPDATED_AT".to_string()),
                                Literal::String("COMMENTS".to_string()),
                            ]))));
                    }
                    _ => {}
                }
            }
        }
        input_schema
    }

    /// Convert an MCP tool to a capability manifest
    fn convert_tool_to_capability(&self, tool: MCPTool) -> CapabilityManifest {
        let capability_id = format!("mcp.{}.{}", self.config.name, tool.name);
        let effects = self.derive_tool_effects(&tool);
        let serialized_effects = Self::serialize_effects(&effects);

        let mut input_schema = tool
            .input_schema
            .as_ref()
            .and_then(|schema| MCPIntrospector::type_expr_from_json_schema(schema).ok());

        if tool.name == "list_issues" {
            input_schema = self.patch_list_issues_schema(input_schema);
        }

        let output_schema = tool
            .output_schema
            .as_ref()
            .and_then(|schema| MCPIntrospector::type_expr_from_json_schema(schema).ok());

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
                auth_token: self.config.auth_token.clone(),
            }),
            version: "1.0.0".to_string(),
            input_schema,
            output_schema,
            attestation: None,
            provenance: Some(crate::capability_marketplace::types::CapabilityProvenance {
                source: "mcp_discovery".to_string(),
                version: Some("1.0.0".to_string()),
                content_hash: format!("mcp_{}_{}", self.config.name, tool.name),
                custody_chain: vec!["mcp_discovery".to_string()],
                registered_at: Utc::now(),
            }),
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
            domains: Vec::new(),
            categories: Vec::new(),
        }
    }

    async fn convert_tool_to_capability_with_introspection(
        &self,
        tool: MCPTool,
    ) -> RuntimeResult<CapabilityManifest> {
        let mut manifest = self.convert_tool_to_capability(tool.clone());
        if let Some(headers) = self.build_auth_headers() {
            if let Ok((schema_opt, sample_opt)) = self
                .introspect_output_schema_for_tool(&tool, manifest.input_schema.clone(), &headers)
                .await
            {
                if let Some(schema) = schema_opt {
                    manifest.output_schema = Some(schema);
                }
                if let Some(sample) = sample_opt {
                    manifest
                        .metadata
                        .insert("output_snippet".to_string(), sample);
                }
            }
        }
        Ok(manifest)
    }

    fn build_auth_headers(&self) -> Option<HashMap<String, String>> {
        self.config.auth_token.as_ref().map(|token| {
            let mut headers = HashMap::new();
            headers.insert("Authorization".to_string(), format!("Bearer {}", token));
            headers
        })
    }

    async fn introspect_output_schema_for_tool(
        &self,
        tool: &MCPTool,
        input_schema: Option<TypeExpr>,
        auth_headers: &HashMap<String, String>,
    ) -> RuntimeResult<(Option<TypeExpr>, Option<String>)> {
        let discovered = self.build_discovered_tool(tool, input_schema);
        let introspector = MCPIntrospector::new();
        match introspector
            .introspect_output_schema(
                &discovered,
                &self.config.endpoint,
                &self.config.name,
                Some(auth_headers.clone()),
                None,
            )
            .await
        {
            Ok((Some(schema), maybe_sample)) => {
                eprintln!(
                    "✅ MCP Discovery: Inferred output schema for '{}'",
                    tool.name
                );
                Ok((Some(schema), maybe_sample))
            }
            Ok((None, Some(sample))) => Ok((None, Some(sample))),
            Ok((None, None)) => Ok((None, None)),
            Err(err) => {
                eprintln!(
                    "⚠️ MCP Discovery: Output schema introspection failed for '{}': {}",
                    tool.name, err
                );
                Ok((None, None))
            }
        }
    }

    fn build_discovered_tool(
        &self,
        tool: &MCPTool,
        input_schema: Option<TypeExpr>,
    ) -> DiscoveredMCPTool {
        DiscoveredMCPTool {
            tool_name: tool.name.clone(),
            description: tool.description.clone(),
            input_schema,
            output_schema: None,
            input_schema_json: tool.input_schema.clone(),
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
                            // Note: tool_name in metadata duplicates :name in the capability map,
                            // but is kept for introspection/debugging to clearly identify the MCP tool.
                            // The provider map also has tool_name, which is required for MCP protocol calls.
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
                            // Note: ccos_effects removed - effects are already in the capability map
                            // at the top level, so storing them again in metadata is redundant
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
            .and_then(|schema| self.convert_json_schema_to_rtfs(schema).ok())
            .and_then(|expr| self.convert_rtfs_to_type_expr(&expr).ok());

        let output_schema = tool
            .output_schema
            .as_ref()
            .and_then(|schema| self.convert_json_schema_to_rtfs(schema).ok())
            .and_then(|expr| self.convert_rtfs_to_type_expr(&expr).ok());

        // Add input-schema and output-schema to the capability map itself
        let mut capability_map = match capability {
            Expression::Map(map) => map,
            _ => {
                return Err(RuntimeError::Generic(
                    "Expected capability to be a Map".to_string(),
                ))
            }
        };

        // Track whether schemas were added to the capability map
        let mut input_schema_added = false;
        let mut output_schema_added = false;

        // Add input-schema to capability map
        if let Some(ref input_schema_expr) = input_schema {
            // Convert TypeExpr to Expression by parsing RTFS text representation
            let rtfs_text = self.type_expr_to_rtfs_text(input_schema_expr);
            if let Ok(parsed) = rtfs::parser::parse(&rtfs_text) {
                if let Some(TopLevel::Expression(expr)) = parsed.first() {
                    capability_map.insert(
                        MapKey::Keyword(Keyword("input-schema".to_string())),
                        expr.clone(),
                    );
                    input_schema_added = true;
                }
            }
        }

        // Add output-schema to capability map (use :any if not available)
        if let Some(ref output_schema_expr) = output_schema {
            // Convert TypeExpr to Expression by parsing RTFS text representation
            let rtfs_text = self.type_expr_to_rtfs_text(output_schema_expr);
            if let Ok(parsed) = rtfs::parser::parse(&rtfs_text) {
                if let Some(TopLevel::Expression(expr)) = parsed.first() {
                    capability_map.insert(
                        MapKey::Keyword(Keyword("output-schema".to_string())),
                        expr.clone(),
                    );
                    output_schema_added = true;
                }
            }
        } else {
            // Use :any as default when output schema is unknown
            capability_map.insert(
                MapKey::Keyword(Keyword("output-schema".to_string())),
                Expression::Literal(Literal::Keyword(Keyword("any".to_string()))),
            );
            output_schema_added = true;
        }

        // If schemas were added to the capability map, set them to None in RTFSCapabilityDefinition
        // to prevent duplication when saving (save_rtfs_capabilities will use the ones in the map)
        Ok(RTFSCapabilityDefinition {
            capability: Expression::Map(capability_map),
            input_schema: if input_schema_added {
                None
            } else {
                input_schema
            },
            output_schema: if output_schema_added {
                None
            } else {
                output_schema
            },
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
                rtfs_content.push_str("\n");
            }
            rtfs_content.push_str("      {\n");

            // Check if schemas are already in the capability map to avoid duplication
            // Use explicit iteration since HashMap key comparison may not work as expected
            let mut input_schema_in_map = false;
            let mut output_schema_in_map = false;

            if let Expression::Map(ref cap_map) = capability.capability {
                for (key, _) in cap_map.iter() {
                    if let MapKey::Keyword(kw) = key {
                        if kw.0 == "input-schema" {
                            input_schema_in_map = true;
                        }
                        if kw.0 == "output-schema" {
                            output_schema_in_map = true;
                        }
                    }
                }
            }

            // Write capability map (schemas are inside, skip wrapper fields)
            rtfs_content.push_str("        :capability ");
            rtfs_content.push_str(&self.expression_to_rtfs_text(&capability.capability, 0));

            // Only write wrapper if the capability map is closed without a trailing comma
            // and we're NOT writing any wrapper fields
            if input_schema_in_map && output_schema_in_map {
                // Schemas are in the map, no wrapper fields needed
                rtfs_content.push_str("\n      }");
            } else {
                // Need to add wrapper fields for backward compatibility
                rtfs_content.push_str("\n");

                if !input_schema_in_map {
                    if let Some(input_schema) = &capability.input_schema {
                        rtfs_content.push_str("        :input-schema ");
                        rtfs_content.push_str(&self.type_expr_to_rtfs_text(input_schema));
                        rtfs_content.push_str("\n");
                    }
                }

                if !output_schema_in_map {
                    if let Some(output_schema) = &capability.output_schema {
                        rtfs_content.push_str("        :output-schema ");
                        rtfs_content.push_str(&self.type_expr_to_rtfs_text(output_schema));
                    }
                }
                rtfs_content.push_str("\n      }");
            }
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

    /// Load RTFS capabilities from a file using the proper RTFS parser
    /// Supports both module format and individual capability format
    pub fn load_rtfs_capabilities(&self, file_path: &str) -> RuntimeResult<RTFSModuleDefinition> {
        let rtfs_content = std::fs::read_to_string(file_path).map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to read RTFS capabilities from file '{}': {}",
                file_path, e
            ))
        })?;

        // Use the real RTFS parser instead of string hacks
        use rtfs::parser::parse;
        let top_levels = parse(&rtfs_content).map_err(|e| {
            RuntimeError::Generic(format!("Failed to parse RTFS file '{}': {}", file_path, e))
        })?;

        // Try module format first, then individual capability format
        match self.extract_module_from_ast(top_levels.clone()) {
            Ok(module) => Ok(module),
            Err(_) => {
                // Try parsing as individual capability file
                self.extract_module_from_individual_capability(top_levels, file_path)
            }
        }
    }

    /// Parse an individual capability file (capability "id" :name "..." format)
    /// and wrap it in a module structure for compatibility
    fn extract_module_from_individual_capability(
        &self,
        top_levels: Vec<TopLevel>,
        file_path: &str,
    ) -> RuntimeResult<RTFSModuleDefinition> {
        // Individual capability files have (capability "id" ...) at top level
        for (i, top_level) in top_levels.iter().enumerate() {
            match top_level {
                TopLevel::Capability(cap_def) => {
                    // Convert CapabilityDefinition to RTFSCapabilityDefinition
                    let mut capability_map = HashMap::new();

                    // The capability ID is stored in the name field of the definition
                    capability_map.insert(
                        MapKey::Keyword(Keyword("id".to_string())),
                        Expression::Literal(Literal::String(cap_def.name.0.clone())),
                    );

                    for prop in &cap_def.properties {
                        capability_map
                            .insert(MapKey::Keyword(prop.key.clone()), prop.value.clone());
                    }

                    // Extract input/output schemas if present
                    let input_schema = capability_map
                        .get(&MapKey::Keyword(Keyword("input-schema".to_string())))
                        .and_then(|e| self.extract_type_expr(e));

                    let output_schema = capability_map
                        .get(&MapKey::Keyword(Keyword("output-schema".to_string())))
                        .and_then(|e| self.extract_type_expr(e));

                    let capability_def = RTFSCapabilityDefinition {
                        capability: Expression::Map(capability_map),
                        input_schema,
                        output_schema,
                    };

                    // Wrap in a synthetic module for compatibility
                    return Ok(RTFSModuleDefinition {
                        module_type: "ccos.capabilities.individual:v1".to_string(),
                        server_config: MCPServerConfig {
                            name: "individual".to_string(),
                            endpoint: "".to_string(),
                            auth_token: None,
                            timeout_seconds: 30,
                            protocol_version: "2024-11-05".to_string(),
                        },
                        capabilities: vec![capability_def],
                        generated_at: chrono::Utc::now().to_rfc3339(),
                    });
                }
                TopLevel::Expression(expr) => {
                    // Handle both List (if quoted) and FunctionCall (if unquoted)
                    let items_opt = match expr {
                        Expression::List(items) => Some(items.clone()),
                        Expression::FunctionCall { callee, arguments } => {
                            // Convert function call back to list format for parsing
                            let mut items = Vec::with_capacity(arguments.len() + 1);
                            items.push(*callee.clone());
                            items.extend(arguments.clone());
                            Some(items)
                        }
                        _ => None,
                    };

                    if let Some(items) = items_opt {
                        if let Some(Expression::Symbol(sym)) = items.first() {
                            if sym.0 == "capability" {
                                // This is an individual capability
                                let capability_def = self.parse_individual_capability(&items)?;

                                // Wrap in a synthetic module for compatibility
                                return Ok(RTFSModuleDefinition {
                                    module_type: "ccos.capabilities.individual:v1".to_string(),
                                    server_config: MCPServerConfig {
                                        name: "individual".to_string(),
                                        endpoint: "".to_string(),
                                        auth_token: None,
                                        timeout_seconds: 30,
                                        protocol_version: "2024-11-05".to_string(),
                                    },
                                    capabilities: vec![capability_def],
                                    generated_at: chrono::Utc::now().to_rfc3339(),
                                });
                            } else {
                                eprintln!(
                                    "Found symbol '{}' instead of 'capability' at top level",
                                    sym.0
                                );
                            }
                        } else {
                            eprintln!("Empty list at top level");
                        }
                    } else {
                        eprintln!(
                            "Top level item {} is not a List or FunctionCall: {:?}",
                            i, expr
                        );
                    }
                }
                _ => {
                    eprintln!(
                        "Top level item {} is not a Capability or Expression: {:?}",
                        i, top_level
                    );
                }
            }
        }

        Err(RuntimeError::Generic(format!(
            "File '{}' is neither a module nor an individual capability",
            file_path
        )))
    }

    /// Parse an individual capability from (capability "id" :key value ...) format
    fn parse_individual_capability(
        &self,
        items: &[Expression],
    ) -> RuntimeResult<RTFSCapabilityDefinition> {
        // items: [symbol("capability"), string("id"), :key, value, ...]
        let capability_id = if let Some(Expression::Literal(Literal::String(id))) = items.get(1) {
            id.clone()
        } else {
            return Err(RuntimeError::Generic(
                "Individual capability missing ID".to_string(),
            ));
        };

        // Parse key-value pairs into a map
        let mut capability_map = HashMap::new();
        capability_map.insert(
            MapKey::Keyword(Keyword("id".to_string())),
            Expression::Literal(Literal::String(capability_id)),
        );

        let mut i = 2;
        while i < items.len() {
            if let Expression::Literal(Literal::Keyword(key)) = &items[i] {
                if i + 1 < items.len() {
                    capability_map.insert(MapKey::Keyword(key.clone()), items[i + 1].clone());
                    i += 2;
                } else {
                    i += 1;
                }
            } else {
                i += 1;
            }
        }

        // Extract input/output schemas if present
        let input_schema = capability_map
            .get(&MapKey::Keyword(Keyword("input-schema".to_string())))
            .and_then(|e| self.extract_type_expr(e));

        let output_schema = capability_map
            .get(&MapKey::Keyword(Keyword("output-schema".to_string())))
            .and_then(|e| self.extract_type_expr(e));

        Ok(RTFSCapabilityDefinition {
            capability: Expression::Map(capability_map),
            input_schema,
            output_schema,
        })
    }

    /// Extract module definition from parsed RTFS AST
    fn extract_module_from_ast(
        &self,
        top_levels: Vec<TopLevel>,
    ) -> RuntimeResult<RTFSModuleDefinition> {
        // Find the mcp-capabilities-module definition
        // It could be in a Module definition or as an Expression with a def form
        for top_level in top_levels {
            match top_level {
                TopLevel::Module(module_def) => {
                    if module_def.name.0 == "mcp-capabilities-module" {
                        return self.extract_module_from_module_def(&module_def);
                    }
                }
                TopLevel::Expression(expr) => {
                    // Check if it's a (def mcp-capabilities-module ...) expression
                    if let Some(module) = self.try_extract_module_from_expr(&expr) {
                        return Ok(module);
                    }
                }
                _ => continue,
            }
        }

        Err(RuntimeError::Generic(
            "No 'mcp-capabilities-module' definition found in RTFS file".to_string(),
        ))
    }

    /// Try to extract module from a ModuleDefinition
    fn extract_module_from_module_def(
        &self,
        module_def: &rtfs::ast::ModuleDefinition,
    ) -> RuntimeResult<RTFSModuleDefinition> {
        // For now, return error as we need to implement proper module extraction
        Err(RuntimeError::Generic(
            "Module definition extraction not yet implemented".to_string(),
        ))
    }

    /// Try to extract module from an expression (handles def forms)
    fn try_extract_module_from_expr(&self, expr: &Expression) -> Option<RTFSModuleDefinition> {
        // Check if this is a (def ...) expression
        if let Expression::Def(def_expr) = expr {
            if def_expr.symbol.0 == "mcp-capabilities-module" {
                if let Ok(module) = self.extract_module_from_def_expression(&def_expr.value) {
                    return Some(module);
                }
            }
        }
        None
    }

    /// Extract module definition from the def body expression
    fn extract_module_from_def_expression(
        &self,
        expr: &Expression,
    ) -> RuntimeResult<RTFSModuleDefinition> {
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

        // The body should be a Map expression
        if let Expression::Map(map) = expr {
            // Extract module-type
            if let Some(Expression::Literal(Literal::String(s))) =
                map.get(&MapKey::Keyword(Keyword("module-type".to_string())))
            {
                module_type = s.clone();
            }

            // Extract generated-at
            if let Some(Expression::Literal(Literal::String(s))) =
                map.get(&MapKey::Keyword(Keyword("generated-at".to_string())))
            {
                generated_at = s.clone();
            }

            // Extract server-config
            if let Some(Expression::Map(server_map)) =
                map.get(&MapKey::Keyword(Keyword("server-config".to_string())))
            {
                if let Some(Expression::Literal(Literal::String(s))) =
                    server_map.get(&MapKey::Keyword(Keyword("name".to_string())))
                {
                    server_config.name = s.clone();
                }

                if let Some(Expression::Literal(Literal::String(s))) =
                    server_map.get(&MapKey::Keyword(Keyword("endpoint".to_string())))
                {
                    server_config.endpoint = s.clone();
                }

                if let Some(Expression::Literal(Literal::String(s))) =
                    server_map.get(&MapKey::Keyword(Keyword("auth-token".to_string())))
                {
                    server_config.auth_token = Some(s.clone());
                }

                if let Some(Expression::Literal(Literal::Integer(n))) =
                    server_map.get(&MapKey::Keyword(Keyword("timeout-seconds".to_string())))
                {
                    server_config.timeout_seconds = *n as u64;
                }

                if let Some(Expression::Literal(Literal::String(s))) =
                    server_map.get(&MapKey::Keyword(Keyword("protocol-version".to_string())))
                {
                    server_config.protocol_version = s.clone();
                }
            }

            // Extract capabilities array (can be List or Vector)
            let cap_list = map
                .get(&MapKey::Keyword(Keyword("capabilities".to_string())))
                .and_then(|expr| match expr {
                    Expression::List(list) => Some(list.as_slice()),
                    Expression::Vector(vec) => Some(vec.as_slice()),
                    _ => None,
                });

            if let Some(cap_list) = cap_list {
                for cap_expr in cap_list {
                    if let Expression::Map(cap_map) = cap_expr {
                        if let Some(capability_expr) =
                            cap_map.get(&MapKey::Keyword(Keyword("capability".to_string())))
                        {
                            let input_schema = cap_map
                                .get(&MapKey::Keyword(Keyword("input-schema".to_string())))
                                .and_then(|e| self.extract_type_expr(e));

                            let output_schema = cap_map
                                .get(&MapKey::Keyword(Keyword("output-schema".to_string())))
                                .and_then(|e| self.extract_type_expr(e));

                            capabilities.push(RTFSCapabilityDefinition {
                                capability: capability_expr.clone(),
                                input_schema,
                                output_schema,
                            });
                        }
                    }
                }
            }
        }

        Ok(RTFSModuleDefinition {
            module_type,
            server_config,
            capabilities,
            generated_at,
        })
    }

    /// Helper to extract TypeExpr from an Expression (if it's a type annotation)
    fn extract_type_expr(&self, expr: &Expression) -> Option<TypeExpr> {
        self.convert_rtfs_to_type_expr(expr).ok()
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
                        // Use increased indentation for nested structures
                        let value_str = self.expression_to_rtfs_text(value, indent + 1);
                        entries.push(format!("{} {}", key_str, value_str));
                    }
                    // Use multi-line format for maps to avoid very long lines that break the parser
                    // RTFS maps use whitespace separation, not commas
                    let entry_indent = "  ".repeat(indent + 1);
                    let entries_str: Vec<String> = entries
                        .iter()
                        .map(|e| format!("{}{}", entry_indent, e))
                        .collect();
                    let map_indent = "  ".repeat(indent);
                    format!("{{\n{}\n{}}}", entries_str.join("\n"), map_indent)
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

    /// Convert TypeExpr to RTFS text format
    fn type_expr_to_rtfs_text(&self, type_expr: &TypeExpr) -> String {
        match type_expr {
            TypeExpr::Primitive(p) => match p {
                PrimitiveType::String => ":string".to_string(),
                PrimitiveType::Int => ":int".to_string(),
                PrimitiveType::Float => ":float".to_string(),
                PrimitiveType::Bool => ":bool".to_string(),
                PrimitiveType::Nil => ":nil".to_string(),
                PrimitiveType::Keyword => ":keyword".to_string(),
                PrimitiveType::Symbol => ":symbol".to_string(),
                PrimitiveType::Custom(k) => format!(":{}", k.0),
            },
            TypeExpr::Any => ":any".to_string(),
            TypeExpr::Never => ":never".to_string(),
            TypeExpr::Vector(inner) => {
                format!("[:vector {}]", self.type_expr_to_rtfs_text(inner))
            }
            TypeExpr::Map { entries, .. } => {
                let mut map_entries = Vec::new();
                for entry in entries {
                    let key_str = format!(":{}", entry.key.0);
                    let value_str = self.type_expr_to_rtfs_text(&entry.value_type);
                    if entry.optional {
                        // RTFS optional map entry syntax: [key type?] (e.g., [:owner :string?])
                        map_entries.push(format!("[{} {}?]", key_str, value_str));
                    } else {
                        map_entries.push(format!("[{} {}]", key_str, value_str));
                    }
                }
                format!("[:map {}]", map_entries.join(" "))
            }
            TypeExpr::Union(options) => {
                let options_str: Vec<String> = options
                    .iter()
                    .map(|opt| self.type_expr_to_rtfs_text(opt))
                    .collect();
                format!("[:union {}]", options_str.join(" "))
            }
            TypeExpr::Optional(inner) => {
                // RTFS optional syntax: T? (e.g., :string?)
                format!("{}?", self.type_expr_to_rtfs_text(inner))
            }
            TypeExpr::Function { .. } => ":fn".to_string(), // Simplified representation
            TypeExpr::Literal(l) => match l {
                Literal::String(s) => format!("\"{}\"", s),
                Literal::Integer(i) => i.to_string(),
                Literal::Float(f) => f.to_string(),
                Literal::Boolean(b) => b.to_string(),
                _ => ":any".to_string(),
            },
            _ => ":any".to_string(), // Fallback for unsupported TypeExpr variants
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
        let mut implementation_expr: Option<Expression> = None;

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
                    match value {
                        Expression::Map(provider_map) => {
                            provider_info = Some(provider_map.clone());
                        }
                        Expression::Literal(Literal::String(provider_str)) => {
                            // String provider (for HTTP APIs) - create a synthetic provider map
                            // We'll handle this in the conversion step
                            let mut synthetic_map = std::collections::HashMap::new();
                            synthetic_map.insert(
                                MapKey::Keyword(Keyword("provider_type".to_string())),
                                Expression::Literal(Literal::String(provider_str.clone())),
                            );
                            provider_info = Some(synthetic_map);
                        }
                        _ => {
                            // Ignore other provider formats
                        }
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
                        // Extract nested metadata, especially openapi base_url
                        for (meta_key, meta_value) in meta_map {
                            match (meta_key, meta_value) {
                                (MapKey::Keyword(k), Expression::Literal(Literal::String(v))) => {
                                    if k.0 == "ccos_effects" {
                                        serialized_effects = Some(v.clone());
                                    }
                                    metadata.insert(k.0.clone(), v.clone());
                                }
                                (MapKey::Keyword(k), Expression::Map(nested_map))
                                    if k.0 == "openapi" =>
                                {
                                    // Extract base_url from nested openapi map
                                    for (nested_key, nested_value) in nested_map {
                                        if let (
                                            MapKey::Keyword(nk),
                                            Expression::Literal(Literal::String(nv)),
                                        ) = (nested_key, nested_value)
                                        {
                                            if nk.0 == "base_url" {
                                                metadata.insert(
                                                    "openapi_base_url".to_string(),
                                                    nv.clone(),
                                                );
                                                metadata.insert("base_url".to_string(), nv.clone());
                                            } else {
                                                metadata.insert(
                                                    format!("openapi_{}", nk.0),
                                                    nv.clone(),
                                                );
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                MapKey::Keyword(k) if k.0 == "implementation" => {
                    implementation_expr = Some(value.clone());
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
        // Check if OpenAPI metadata is present - if so, prefer OpenApiCapability
        // which properly handles query parameters and authentication
        let has_openapi_metadata = metadata.contains_key("openapi_base_url")
            || metadata.contains_key("openapi_endpoint_path")
            || metadata.contains_key("openapi_endpoint_method");

        let provider = if has_openapi_metadata {
            // Create OpenApiCapability from metadata - this properly handles query params
            let base_url = metadata
                .get("openapi_base_url")
                .or_else(|| metadata.get("base_url"))
                .cloned()
                .unwrap_or_else(|| "https://api.example.com".to_string());
            let endpoint_path = metadata
                .get("openapi_endpoint_path")
                .cloned()
                .unwrap_or_else(|| "/".to_string());
            let endpoint_method = metadata
                .get("openapi_endpoint_method")
                .cloned()
                .unwrap_or_else(|| "GET".to_string());

            // Create a single operation from the endpoint metadata
            let operation = crate::capability_marketplace::types::OpenApiOperation {
                operation_id: Some(name.clone()),
                method: endpoint_method.to_uppercase(),
                path: endpoint_path,
                summary: Some(description.clone()),
                description: None,
            };

            // Check for auth - OpenWeatherMap uses query param "appid"
            let auth = Some(crate::capability_marketplace::types::OpenApiAuth {
                auth_type: "apiKey".to_string(),
                location: "query".to_string(),
                parameter_name: "appid".to_string(),
                env_var_name: Some("OPENWEATHER_API_KEY".to_string()),
                required: true,
            });

            ProviderType::OpenApi(crate::capability_marketplace::types::OpenApiCapability {
                base_url,
                spec_url: None,
                operations: vec![operation],
                auth,
                timeout_ms: 30000,
            })
        } else if let Some(provider_map) = provider_info {
            // Check if this is a string provider (HTTP API) or MCP provider map
            if provider_map.len() == 1 {
                if let Some((
                    MapKey::Keyword(k),
                    Expression::Literal(Literal::String(_provider_type)),
                )) = provider_map.iter().next()
                {
                    if k.0 == "provider_type" {
                        // This is a string provider - create Http provider from metadata
                        let base_url = metadata
                            .get("base_url")
                            .cloned()
                            .unwrap_or_else(|| "https://api.example.com".to_string());

                        debug!(
                            "RTFS capability '{}' has provider_type string, defaulting to HTTP base_url={}",
                            name, base_url
                        );

                        ProviderType::Http(crate::capability_marketplace::types::HttpCapability {
                            base_url,
                            timeout_ms: 30000,
                            auth_token: None,
                        })
                    } else {
                        self.convert_rtfs_provider_to_manifest(&provider_map)?
                    }
                } else {
                    self.convert_rtfs_provider_to_manifest(&provider_map)?
                }
            } else {
                self.convert_rtfs_provider_to_manifest(&provider_map)?
            }
        } else if let Some(impl_expr) = implementation_expr.clone() {
            // Pure RTFS implementation -> run locally
            let impl_expr_cloned = impl_expr.clone();
            let rtfs_host_factory = std::sync::Arc::clone(&self.rtfs_host_factory);
            let capability_name = name.clone();
            ProviderType::Local(LocalCapability {
                handler: std::sync::Arc::new(move |inputs: &rtfs::runtime::values::Value| {
                    // Build a tiny RTFS evaluator with stdlib and a bound `input`
                    let module_registry = std::sync::Arc::new(ModuleRegistry::new());
                    let _ = rtfs::runtime::stdlib::load_stdlib(&module_registry);

                    let mut env = Environment::new();
                    if let Some(stdlib) = module_registry.get_module("stdlib") {
                        if let Ok(exports) = stdlib.exports.read() {
                            for (name, export) in exports.iter() {
                                env.define(&Symbol(name.clone()), export.value.clone());
                            }
                        }
                    }
                    env.define(&Symbol("input".to_string()), inputs.clone());

                    let host = (rtfs_host_factory)();
                    // Ensure the host has a minimal execution context to avoid fatal errors
                    let plan_id = format!("rtfs-{}", uuid::Uuid::new_v4());
                    let intent_id = capability_name.clone();
                    host.set_execution_context(
                        plan_id,
                        vec![intent_id],
                        "rtfs-standalone".to_string(),
                    );
                    let evaluator = Evaluator::with_environment(
                        module_registry.clone(),
                        env,
                        RuntimeContext::pure(),
                        host,
                    );
                    let mut runtime = Runtime::new(Box::new(TreeWalkingStrategy::new(evaluator)));

                    let call_expr = Expression::FunctionCall {
                        callee: Box::new(impl_expr_cloned.clone()),
                        arguments: vec![Expression::Symbol(Symbol("input".to_string()))],
                    };

                    runtime.run(&call_expr)
                }),
            })
        } else {
            // No provider info and no implementation - default to pure local no-op
            debug!(
                "RTFS capability '{}' missing provider and implementation; defaulting to local noop",
                name
            );
            ProviderType::Local(LocalCapability {
                handler: std::sync::Arc::new(|_inputs: &rtfs::runtime::values::Value| {
                    Ok(rtfs::runtime::values::Value::Nil)
                }),
            })
        };

        // Convert input/output schemas
        let mut input_schema = rtfs_def.input_schema.clone();
        let output_schema = rtfs_def.output_schema.clone();

        if name == "list_issues" {
            input_schema = self.patch_list_issues_schema(input_schema);
        }

        if effects.is_empty() {
            if let Some(serialized) = serialized_effects {
                effects = Self::parse_effects_from_serialized(&serialized);
            }
        }

        if effects.is_empty() {
            effects = match provider {
                ProviderType::Local(_) => vec![":pure".to_string()],
                _ => vec![":network".to_string()],
            };
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
            provenance: Some(crate::capability_marketplace::types::CapabilityProvenance {
                source: "rtfs_persistence".to_string(),
                version: Some("1.0.0".to_string()),
                content_hash: format!("rtfs_{}", name),
                custody_chain: vec!["rtfs_persistence".to_string()],
                registered_at: Utc::now(),
            }),
            permissions,
            effects,
            metadata,
            agent_metadata: None,
            domains: Vec::new(),
            categories: Vec::new(),
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
            auth_token: None,
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
            auth_token: None,
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

    fn convert_string_to_type_expr(&self, s: &str) -> RuntimeResult<TypeExpr> {
        if s.ends_with('?') {
            let base = &s[..s.len() - 1];
            let base_type = self.convert_string_to_type_expr(base)?;
            return Ok(TypeExpr::Optional(Box::new(base_type)));
        }
        match s {
            "string" => Ok(TypeExpr::Primitive(PrimitiveType::String)),
            "integer" | "int" => Ok(TypeExpr::Primitive(PrimitiveType::Int)),
            "number" | "float" => Ok(TypeExpr::Primitive(PrimitiveType::Float)),
            "boolean" | "bool" => Ok(TypeExpr::Primitive(PrimitiveType::Bool)),
            "any" => Ok(TypeExpr::Any),
            _ => Ok(TypeExpr::Primitive(PrimitiveType::Custom(Keyword(
                s.to_string(),
            )))),
        }
    }

    /// Convert RTFS Expression back to TypeExpr for capability manifests
    fn convert_rtfs_to_type_expr(&self, expr: &Expression) -> RuntimeResult<TypeExpr> {
        match expr {
            Expression::Symbol(symbol) => self.convert_string_to_type_expr(symbol.0.as_str()),
            Expression::Literal(Literal::Keyword(k)) => {
                self.convert_string_to_type_expr(k.0.as_str())
            }
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
                // Check for special type forms like [:enum ...] or [:optional ...]
                if let Some(Expression::Literal(Literal::Keyword(kw))) = vec.first() {
                    match kw.0.as_str() {
                        "map" => {
                            let mut entries = Vec::new();
                            for entry_expr in vec.iter().skip(1) {
                                if let Expression::Vector(entry_vec) = entry_expr {
                                    if entry_vec.len() >= 2 {
                                        if let Expression::Literal(Literal::Keyword(key_kw)) =
                                            &entry_vec[0]
                                        {
                                            let value_type =
                                                self.convert_rtfs_to_type_expr(&entry_vec[1])?;
                                            // Treat as optional entry if the value type is optional
                                            let is_optional =
                                                matches!(value_type, TypeExpr::Optional(_));

                                            entries.push(MapTypeEntry {
                                                key: Keyword(key_kw.0.clone()),
                                                value_type: Box::new(value_type),
                                                optional: is_optional,
                                            });
                                        }
                                    }
                                }
                            }
                            return Ok(TypeExpr::Map {
                                entries,
                                wildcard: None,
                            });
                        }
                        "enum" => {
                            let variants = vec
                                .iter()
                                .skip(1)
                                .filter_map(|e| match e {
                                    Expression::Literal(lit) => Some(lit.clone()),
                                    _ => None,
                                })
                                .collect();
                            return Ok(TypeExpr::Enum(variants));
                        }
                        "optional" => {
                            if vec.len() >= 2 {
                                let inner_type = self.convert_rtfs_to_type_expr(&vec[1])?;
                                return Ok(TypeExpr::Optional(Box::new(inner_type)));
                            }
                        }
                        _ => {}
                    }
                }

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
                auth_token: None,
            }),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None, // MCP resources don't have structured schemas like tools
            attestation: None,
            provenance: Some(crate::capability_marketplace::types::CapabilityProvenance {
                source: "mcp_discovery".to_string(),
                version: Some("1.0.0".to_string()),
                content_hash: format!("mcp_resource_{}_{}", self.config.name, resource_name),
                custody_chain: vec!["mcp_discovery".to_string()],
                registered_at: Utc::now(),
            }),
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
            domains: Vec::new(),
            categories: Vec::new(),
        })
    }

    /// Health check for the MCP server
    pub async fn health_check(&self) -> RuntimeResult<bool> {
        let health_url = format!("{}/health", self.config.endpoint);
        let client = reqwest::Client::new();

        let request = client.get(&health_url).build().map_err(|e| {
            RuntimeError::Generic(format!("Failed to build MCP health check request: {}", e))
        })?;

        match timeout(
            Duration::from_secs(5), // Shorter timeout for health checks
            client.execute(request),
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
