//! API and protocol introspection.
//!
//! This module contains introspectors for discovering capabilities from:
//! - MCP servers (Model Context Protocol)
//! - REST APIs (via introspection)
//! - Known APIs (pre-built definitions for common APIs)
//! - APIs.guru directory (community OpenAPI specs)
//! - Authentication handling

pub mod api_introspector;
pub mod apis_guru;
pub mod auth_injector;
pub mod known_apis;
pub mod mcp_introspector;

// Re-export commonly used types
pub use api_introspector::{
    APIIntrospectionResult, APIIntrospector, AuthRequirements, DiscoveredEndpoint,
    EndpointParameter, RateLimitInfo,
};
pub use apis_guru::{ApisGuruClient, ApisGuruSearchResult};
pub use auth_injector::{AuthInjector, AuthType};
pub use known_apis::KnownApisRegistry;
pub use mcp_introspector::MCPIntrospector;
