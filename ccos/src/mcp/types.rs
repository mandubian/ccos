//! Shared types for MCP discovery
//!
//! This module defines core types used by the unified MCP discovery API.
//! Types are defined here rather than re-exported to provide a single source of truth.

use std::collections::HashMap;
use rtfs::ast::TypeExpr;
use serde::{Deserialize, Serialize};

// Re-export existing types to avoid duplication
pub use crate::capability_marketplace::mcp_discovery::{
    MCPServerConfig, MCPTool,
};

/// A discovered MCP tool with its schema
///
/// This represents a tool discovered from an MCP server, including its
/// parsed input/output schemas converted from JSON Schema to RTFS TypeExpr.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredMCPTool {
    pub tool_name: String,
    pub description: Option<String>,
    pub input_schema: Option<TypeExpr>,
    pub output_schema: Option<TypeExpr>,
    pub input_schema_json: Option<serde_json::Value>,
}

/// Result of discovering tools from an MCP server
#[derive(Debug, Clone)]
pub struct MCPDiscoveryResult {
    /// Server configuration used for discovery
    pub server_config: MCPServerConfig,
    /// Discovered tools (parsed with schemas)
    pub tools: Vec<DiscoveredMCPTool>,
    /// Discovered resources (raw JSON)
    pub resources: Vec<serde_json::Value>,
    /// Protocol version from server
    pub protocol_version: String,
}

/// Options for tool discovery
#[derive(Debug, Clone, Default)]
pub struct DiscoveryOptions {
    /// Whether to introspect output schemas (requires calling tools)
    pub introspect_output_schemas: bool,
    /// Whether to use cache if available
    pub use_cache: bool,
    /// Whether to register discovered tools in marketplace
    pub register_in_marketplace: bool,
    /// Whether to export discovered capabilities to RTFS files
    pub export_to_rtfs: bool,
    /// Directory to export RTFS files to (default: "capabilities/discovered")
    pub export_directory: Option<String>,
    /// Custom auth headers (overrides server config)
    pub auth_headers: Option<HashMap<String, String>>,
}

