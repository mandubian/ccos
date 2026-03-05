//! MCP stub types for tool discovery and registration.

use serde::{Deserialize, Serialize};

/// A registered MCP server managed by the Gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServer {
    /// Human-readable server name.
    pub name: String,
    /// Subprocess command to spawn.
    pub command: String,
    /// Subprocess arguments.
    #[serde(default)]
    pub args: Vec<String>,
}

/// A tool discovered from an MCP server via `tools/list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    /// Tool name (namespaced as `mcp_{server}_{name}`).
    pub name: String,
    /// Human-readable description.
    pub description: Option<String>,
    /// JSON Schema for tool inputs.
    pub input_schema: Option<serde_json::Value>,
}
