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
    /// Transport mode used to connect to this MCP server.
    #[serde(default)]
    pub transport: McpTransportConfig,
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

/// How to connect to an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpTransportConfig {
    /// JSON-RPC over stdio to a spawned subprocess.
    Stdio,
    /// JSON-RPC HTTP endpoint for SSE-capable MCP servers.
    /// The endpoint should accept MCP JSON-RPC requests.
    Sse {
        url: String,
    },
}

impl Default for McpTransportConfig {
    fn default() -> Self {
        Self::Stdio
    }
}

/// Result payload from `tools/call`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolCallResult {
    /// Raw MCP `result` payload.
    pub payload: serde_json::Value,
}

/// Agent descriptor exposed as an MCP tool by Autonoetic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExposedAgent {
    pub id: String,
    pub name: String,
    pub description: String,
}
