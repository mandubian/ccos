//! Capability providers (MCP and others)

pub mod github_mcp;
pub mod weather_mcp;

pub use github_mcp::GitHubMCPCapability;
pub use weather_mcp::WeatherMCPCapability;
