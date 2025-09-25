pub mod mock_loop;
pub mod rtfs_streaming_syntax;

use crate::ccos::capability_marketplace::CapabilityMarketplace;
use crate::runtime::streaming::{
    InMemoryStreamPersistence, McpStreamingProvider, StreamType, StreamingProvider,
};
use std::sync::Arc;

/// Register the core MCP streaming capability `mcp.stream.start` so lowered forms can resolve.
/// This is an initial bootstrap helper; later we may attach richer metadata or dynamic discovery.
pub async fn register_mcp_streaming_capability(
    marketplace: Arc<CapabilityMarketplace>,
    server_url: String,
) -> Result<(), crate::runtime::error::RuntimeError> {
    let provider: StreamingProvider = Arc::new(McpStreamingProvider::new_with_persistence(
        server_url,
        Arc::new(InMemoryStreamPersistence::new()),
        None,
    ));
    marketplace
        .register_streaming_capability(
            "mcp.stream.start".to_string(),
            "MCP Streaming Start".to_string(),
            "Initiate an MCP streaming session and register its RTFS processor".to_string(),
            StreamType::Unidirectional,
            provider,
        )
        .await
}
