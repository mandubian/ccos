//! API and protocol introspection.
//!
//! This module contains introspectors for discovering capabilities from:
//! - MCP servers (Model Context Protocol)
//! - REST APIs (via introspection)
//! - Authentication handling

pub mod api_introspector;
pub mod auth_injector;
pub mod mcp_introspector;

// Re-export commonly used types
pub use api_introspector::{APIIntrospectionResult, APIIntrospector};
pub use auth_injector::{AuthInjector, AuthType};
pub use mcp_introspector::MCPIntrospector;
