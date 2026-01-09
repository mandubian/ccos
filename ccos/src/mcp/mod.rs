//! Unified MCP Discovery API
//!
//! This module provides a single, common API for MCP discovery across all CCOS modules.
//! It consolidates the discovery logic previously scattered across:
//! - `planner/modular_planner/resolution/mcp.rs` (resolution strategy)
//! - `capability_marketplace/mcp_discovery.rs` (marketplace discovery)
//! - `synthesis/mcp_introspector.rs` (introspection)
//!
//! ## Architecture
//!
//! - **Core**: `core.rs` - Unified discovery service
//! - **Discovery Session**: `discovery_session.rs` - Ephemeral session management for discovery
//! - **Registry**: `registry.rs` - MCP Registry API client
//! - **Types**: `types.rs` - Shared types and structures
//! - **Cache**: `cache.rs` - Caching layer for discovered tools
//! - **Rate Limiter**: `rate_limiter.rs` - Rate limiting and retry policies

pub mod cache;
pub mod capabilities;
pub mod core;
pub mod discovery_session;
pub mod http_transport;
pub mod rate_limiter;
pub mod registry;
pub mod server;
pub mod stdio_client;
pub mod types;

// Re-export main types with consistent naming (all caps for acronyms)
pub use discovery_session::{MCPCapabilities, MCPServerInfo, MCPSession, MCPSessionManager};
pub use registry::{MCPRegistryClient, McpServer};
pub use stdio_client::StdioClient;
pub use types::*;

// Re-export rate limiter types
pub use rate_limiter::{
    RateLimitConfig, RateLimiter, RetryContext, RetryPolicy, RetryPolicyConfig,
};

// Re-export core service
pub use core::{CacheWarmingStats, MCPDiscoveryService};
