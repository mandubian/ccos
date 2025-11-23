//! MCP Tool Introspection and RTFS Capability Synthesis
//!
//! This module discovers MCP tools from MCP servers and generates RTFS-first capabilities
//! with proper schema encoding, following the same pattern as api_introspector.rs.
//!
//! The generated capabilities are simple RTFS wrappers that:
//! 1. Validate inputs against schemas (runtime-controlled)
//! 2. Build MCP JSON-RPC requests
//! 3. Call the MCP server via ccos.network.http-fetch
//! 4. Parse and validate the response
//!
//! This makes MCP tools trivially callable from RTFS/CCOS plans alongside OpenAPI capabilities.
//!
//! ## Session Management
//!
//! This module properly implements MCP session management according to the specification:
//! https://modelcontextprotocol.io/specification/2025-03-26/basic/transports#session-management
//!
//! The flow is:
//! 1. Initialize session → Get Mcp-Session-Id
//! 2. Call tools/list with session ID
//! 3. Terminate session when done

use crate::capability_marketplace::types::CapabilityManifest;
use crate::synthesis::mcp_session::{MCPServerInfo, MCPSessionManager};
use crate::synthesis::schema_serializer::type_expr_to_rtfs_compact;
use rtfs::ast::{Keyword, MapTypeEntry, TypeExpr};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// MCP server introspection result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPIntrospectionResult {
    pub server_url: String,
    pub server_name: String,
    pub protocol_version: String,
    pub tools: Vec<DiscoveredMCPTool>,
}

/// A discovered MCP tool with its schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredMCPTool {
    pub tool_name: String,
    pub description: Option<String>,
    pub input_schema: Option<TypeExpr>,
    pub output_schema: Option<TypeExpr>,
    pub input_schema_json: Option<serde_json::Value>,
}

/// MCP introspector for discovering tools and generating RTFS capabilities
pub struct MCPIntrospector {
    mock_mode: bool,
}

impl MCPIntrospector {
    /// Create a new MCP introspector
    pub fn new() -> Self {
        Self { mock_mode: false }
    }

    /// Create a mock MCP introspector for testing
    pub fn mock() -> Self {
        Self { mock_mode: true }
    }

    /// Introspect an MCP server and discover its tools
    pub async fn introspect_mcp_server(
        &self,
        server_url: &str,
        server_name: &str,
    ) -> RuntimeResult<MCPIntrospectionResult> {
        self.introspect_mcp_server_with_auth(server_url, server_name, None)
            .await
    }

    /// Introspect an MCP server with authentication headers
    ///
    /// This properly implements MCP session management:
    /// 1. Initialize session (get Mcp-Session-Id)
    /// 2. Call tools/list with session ID
    /// 3. Terminate session when done
    pub async fn introspect_mcp_server_with_auth(
        &self,
        server_url: &str,
        server_name: &str,
        auth_headers: Option<HashMap<String, String>>,
    ) -> RuntimeResult<MCPIntrospectionResult> {
        if self.mock_mode {
            return self.introspect_mock_mcp_server(server_name);
        }

        println!(
            "🔍 Introspecting MCP server: {} ({})",
            server_name, server_url
        );

        // Create session manager with authentication
        let session_manager = MCPSessionManager::new(auth_headers);

        // Step 1: Initialize MCP session
        let client_info = MCPServerInfo {
            name: "ccos-introspector".to_string(),
            version: "1.0.0".to_string(),
        };

        let session = session_manager
            .initialize_session(server_url, &client_info)
            .await?;

        // Step 2: Call tools/list with session
        let tools_response = session_manager
            .make_request(&session, "tools/list", serde_json::json!({}))
            .await;

        // Step 3: Terminate session (even if tools/list failed)
        let _ = session_manager.terminate_session(&session).await;

        // Check tools/list result
        let mcp_response = tools_response?;

        // Extract tools from response
        let tools_array = mcp_response
            .get("result")
            .and_then(|r| r.get("tools"))
            .and_then(|t| t.as_array())
            .ok_or_else(|| RuntimeError::Generic("Invalid MCP tools/list response".to_string()))?;

        let mut discovered_tools = Vec::new();
        for tool_json in tools_array {
            if let Ok(tool) = self.parse_mcp_tool(tool_json) {
                discovered_tools.push(tool);
            }
        }

        Ok(MCPIntrospectionResult {
            server_url: server_url.to_string(),
            server_name: server_name.to_string(),
            protocol_version: session.protocol_version.clone(),
            tools: discovered_tools,
        })
    }

