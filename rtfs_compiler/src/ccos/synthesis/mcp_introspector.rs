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

use crate::ast::{Keyword, MapTypeEntry, TypeExpr};
use crate::ccos::capability_marketplace::types::CapabilityManifest;
use crate::ccos::synthesis::mcp_session::{MCPSessionManager, MCPServerInfo};
use crate::ccos::synthesis::schema_serializer::type_expr_to_rtfs_pretty;
use crate::runtime::error::{RuntimeError, RuntimeResult};
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
        self.introspect_mcp_server_with_auth(server_url, server_name, None).await
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

        println!("🔍 Introspecting MCP server: {} ({})", server_name, server_url);

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
                "string" => Ok(TypeExpr::Primitive(crate::ast::PrimitiveType::String)),
                "integer" | "number" => {
                    if schema.get("format").and_then(|f| f.as_str()) == Some("integer") {
                        Ok(TypeExpr::Primitive(crate::ast::PrimitiveType::Int))
                    } else {
                        Ok(TypeExpr::Primitive(crate::ast::PrimitiveType::Float))
                    }
                }
                "boolean" => Ok(TypeExpr::Primitive(crate::ast::PrimitiveType::Bool)),
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

    /// Create a single capability from an MCP tool
    fn create_capability_from_mcp_tool(
        &self,
        tool: &DiscoveredMCPTool,
        introspection: &MCPIntrospectionResult,
    ) -> RuntimeResult<CapabilityManifest> {
        let capability_id = format!("mcp.{}.{}", 
            introspection.server_name.replace("/", ".").replace(" ", "_"),
            tool.tool_name.replace("-", "_")
        );

        let mut effects = vec!["network_request".to_string(), "mcp_call".to_string()];
        
        let mut metadata = HashMap::new();
        metadata.insert("mcp_server_url".to_string(), introspection.server_url.clone());
        metadata.insert("mcp_server_name".to_string(), introspection.server_name.clone());
        metadata.insert("mcp_tool_name".to_string(), tool.tool_name.clone());
        metadata.insert("mcp_protocol_version".to_string(), introspection.protocol_version.clone());
        metadata.insert("discovery_method".to_string(), "mcp_introspection".to_string());

        if let Some(input_json) = &tool.input_schema_json {
            metadata.insert("mcp_input_schema_json".to_string(), input_json.to_string());
        }

        Ok(CapabilityManifest {
            id: capability_id,
            name: tool.tool_name.clone(),
            description: tool.description.clone()
                .unwrap_or_else(|| format!("MCP tool: {}", tool.tool_name)),
            provider: crate::ccos::capability_marketplace::types::ProviderType::Local(
                crate::ccos::capability_marketplace::types::LocalCapability {
                    handler: std::sync::Arc::new(|_| {
                        Ok(crate::runtime::values::Value::String(
                            "MCP RTFS capability".to_string(),
                        ))
                    }),
                },
            ),
            version: "1.0.0".to_string(),
            input_schema: tool.input_schema.clone(),
            output_schema: tool.output_schema.clone(),
            attestation: None,
            provenance: Some(crate::ccos::capability_marketplace::types::CapabilityProvenance {
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
    pub fn generate_mcp_rtfs_implementation(
        &self,
        tool: &DiscoveredMCPTool,
        server_url: &str,
    ) -> String {
        format!(
            r#"(fn [input]
  ;; MCP Tool: {}
  ;; Runtime validates input against input_schema
  ;; Makes MCP JSON-RPC call and validates result against output_schema
  ;; 
  ;; Note: This capability requires an MCP server.
  ;; Set MCP_SERVER_URL environment variable to override the default.
  ;; For local testing, you can use: export MCP_SERVER_URL=http://localhost:3000/mcp/github
  (let [default_url "{}"
        env_url (call "ccos.system.get-env" "MCP_SERVER_URL")
        mcp_url (if env_url env_url default_url)
        mcp_request {{:jsonrpc "2.0"
                      :id "mcp_call"
                      :method "tools/call"
                      :params {{:name "{}"
                               :arguments input}}}}
        ;; Make HTTP POST to MCP server
        response (call "ccos.network.http-fetch"
                      :method "POST"
                      :url mcp_url
                      :headers {{:content-type "application/json"}}
                      :body (call "ccos.data.serialize-json" mcp_request))]
    ;; Check if response body is nil or empty
    (if (get response :body)
      (let [response_json (call "ccos.data.parse-json" (get response :body))
            ;; Extract result (MCP wraps actual result in 'result' field)
            result (get response_json :result)]
        ;; Return the MCP tool result (runtime validates against output_schema)
        result)
      ;; Return error if no body
      {{:error "No response from MCP server" :url mcp_url}})))"#,
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
            .map(|s| type_expr_to_rtfs_pretty(s))
            .unwrap_or_else(|| ":any".to_string());

        let output_schema_str = capability
            .output_schema
            .as_ref()
            .map(|s| type_expr_to_rtfs_pretty(s))
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

        let mcp_server_url = capability.metadata.get("mcp_server_url").map(|s| s.as_str()).unwrap_or("");
        let mcp_tool_name = capability.metadata.get("mcp_tool_name").map(|s| s.as_str()).unwrap_or("");
        let mcp_server_name = capability.metadata.get("mcp_server_name").map(|s| s.as_str()).unwrap_or("");

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
  :source_url "{}"
  :discovery_method "mcp_introspection"
  :created_at "{}"
  :capability_type "mcp_tool"
  :permissions {}
  :effects {}
  :mcp_metadata {{
    :server_url "{}"
    :server_name "{}"
    :tool_name "{}"
    :protocol_version "{}"
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
            mcp_server_url,
            chrono::Utc::now().to_rfc3339(),
            permissions_str,
            effects_str,
            mcp_server_url,
            mcp_server_name,
            mcp_tool_name,
            capability.metadata.get("mcp_protocol_version").map(|s| s.as_str()).unwrap_or("2024-11-05"),
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
        let namespace = parts[1];     // "github"
        let tool_name = parts[2..].join("_"); // "list_issues" or "add_comment_to_pending_review"

        // Create directory: output_dir/mcp/<namespace>/
        let capability_dir = output_dir.join(provider_type).join(namespace);
        std::fs::create_dir_all(&capability_dir).map_err(|e| {
            RuntimeError::Generic(format!("Failed to create capability directory: {}", e))
        })?;

        let rtfs_content = self.capability_to_rtfs_string(capability, implementation_code);
        let rtfs_file = capability_dir.join(format!("{}.rtfs", tool_name));

        std::fs::write(&rtfs_file, rtfs_content).map_err(|e| {
            RuntimeError::Generic(format!("Failed to write RTFS file: {}", e))
        })?;

        Ok(rtfs_file)
    }

    /// Mock MCP server introspection for testing
    fn introspect_mock_mcp_server(&self, server_name: &str) -> RuntimeResult<MCPIntrospectionResult> {
        // Special handling for GitHub MCP
        if server_name.contains("github") {
            return self.introspect_mock_github_mcp();
        }

        // Default mock MCP server
        Ok(MCPIntrospectionResult {
            server_url: format!("http://localhost:3000/{}", server_name),
            server_name: server_name.to_string(),
            protocol_version: "2024-11-05".to_string(),
            tools: vec![
                DiscoveredMCPTool {
                    tool_name: "example_tool".to_string(),
                    description: Some("Example MCP tool".to_string()),
                    input_schema: Some(TypeExpr::Map {
                        entries: vec![
                            MapTypeEntry {
                                key: Keyword("query".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(crate::ast::PrimitiveType::String)),
                                optional: false,
                            },
                        ],
                        wildcard: None,
                    }),
                    output_schema: Some(TypeExpr::Map {
                        entries: vec![
                            MapTypeEntry {
                                key: Keyword("result".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(crate::ast::PrimitiveType::String)),
                                optional: false,
                            },
                        ],
                        wildcard: None,
                    }),
                    input_schema_json: None,
                },
            ],
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
                                value_type: Box::new(TypeExpr::Primitive(crate::ast::PrimitiveType::String)),
                                optional: false,
                            },
                            MapTypeEntry {
                                key: Keyword("repo".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(crate::ast::PrimitiveType::String)),
                                optional: false,
                            },
                            MapTypeEntry {
                                key: Keyword("title".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(crate::ast::PrimitiveType::String)),
                                optional: false,
                            },
                            MapTypeEntry {
                                key: Keyword("body".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(crate::ast::PrimitiveType::String)),
                                optional: true,
                            },
                            MapTypeEntry {
                                key: Keyword("labels".to_string()),
                                value_type: Box::new(TypeExpr::Vector(Box::new(TypeExpr::Primitive(crate::ast::PrimitiveType::String)))),
                                optional: true,
                            },
                        ],
                        wildcard: None,
                    }),
                    output_schema: Some(TypeExpr::Map {
                        entries: vec![
                            MapTypeEntry {
                                key: Keyword("number".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(crate::ast::PrimitiveType::Int)),
                                optional: false,
                            },
                            MapTypeEntry {
                                key: Keyword("url".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(crate::ast::PrimitiveType::String)),
                                optional: false,
                            },
                            MapTypeEntry {
                                key: Keyword("state".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(crate::ast::PrimitiveType::String)),
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
                                value_type: Box::new(TypeExpr::Primitive(crate::ast::PrimitiveType::String)),
                                optional: false,
                            },
                            MapTypeEntry {
                                key: Keyword("repo".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(crate::ast::PrimitiveType::String)),
                                optional: false,
                            },
                            MapTypeEntry {
                                key: Keyword("state".to_string()),
                                value_type: Box::new(TypeExpr::Primitive(crate::ast::PrimitiveType::String)),
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

