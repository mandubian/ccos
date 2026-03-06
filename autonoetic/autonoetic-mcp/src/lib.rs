//! Autonoetic MCP — Model Context Protocol client/server adapter.
//!
//! This crate provides the types and transport logic for integrating
//! external MCP servers (tool discovery) and exposing agents as MCP tools.

pub mod client;
pub mod protocol;
pub mod server;
pub mod types;

pub use client::McpClient;
pub use server::{AgentExecutor, AgentMcpServer};
pub use types::{ExposedAgent, McpServer, McpTool, McpToolCallResult, McpTransportConfig};
