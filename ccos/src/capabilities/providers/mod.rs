//! Capability providers (MCP and others)

pub mod a2a_provider;
pub mod github_mcp;
pub mod json_provider;
pub mod local_file_provider;
pub mod local_llm;
pub mod remote_rtfs_provider;
pub mod weather_mcp;

pub use a2a_provider::A2AProvider;
pub use github_mcp::GitHubMCPCapability;
pub use json_provider::JsonProvider;
pub use local_file_provider::LocalFileProvider;
pub use local_llm::LocalLlmProvider;
pub use remote_rtfs_provider::RemoteRTFSProvider;
pub use weather_mcp::WeatherMCPCapability;
