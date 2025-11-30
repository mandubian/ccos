//! API and protocol introspection.
//!
//! This module contains introspectors for discovering capabilities from:
//! - MCP servers (Model Context Protocol)
//! - REST APIs (via introspection)
//! - Known APIs (pre-built definitions for common APIs)
//! - APIs.guru directory (community OpenAPI specs)
//! - LLM doc parsing (extract endpoints from HTML documentation)
//! - Authentication handling

pub mod api_introspector;
pub mod apis_guru;
pub mod auth_injector;
pub mod known_apis;
pub mod llm_doc_parser;
pub mod mcp_introspector;

// Re-export commonly used types
pub use api_introspector::{
    APIIntrospectionResult, APIIntrospector, AuthRequirements, DiscoveredEndpoint,
    EndpointParameter, RateLimitInfo,
};
pub use apis_guru::{ApisGuruClient, ApisGuruSearchResult};
pub use auth_injector::{AuthInjector, AuthType};
pub use known_apis::KnownApisRegistry;
pub use llm_doc_parser::LlmDocParser;
pub use mcp_introspector::MCPIntrospector;
