//! Capability providers (MCP and others)

pub mod github_mcp;
pub mod local_llm;
pub mod local_file_provider;
pub mod json_provider;
pub mod weather_mcp;
pub mod remote_rtfs_provider;
pub mod a2a_provider;

pub use github_mcp::GitHubMCPCapability;
pub use local_llm::LocalLlmProvider;
pub use local_file_provider::LocalFileProvider;
pub use json_provider::JsonProvider;
pub use weather_mcp::WeatherMCPCapability;
pub use remote_rtfs_provider::RemoteRTFSProvider;
pub use a2a_provider::A2AProvider;
