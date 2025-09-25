//! Capability providers (MCP and others)

pub mod github_mcp;
pub mod local_llm;
pub mod weather_mcp;

pub use github_mcp::GitHubMCPCapability;
pub use local_llm::LocalLlmProvider;
pub use weather_mcp::WeatherMCPCapability;