    /// Parse an MCP tool from JSON-RPC response
    fn parse_mcp_tool(&self, tool_json: &serde_json::Value) -> RuntimeResult<DiscoveredMCPTool> {
        let tool_name = tool_json
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or_else(|| RuntimeError::Generic("MCP tool missing name".to_string()))?
            .to_string();

        let description = tool_json
            .get("description")
            .and_then(|d| d.as_str())
            .map(|s| s.to_string());

        // Extract input schema from MCP tool's inputSchema field
        let (input_schema, input_schema_json) = if let Some(schema) = tool_json.get("inputSchema") {
            let type_expr = self.json_schema_to_rtfs_type(schema)?;
            (Some(type_expr), Some(schema.clone()))
        } else {
            (None, None)
        };

        // MCP tools typically don't declare output schemas explicitly
        // We can infer basic structure or leave as :any
        // TODO: Optionally call the tool once with safe inputs to infer output schema
        let output_schema = None;

        Ok(DiscoveredMCPTool {
            tool_name,
            description,
            input_schema,
            output_schema,
            input_schema_json,
        })
    }

    /// Convert JSON Schema to RTFS TypeExpr (reuses logic from api_introspector)
    /// Convert JSON Schema to RTFS TypeExpr (reuses logic from api_introspector)
    fn json_schema_to_rtfs_type(&self, schema: &serde_json::Value) -> RuntimeResult<TypeExpr> {
        // Handle "type": ["string", "null"] or "nullable": true
        let is_nullable = schema
            .get("nullable")
            .and_then(|n| n.as_bool())
            .unwrap_or(false);

        let type_val = schema.get("type");

        let (base_type_str, is_nullable_type) = if let Some(t) = type_val.and_then(|t| t.as_str()) {
            (Some(t), false)
        } else if let Some(arr) = type_val.and_then(|t| t.as_array()) {
            // Check if it contains "null"
            let has_null = arr.iter().any(|v| v.as_str() == Some("null"));
            // Find the first non-null type
            let type_str = arr.iter().find_map(|v| {
                let s = v.as_str()?;
                if s != "null" {
                    Some(s)
                } else {
                    None
                }
            });
            (type_str, has_null)
        } else {
            (None, false)
        };

        let effective_nullable = is_nullable || is_nullable_type;

        let base_type = if let Some(type_str) = base_type_str {
            match type_str {
                "string" => TypeExpr::Primitive(rtfs::ast::PrimitiveType::String),
                "integer" | "number" => {
                    if schema.get("format").and_then(|f| f.as_str()) == Some("integer") {
                        TypeExpr::Primitive(rtfs::ast::PrimitiveType::Int)
                    } else {
                        TypeExpr::Primitive(rtfs::ast::PrimitiveType::Float)
                    }
                }
                "boolean" => TypeExpr::Primitive(rtfs::ast::PrimitiveType::Bool),
                "array" => {
                    let element_type = if let Some(items) = schema.get("items") {
                        self.json_schema_to_rtfs_type(items)?
                    } else {
                        TypeExpr::Any
                    };
                    TypeExpr::Vector(Box::new(element_type))
                }
                "object" => {
                    let mut entries = Vec::new();
                    if let Some(properties) = schema.get("properties").and_then(|p| p.as_object()) {
                        let required_fields: Vec<String> = schema
                            .get("required")
                            .and_then(|r| r.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|s| s.as_str())
                                    .map(|s| s.to_string())
                                    .collect()
                            })
                            .unwrap_or_default();

                        for (key, prop_schema) in properties {
                            let prop_type = self.json_schema_to_rtfs_type(prop_schema)?;

                            // Check if prop_type is optional/nullable
                            let is_prop_nullable = match &prop_type {
                                TypeExpr::Optional(_) => true,
                                TypeExpr::Union(types) => types.iter().any(|t| {
                                    matches!(t, TypeExpr::Primitive(rtfs::ast::PrimitiveType::Nil))
                                }),
                                TypeExpr::Primitive(rtfs::ast::PrimitiveType::Nil) => true,
                                _ => false,
                            };

                            entries.push(MapTypeEntry {
                                key: Keyword(key.clone()),
                                value_type: Box::new(prop_type),
                                optional: !required_fields.contains(key) || is_prop_nullable,
                            });
                        }
                    }
                    TypeExpr::Map {
                        entries,
                        wildcard: None,
                    }
                }
                _ => TypeExpr::Any,
            }
        } else {
            TypeExpr::Any
        };

        if effective_nullable
            && !matches!(base_type, TypeExpr::Any)
            && !matches!(base_type, TypeExpr::Optional(_))
        {
            Ok(TypeExpr::Optional(Box::new(base_type)))
        } else {
            Ok(base_type)
        }
    }

    /// Helper to convert JSON Schema directly to a `TypeExpr` while preserving optionality
    pub fn type_expr_from_json_schema(schema: &serde_json::Value) -> RuntimeResult<TypeExpr> {
        let introspector = MCPIntrospector::new();
        introspector.json_schema_to_rtfs_type(schema)
    }

    /// Create RTFS capabilities from MCP introspection
    pub fn create_capabilities_from_mcp(
        &self,
        introspection: &MCPIntrospectionResult,
    ) -> RuntimeResult<Vec<CapabilityManifest>> {
        let mut capabilities = Vec::new();

        for tool in &introspection.tools {
            let capability = self.create_capability_from_mcp_tool(tool, introspection)?;
            capabilities.push(capability);
        }

        Ok(capabilities)
    }

    /// Introspect output schema by calling the MCP tool once with safe/dummy inputs
    /// Returns (inferred_schema, pretty_sample_json)
    pub async fn introspect_output_schema(
        &self,
        tool: &DiscoveredMCPTool,
        server_url: &str,
        server_name: &str,
        auth_headers: Option<HashMap<String, String>>,
        input_overrides: Option<serde_json::Value>,
    ) -> RuntimeResult<(Option<TypeExpr>, Option<String>)> {
        // Only introspect if we have auth (indicates we're authorized to make calls)
        if auth_headers.is_none() {
            return Ok((None, None));
        }

        // Only introspect for read-only operations (safe to call)
        // Skip for operations that might modify data (create, update, delete, etc.)
        let tool_name_lower = tool.tool_name.to_lowercase();
        let unsafe_verbs = [
            "create", "update", "delete", "remove", "add", "modify", "write", "post", "put",
            "patch",
        ];
        if unsafe_verbs
            .iter()
            .any(|verb| tool_name_lower.contains(verb))
        {
            eprintln!(
                "⚠️ Skipping output schema introspection for '{}' (potentially unsafe operation)",
                tool.tool_name
            );
            return Ok((None, None));
        }

        eprintln!(
            "🔍 Introspecting output schema for '{}' by calling it once with safe inputs",
            tool.tool_name
        );

        // Generate safe test inputs from input schema
        let mut test_inputs = self.generate_safe_test_inputs(tool, false, input_overrides.clone())?;

        // Create session manager
        let session_manager = MCPSessionManager::new(auth_headers);
        let client_info = MCPServerInfo {
            name: "ccos-introspector".to_string(),
            version: "1.0.0".to_string(),
        };

        // Initialize session
        let session = match session_manager
            .initialize_session(server_url, &client_info)
            .await
        {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "⚠️ Failed to initialize session for output schema introspection: {}",
                    e
                );
                return Ok((None, None));
            }
        };

        // Call the tool with test inputs
        let response = match session_manager
            .make_request(
                &session,
                "tools/call",
                serde_json::json!({
                    "name": tool.tool_name,
                    "arguments": test_inputs
                }),
            )
            .await
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!(
                    "⚠️ Failed to call tool for output schema introspection: {}",
                    e
                );
                let _ = session_manager.terminate_session(&session).await;
                return Ok((None, None));
            }
        };

        // Terminate session
        // let _ = session_manager.terminate_session(&session).await;

        // Extract result from response
        if let Some(result) = response.get("result") {
            let mut sample_snippet = serde_json::to_string_pretty(result)
                .ok()
                .filter(|s| !s.is_empty());
            let error_detected = sample_snippet
                .as_ref()
                .map(|s| {
                    let s = s.to_lowercase();
                    s.contains("missing required")
                        || s.contains("required parameter")
                        || s.contains("error")
                        || s.contains("unauthorized")
                        || s.contains("forbidden")
                })
                .unwrap_or(false);

            if error_detected {
                if let Some(snippet) = &sample_snippet {
                    eprintln!(
                        "⚠️ Introspection encountered an error for '{}': {}",
                        tool.tool_name, snippet
                    );
                } else {
                    eprintln!(
                        "⚠️ Introspection returned an error-like response for '{}', but no snippet was captured",
                        tool.tool_name
                    );
                }

                eprintln!(
                    "⚠️ Introspection result appears invalid for '{}', retrying with plausible inputs...",
                    tool.tool_name
                );

                test_inputs = self.generate_safe_test_inputs(tool, true, input_overrides)?;
                let retry_response = match session_manager
                    .make_request(
                        &session,
                        "tools/call",
                        serde_json::json!({
                            "name": tool.tool_name,
                            "arguments": test_inputs
                        }),
                    )
                    .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!(
                            "⚠️ Retry for output schema introspection failed for '{}': {}",
                            tool.tool_name, e
                        );
                        let _ = session_manager.terminate_session(&session).await;
                        return Ok((None, sample_snippet));
                    }
                };

                // Terminate session after retry
                let _ = session_manager.terminate_session(&session).await;

                if let Some(result2) = retry_response.get("result") {
                    let output_schema = self.infer_type_from_json_value(result2)?;
                    sample_snippet = serde_json::to_string_pretty(result2)
                        .ok()
                        .filter(|s| !s.is_empty());
                    eprintln!(
                        "✅ Inferred output schema + sample (retry) for '{}': schema={}, sample lines={}",
                        tool.tool_name,
                        type_expr_to_rtfs_compact(&output_schema),
                        sample_snippet
                            .as_ref()
                            .map(|s| s.lines().count())
                            .unwrap_or(0)
                    );
                    return Ok((Some(output_schema), sample_snippet));
                }

                eprintln!(
                    "⚠️ Retry did not yield a valid result for '{}'; storing error snippet",
                    tool.tool_name
                );
                return Ok((None, sample_snippet));
            }

            let output_schema = self.infer_type_from_json_value(result)?;
            eprintln!(
                "✅ Inferred output schema + sample for '{}': schema={}, sample lines={}",
                tool.tool_name,
                type_expr_to_rtfs_compact(&output_schema),
                sample_snippet
                    .as_ref()
                    .map(|s| s.lines().count())
                    .unwrap_or(0)
            );
            // Terminate session if no error was detected
            let _ = session_manager.terminate_session(&session).await;
            Ok((Some(output_schema), sample_snippet))
        } else {
            eprintln!("⚠️ No result in response for output schema introspection");
            let _ = session_manager.terminate_session(&session).await;
            Ok((None, None))
        }
    }

    /// Generate safe test inputs from input schema
    fn generate_safe_test_inputs(
        &self,
        tool: &DiscoveredMCPTool,
        plausible: bool,
        overrides: Option<serde_json::Value>,
    ) -> RuntimeResult<serde_json::Value> {
        let mut inputs = serde_json::Map::new();

        if let Some(input_schema_json) = &tool.input_schema_json {
            if let Some(properties) = input_schema_json
                .get("properties")
                .and_then(|p| p.as_object())
            {
                let required: Vec<String> = input_schema_json
                    .get("required")
                    .and_then(|r| r.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|s| s.as_str())
                            .map(|s| s.to_string())
                            .collect()
                    })
                    .unwrap_or_default();

                for (key, prop_schema) in properties {
                    // Only include required fields or fields with safe defaults
                    if required.contains(key) {
                        let default_value =
                            self.generate_safe_default_value(prop_schema, key, plausible);
                        inputs.insert(key.clone(), default_value);
                    }
                }
            }
        }

        if let Some(serde_json::Value::Object(override_map)) = overrides {
            for (k, v) in override_map {
                inputs.insert(k, v);
            }
        }

        Ok(serde_json::Value::Object(inputs))
    }

    /// Generate a safe default value for a JSON schema property
    fn generate_safe_default_value(
        &self,
        schema: &serde_json::Value,
        name: &str,
        plausible: bool,
    ) -> serde_json::Value {
        if let Some(type_str) = schema.get("type").and_then(|t| t.as_str()) {
            match type_str {
                "string" => {
                    // Use empty string or a safe default from enum if available
                    if let Some(enum_vals) = schema.get("enum").and_then(|e| e.as_array()) {
                        if let Some(first) = enum_vals.first().and_then(|v| v.as_str()) {
                            return serde_json::Value::String(first.to_string());
                        }
                    }
                    // For string fields, use empty string or a safe placeholder
                    let name_l = name.to_lowercase();
                    if plausible {
                        if name_l.contains("owner") || name_l.contains("user") {
                            return serde_json::Value::String("octocat".to_string());
                        }
                        if name_l.contains("repo") || name_l.contains("repository") {
                            return serde_json::Value::String("hello-world".to_string());
                        }
                        if name_l.contains("sha") || name_l.contains("commit") {
                            return serde_json::Value::String(
                                "0000000000000000000000000000000000000000".to_string(),
                            );
                        }
                        if name_l.contains("email") {
                            return serde_json::Value::String("example@example.com".to_string());
                        }
                        if name_l.contains("url") || name_l.contains("uri") {
                            return serde_json::Value::String("https://example.com".to_string());
                        }
                        if name_l.contains("name")
                            || name_l.contains("title")
                            || name_l.contains("label")
                        {
                            return serde_json::Value::String("example".to_string());
                        }
                        if name_l.contains("path") {
                            return serde_json::Value::String("/path/to/file".to_string());
                        }
                    }
                    serde_json::Value::String("".to_string())
                }
                "integer" | "number" => {
                    // Use 0 or minimum value if specified
                    if let Some(min) = schema.get("minimum").and_then(|m| m.as_i64()) {
                        serde_json::Value::Number(serde_json::Number::from(min))
                    } else {
                        serde_json::Value::Number(serde_json::Number::from(0))
                    }
                }
                "boolean" => serde_json::Value::Bool(false),
                "array" => serde_json::Value::Array(vec![]),
                "object" => serde_json::Value::Object(serde_json::Map::new()),
                _ => serde_json::Value::Null,
            }
        } else {
            serde_json::Value::Null
        }
    }

    /// Infer RTFS TypeExpr from a JSON value by analyzing its structure
    pub fn infer_type_from_json_value(&self, value: &serde_json::Value) -> RuntimeResult<TypeExpr> {
        match value {
            serde_json::Value::Null => Ok(TypeExpr::Primitive(rtfs::ast::PrimitiveType::Nil)),
            serde_json::Value::Bool(_) => Ok(TypeExpr::Primitive(rtfs::ast::PrimitiveType::Bool)),
            serde_json::Value::Number(n) => {
                if n.is_i64() {
                    Ok(TypeExpr::Primitive(rtfs::ast::PrimitiveType::Int))
                } else {
                    Ok(TypeExpr::Primitive(rtfs::ast::PrimitiveType::Float))
                }
            }
            serde_json::Value::String(_) => {
                Ok(TypeExpr::Primitive(rtfs::ast::PrimitiveType::String))
            }
            serde_json::Value::Array(arr) => {
                if arr.is_empty() {
                    // Empty array - can't infer element type, use :any
                    Ok(TypeExpr::Vector(Box::new(TypeExpr::Any)))
                } else {
                    // Infer element type from first element (or union if different types)
                    let first_type = self.infer_type_from_json_value(&arr[0])?;
                    // Check if all elements have the same type
                    let all_same = arr.iter().all(|item| {
                        self.infer_type_from_json_value(item)
                            .map(|t| t == first_type)
                            .unwrap_or(false)
                    });
                    if all_same {
                        Ok(TypeExpr::Vector(Box::new(first_type)))
                    } else {
                        // Mixed types - use :any for element type
                        Ok(TypeExpr::Vector(Box::new(TypeExpr::Any)))
                    }
                }
            }
            serde_json::Value::Object(obj) => {
                let mut entries = Vec::new();
                for (key, val) in obj {
                    let value_type = self.infer_type_from_json_value(val)?;
                    entries.push(MapTypeEntry {
                        key: Keyword(key.clone()),
                        value_type: Box::new(value_type),
                        optional: false, // Can't determine optionality from a single sample
                    });
                }
                Ok(TypeExpr::Map {
                    entries,
                    wildcard: None,
                })
            }
        }
    }

    /// Create a single capability from an MCP tool
    pub fn create_capability_from_mcp_tool(
        &self,
        tool: &DiscoveredMCPTool,
        introspection: &MCPIntrospectionResult,
    ) -> RuntimeResult<CapabilityManifest> {
        let capability_id = format!(
            "mcp.{}.{}",
            introspection
                .server_name
                .replace("/", ".")
                .replace(" ", "_"),
            tool.tool_name.replace("-", "_")
        );

        let effects = vec!["network_request".to_string(), "mcp_call".to_string()];

        let mut metadata = HashMap::new();
        metadata.insert(
            "mcp_server_url".to_string(),
            introspection.server_url.clone(),
        );
        metadata.insert(
            "mcp_server_name".to_string(),
            introspection.server_name.clone(),
        );
        metadata.insert("mcp_tool_name".to_string(), tool.tool_name.clone());
        metadata.insert(
            "mcp_protocol_version".to_string(),
            introspection.protocol_version.clone(),
        );
        metadata.insert(
            "discovery_method".to_string(),
            "mcp_introspection".to_string(),
        );

        // MCP session management hints (generic, not server-specific)
        metadata.insert("mcp_requires_session".to_string(), "auto".to_string()); // auto, true, false
        metadata.insert("mcp_auth_env_var".to_string(), "MCP_AUTH_TOKEN".to_string()); // generic env var name
        metadata.insert(
            "mcp_server_url_override_env".to_string(),
            "MCP_SERVER_URL".to_string(),
        );

        if let Some(input_json) = &tool.input_schema_json {
            metadata.insert("mcp_input_schema_json".to_string(), input_json.to_string());
        }

        Ok(CapabilityManifest {
            id: capability_id,
            name: tool.tool_name.clone(),
            description: tool
                .description
                .clone()
                .unwrap_or_else(|| format!("MCP tool: {}", tool.tool_name)),
            provider: crate::capability_marketplace::types::ProviderType::MCP(
                crate::capability_marketplace::types::MCPCapability {
                    server_url: introspection.server_url.clone(),
                    tool_name: tool.tool_name.clone(),
                    timeout_ms: 30000,
                    auth_token: None,
                },
            ),
            version: "1.0.0".to_string(),
            input_schema: tool.input_schema.clone(),
            output_schema: tool.output_schema.clone(),
            attestation: None,
            provenance: Some(crate::capability_marketplace::types::CapabilityProvenance {
                source: "mcp_introspector".to_string(),
                version: Some("1.0.0".to_string()),
                content_hash: format!("mcp_{}_{}", introspection.server_name, tool.tool_name),
                custody_chain: vec!["mcp_introspector".to_string()],
                registered_at: chrono::Utc::now(),
            }),
            permissions: vec!["network.http".to_string()],
            effects,
            metadata,
            agent_metadata: None,
        })
    }

    /// Generate RTFS implementation for MCP tool
    ///
    /// Since we use native MCP provider, the RTFS implementation is nil.
    /// The runtime handles execution via MCPExecutor.
    pub fn generate_mcp_rtfs_implementation(
        &self,
        _tool: &DiscoveredMCPTool,
        _server_url: &str,
    ) -> String {
        "nil".to_string()
    }

    /// Serialize MCP capability to RTFS format
    pub fn capability_to_rtfs_string(
        &self,
        capability: &CapabilityManifest,
        implementation_code: &str,
        sample_output: Option<&str>,
    ) -> String {
        let sample_comment = if let Some(sample) = sample_output {
            let indented_sample = sample
                .lines()
                .map(|line| format!(";; {}", line))
                .collect::<Vec<_>>()
                .join("\n");
            format!(";; Sample Output:\n{}\n\n", indented_sample)
        } else {
            String::new()
        };

        let input_schema_str = capability
            .input_schema
            .as_ref()
            .map(type_expr_to_rtfs_compact)
            .unwrap_or_else(|| ":any".to_string());

        let output_schema_str = capability
            .output_schema
            .as_ref()
            .map(type_expr_to_rtfs_compact)
            .unwrap_or_else(|| ":any".to_string());

        let permissions_str = if capability.permissions.is_empty() {
            "[]".to_string()
        } else {
            format!(
                "[{}]",
                capability
                    .permissions
                    .iter()
                    .map(|p| format!(":{}", p))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        };

        let effects_str = if capability.effects.is_empty() {
            "[]".to_string()
        } else {
            format!(
                "[{}]",
                capability
                    .effects
                    .iter()
                    .map(|e| format!(":{}", e))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        };

        let mcp_server_url = capability
            .metadata
            .get("mcp_server_url")
            .map(|s| s.as_str())
            .unwrap_or("");
        let mcp_tool_name = capability
            .metadata
            .get("mcp_tool_name")
            .map(|s| s.as_str())
            .unwrap_or("");
        let mcp_server_name = capability
            .metadata
            .get("mcp_server_name")
            .map(|s| s.as_str())
            .unwrap_or("");

        let provider_str = match &capability.provider {
            crate::capability_marketplace::types::ProviderType::MCP(mcp) => format!(
                r#"{{
    :type "mcp"
    :server_endpoint "{}"
    :tool_name "{}"
    :timeout_seconds {}
    :protocol_version "{}"
  }}"#,
                mcp.server_url,
                mcp.tool_name,
                mcp.timeout_ms / 1000,
                capability.metadata.get("mcp_protocol_version").map(|s| s.as_str()).unwrap_or("2024-11-05")
            ),
            _ => "\"MCP\"".to_string(),
        };

        format!(
            r#";; MCP Capability: {}
;; Generated from MCP tool introspection
;; MCP Server: {} ({})
;; Tool: {}

{}
(capability "{}"
  :name "{}"
  :version "{}"
  :description "{}"
  ;; PROVIDER START
  :provider {}
  ;; PROVIDER END
  :permissions {}
  :effects {}
  :metadata {{
    :mcp {{
      :server_url "{}"
      :server_name "{}"
      :tool_name "{}"
      :protocol_version "{}"
      :requires_session "auto"
      :auth_env_var "MCP_AUTH_TOKEN"
      :server_url_override_env "MCP_SERVER_URL"
    }}
    :discovery {{
      :method "mcp_introspection"
      :source_url "{}"
      :created_at "{}"
      :capability_type "mcp_tool"
    }}
  }}
  :input-schema {}
  :output-schema {}
  :implementation
    {}
)
"#,
            capability.name,
            mcp_server_name,
            mcp_server_url,
            mcp_tool_name,
            sample_comment,
            capability.id,
            capability.name,
            capability.version,
            capability.description,
            provider_str,
            permissions_str,
            effects_str,
            mcp_server_url,
            mcp_server_name,
            mcp_tool_name,
            capability
                .metadata
                .get("mcp_protocol_version")
                .map(|s| s.as_str())
                .unwrap_or("2024-11-05"),
            mcp_server_url,
            chrono::Utc::now().to_rfc3339(),
            input_schema_str,
            output_schema_str,
            implementation_code
        )
    }

    /// Save MCP capability to RTFS file
    ///
    /// Uses hierarchical directory structure:
    /// output_dir/mcp/<namespace>/<tool_name>.rtfs
    ///
    /// Example: capabilities/mcp/github/list_issues.rtfs
    pub fn save_capability_to_rtfs(
        &self,
        capability: &CapabilityManifest,
        implementation_code: &str,
        output_dir: &std::path::Path,
        sample_output: Option<&str>,
    ) -> RuntimeResult<std::path::PathBuf> {
        // Parse capability ID: "mcp.namespace.tool_name"
        let parts: Vec<&str> = capability.id.split('.').collect();
        if parts.len() < 3 {
            return Err(RuntimeError::Generic(format!(
                "Invalid capability ID format: {}. Expected: mcp.<namespace>.<tool>",
                capability.id
            )));
        }

        let provider_type = parts[0]; // "mcp"
        let namespace = parts[1]; // "github"
        let tool_name = parts[2..].join("_"); // "list_issues" or "add_comment_to_pending_review"

        // Create directory: output_dir/mcp/<namespace>/
        let capability_dir = output_dir.join(provider_type).join(namespace);
        std::fs::create_dir_all(&capability_dir).map_err(|e| {
            RuntimeError::Generic(format!("Failed to create capability directory: {}", e))
        })?;

        let rtfs_content =
            self.capability_to_rtfs_string(capability, implementation_code, sample_output);
        let rtfs_file = capability_dir.join(format!("{}.rtfs", tool_name));

        std::fs::write(&rtfs_file, rtfs_content)
            .map_err(|e| RuntimeError::Generic(format!("Failed to write RTFS file: {}", e)))?;

        Ok(rtfs_file)
    }

    /// Mock MCP server introspection for testing
    fn introspect_mock_mcp_server(
        &self,
        server_name: &str,
    ) -> RuntimeResult<MCPIntrospectionResult> {
        // Special handling for GitHub MCP
        if server_name.contains("github") {
            return self.introspect_mock_github_mcp();
        }

        // Default mock MCP server
        Ok(MCPIntrospectionResult {
            server_url: format!("http://localhost:3000/{}", server_name),
            server_name: server_name.to_string(),
            protocol_version: "2024-11-05".to_string(),
            tools: vec![DiscoveredMCPTool {
                tool_name: "example_tool".to_string(),
                description: Some("Example MCP tool".to_string()),
                input_schema: Some(TypeExpr::Map {
                    entries: vec![MapTypeEntry {
                        key: Keyword("query".to_string()),
                        value_type: Box::new(TypeExpr::Primitive(rtfs::ast::PrimitiveType::String)),
                        optional: false,
                    }],
                    wildcard: None,
                }),
                output_schema: Some(TypeExpr::Map {
                    entries: vec![MapTypeEntry {
                        key: Keyword("result".to_string()),
                        value_type: Box::new(TypeExpr::Primitive(rtfs::ast::PrimitiveType::String)),
                        optional: false,
                    }],
                    wildcard: None,
                }),
                input_schema_json: None,
            }],
        })
    }

    /// Mock GitHub MCP server introspection
    fn introspect_mock_github_mcp(&self) -> RuntimeResult<MCPIntrospectionResult> {
        Ok(MCPIntrospectionResult {
            server_url: "http://localhost:3000/github-mcp".to_string(),
            server_name: "github".to_string(),
            protocol_version: "2024-11-05".to_string(),
            tools: vec![
                DiscoveredMCPTool {
                    tool_name: "create_issue".to_string(),
                    description: Some("Create a new GitHub issue".to_string()),
                    input_schema: Some(TypeExpr::Map {
                        entries: vec![
                            MapTypeEntry {
                                key: Keyword("owner".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(
                                    rtfs::ast::PrimitiveType::String,
                                )),
                                optional: false,
                            },
                            MapTypeEntry {
                                key: Keyword("repo".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(
                                    rtfs::ast::PrimitiveType::String,
                                )),
                                optional: false,
                            },
                            MapTypeEntry {
                                key: Keyword("title".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(
                                    rtfs::ast::PrimitiveType::String,
                                )),
                                optional: false,
                            },
                            MapTypeEntry {
                                key: Keyword("body".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(
                                    rtfs::ast::PrimitiveType::String,
                                )),
                                optional: true,
                            },
                            MapTypeEntry {
                                key: Keyword("labels".to_string()),
                                value_type: Box::new(TypeExpr::Vector(Box::new(
                                    TypeExpr::Primitive(rtfs::ast::PrimitiveType::String),
                                ))),
                                optional: true,
                            },
                        ],
                        wildcard: None,
                    }),
                    output_schema: Some(TypeExpr::Map {
                        entries: vec![
                            MapTypeEntry {
                                key: Keyword("number".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(
                                    rtfs::ast::PrimitiveType::Int,
                                )),
                                optional: false,
                            },
                            MapTypeEntry {
                                key: Keyword("url".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(
                                    rtfs::ast::PrimitiveType::String,
                                )),
                                optional: false,
                            },
                            MapTypeEntry {
                                key: Keyword("state".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(
                                    rtfs::ast::PrimitiveType::String,
                                )),
                                optional: false,
                            },
                        ],
                        wildcard: None,
                    }),
                    input_schema_json: None,
                },
                DiscoveredMCPTool {
                    tool_name: "list_issues".to_string(),
                    description: Some("List issues in a GitHub repository".to_string()),
                    input_schema: Some(TypeExpr::Map {
                        entries: vec![
                            MapTypeEntry {
                                key: Keyword("owner".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(
                                    rtfs::ast::PrimitiveType::String,
                                )),
                                optional: false,
                            },
                            MapTypeEntry {
                                key: Keyword("repo".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(
                                    rtfs::ast::PrimitiveType::String,
                                )),
                                optional: false,
                            },
                            MapTypeEntry {
                                key: Keyword("state".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(
                                    rtfs::ast::PrimitiveType::String,
                                )),
                                optional: true,
                            },
                        ],
                        wildcard: None,
                    }),
                    output_schema: Some(TypeExpr::Vector(Box::new(TypeExpr::Map {
                        entries: vec![],
                        wildcard: None,
                    }))),
                    input_schema_json: None,
                },
            ],
        })
    }

    /// Generate the RTFS implementation code for an MCP tool wrapper
    pub fn generate_mcp_wrapper_code(
        &self,
        server_url: &str,
        tool_name: &str,
        display_name: &str,
    ) -> String {
        format!(
            "(fn [input]\n  ;; MCP Tool: {}\n  (call :ccos.capabilities.mcp.call\n    :server-url \"{}\"\n    :tool-name \"{}\"\n    :input input))",
            display_name, server_url, tool_name
        )
    }
}

