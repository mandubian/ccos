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
    fn json_schema_to_rtfs_type(&self, schema: &serde_json::Value) -> RuntimeResult<TypeExpr> {
        if let Some(type_str) = schema.get("type").and_then(|t| t.as_str()) {
            match type_str {
                "string" => Ok(TypeExpr::Primitive(rtfs::ast::PrimitiveType::String)),
                "integer" | "number" => {
                    if schema.get("format").and_then(|f| f.as_str()) == Some("integer") {
                        Ok(TypeExpr::Primitive(rtfs::ast::PrimitiveType::Int))
                    } else {
                        Ok(TypeExpr::Primitive(rtfs::ast::PrimitiveType::Float))
                    }
                }
                "boolean" => Ok(TypeExpr::Primitive(rtfs::ast::PrimitiveType::Bool)),
                "array" => {
                    let element_type = if let Some(items) = schema.get("items") {
                        self.json_schema_to_rtfs_type(items)?
                    } else {
                        TypeExpr::Any
                    };
                    Ok(TypeExpr::Vector(Box::new(element_type)))
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
                            entries.push(MapTypeEntry {
                                key: Keyword(key.clone()),
                                value_type: Box::new(prop_type),
                                optional: !required_fields.contains(key),
                            });
                        }
                    }
                    Ok(TypeExpr::Map {
                        entries,
                        wildcard: None,
                    })
                }
                _ => Ok(TypeExpr::Any),
            }
        } else {
            Ok(TypeExpr::Any)
        }
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
    /// This is only done if authorized (auth_headers provided) and safe to do so
    pub async fn introspect_output_schema(
        &self,
        tool: &DiscoveredMCPTool,
        server_url: &str,
        server_name: &str,
        auth_headers: Option<HashMap<String, String>>,
    ) -> RuntimeResult<Option<TypeExpr>> {
        // Only introspect if we have auth (indicates we're authorized to make calls)
        if auth_headers.is_none() {
            return Ok(None);
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
            return Ok(None);
        }

        eprintln!(
            "🔍 Introspecting output schema for '{}' by calling it once with safe inputs",
            tool.tool_name
        );

        // Generate safe test inputs from input schema
        let test_inputs = self.generate_safe_test_inputs(tool)?;

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
                return Ok(None);
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
                return Ok(None);
            }
        };

        // Terminate session
        let _ = session_manager.terminate_session(&session).await;

        // Extract result from response
        if let Some(result) = response.get("result") {
            // Infer output schema from the actual response
            let output_schema = self.infer_type_from_json_value(result)?;
            eprintln!(
                "✅ Inferred output schema for '{}': {}",
                tool.tool_name,
                type_expr_to_rtfs_compact(&output_schema)
            );
            Ok(Some(output_schema))
        } else {
            eprintln!("⚠️ No result in response for output schema introspection");
            Ok(None)
        }
    }

    /// Generate safe test inputs from input schema
    fn generate_safe_test_inputs(
        &self,
        tool: &DiscoveredMCPTool,
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
                        let default_value = self.generate_safe_default_value(prop_schema);
                        inputs.insert(key.clone(), default_value);
                    }
                }
            }
        }

        Ok(serde_json::Value::Object(inputs))
    }

    /// Generate a safe default value for a JSON schema property
    fn generate_safe_default_value(&self, schema: &serde_json::Value) -> serde_json::Value {
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
    fn infer_type_from_json_value(&self, value: &serde_json::Value) -> RuntimeResult<TypeExpr> {
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
    fn create_capability_from_mcp_tool(
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
            provider: crate::capability_marketplace::types::ProviderType::Local(
                crate::capability_marketplace::types::LocalCapability {
                    handler: std::sync::Arc::new(|_| {
                        Ok(rtfs::runtime::values::Value::String(
                            "MCP RTFS capability".to_string(),
                        ))
                    }),
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
    /// This is a generic MCP wrapper that:
    /// 1. Reads MCP server URL from metadata (overridable via MCP_SERVER_URL env var)
    /// 2. Optionally gets auth token from input schema or env var (MCP_AUTH_TOKEN)
    /// 3. Makes standard JSON-RPC call to MCP server
    /// 4. Returns result with schema validation by runtime
    ///
    /// Session management is handled transparently by the runtime/registry based on
    /// capability metadata (mcp_requires_session, mcp_auth_env_var).
    pub fn generate_mcp_rtfs_implementation(
        &self,
        tool: &DiscoveredMCPTool,
        server_url: &str,
    ) -> String {
        format!(
            r#"(fn [input]
  ;; MCP Tool: {}
  ;; Runtime validates input against input_schema and output_schema
  ;; Makes standard MCP JSON-RPC call to tools/call endpoint
  ;; 
  ;; Configuration:
  ;;   - MCP_SERVER_URL: Override server URL (default from metadata)
  ;;   - MCP_AUTH_TOKEN: Optional auth token for MCP server
  ;;
  ;; Session management is handled by the runtime based on capability metadata.
  (let [default_url "{}"
        env_url (call "ccos.system.get-env" "MCP_SERVER_URL")
        mcp_url (if env_url env_url default_url)
        ;; Optional: get auth token from input or env
        auth_token (or (get input :auth-token)
                       (call "ccos.system.get-env" "MCP_AUTH_TOKEN"))
        ;; Build MCP JSON-RPC request
        mcp_request {{:jsonrpc "2.0"
                      :id "mcp_call"
                      :method "tools/call"
                      :params {{:name "{}"
                               :arguments input}}}}
        ;; Build headers with optional auth
        headers (if auth_token
                  {{:content-type "application/json"
                    :authorization (str "Bearer " auth_token)}}
                  {{:content-type "application/json"}})]
    ;; Make HTTP POST to MCP server
    (let [response (call "ccos.network.http-fetch"
                        :method "POST"
                        :url mcp_url
                        :headers headers
                        :body (call "ccos.data.serialize-json" mcp_request))]
      ;; Parse response and extract result
      (if (get response :body)
        (let [response_json (call "ccos.data.parse-json" (get response :body))
              result (get response_json :result)]
          ;; Return MCP tool result (runtime validates against output_schema)
          result)
        ;; Return error if no response body
        {{:error "No response from MCP server" :url mcp_url}}))))"#,
            tool.description.as_deref().unwrap_or(&tool.tool_name),
            server_url,
            tool.tool_name
        )
    }

    /// Serialize MCP capability to RTFS format
    pub fn capability_to_rtfs_string(
        &self,
        capability: &CapabilityManifest,
        implementation_code: &str,
    ) -> String {
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

        format!(
            r#";; MCP Capability: {}
;; Generated from MCP tool introspection
;; MCP Server: {} ({})
;; Tool: {}

(capability "{}"
  :name "{}"
  :version "{}"
  :description "{}"
  :provider "MCP"
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
            capability.id,
            capability.name,
            capability.version,
            capability.description,
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

        let rtfs_content = self.capability_to_rtfs_string(capability, implementation_code);
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