impl Default for MCPIntrospector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_introspector_creation() {
        let introspector = MCPIntrospector::new();
        assert!(!introspector.mock_mode);
    }

    #[test]
    fn test_mcp_introspector_mock() {
        let introspector = MCPIntrospector::mock();
        assert!(introspector.mock_mode);
    }

    #[test]
    fn test_introspect_mock_github_mcp() {
        let introspector = MCPIntrospector::mock();
        let result = introspector.introspect_mock_mcp_server("github").unwrap();

        assert_eq!(result.server_name, "github");
        assert_eq!(result.tools.len(), 2);
        assert!(result.tools.iter().any(|t| t.tool_name == "create_issue"));
        assert!(result.tools.iter().any(|t| t.tool_name == "list_issues"));
    }

    #[test]
    fn test_create_capabilities_from_mcp() {
        let introspector = MCPIntrospector::mock();
        let introspection = introspector.introspect_mock_mcp_server("github").unwrap();
        let capabilities = introspector
            .create_capabilities_from_mcp(&introspection)
            .unwrap();

        assert_eq!(capabilities.len(), 2);
        assert!(capabilities.iter().any(|c| c.id.contains("create_issue")));
        assert!(capabilities.iter().any(|c| c.id.contains("list_issues")));

        // Check that schemas are properly encoded
        let create_issue_cap = capabilities
            .iter()
            .find(|c| c.id.contains("create_issue"))
            .unwrap();
        assert!(create_issue_cap.input_schema.is_some());
        assert!(create_issue_cap.output_schema.is_some());
    }
}
