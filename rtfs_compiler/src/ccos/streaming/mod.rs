pub mod mock_loop;
pub mod rtfs_streaming_syntax;

use crate::ccos::capability_marketplace::CapabilityMarketplace;
use crate::runtime::error::RuntimeError;
use crate::runtime::streaming::{
    InMemoryStreamPersistence, McpStreamingProvider, StreamInspectOptions, StreamType,
    StreamingProvider,
};
use crate::runtime::values::Value;
use std::sync::Arc;

/// Register the core MCP streaming capability `mcp.stream.start` so lowered forms can resolve.
/// This is an initial bootstrap helper; later we may attach richer metadata or dynamic discovery.
pub async fn register_mcp_streaming_capability(
    marketplace: Arc<CapabilityMarketplace>,
    server_url: String,
) -> Result<(), RuntimeError> {
    let provider_impl = Arc::new(McpStreamingProvider::new_with_persistence(
        server_url,
        Arc::new(InMemoryStreamPersistence::new()),
        None,
    ));
    let provider: StreamingProvider = provider_impl.clone();
    marketplace
        .register_streaming_capability(
            "mcp.stream.start".to_string(),
            "MCP Streaming Start".to_string(),
            "Initiate an MCP streaming session and register its RTFS processor".to_string(),
            StreamType::Unidirectional,
            provider.clone(),
            None,
            None,
            vec![":network".to_string()],
        )
        .await?;

    let inspect_provider = provider_impl.clone();
    marketplace
        .register_local_capability(
            "mcp.stream.inspect".to_string(),
            "MCP Stream Inspect".to_string(),
            "Inspect currently tracked MCP stream processors, including stats and state snapshots"
                .to_string(),
            Arc::new(move |params| {
                use crate::ast::{Keyword, MapKey};
                let mut options = StreamInspectOptions::default();
                let mut stream_id: Option<String> = None;

                if let Value::Map(map) = params {
                    let lookup = |key: &str| -> Option<&Value> {
                        let keyword_key = MapKey::Keyword(Keyword(key.to_string()));
                        map.get(&keyword_key)
                            .or_else(|| map.get(&MapKey::String(key.to_string())))
                    };

                    if let Some(Value::String(id)) = lookup("stream-id") {
                        stream_id = Some(id.clone());
                    }
                    if let Some(Value::Boolean(include_state)) = lookup("include-state") {
                        options.include_state = *include_state;
                    }
                    if let Some(Value::Boolean(include_initial_state)) =
                        lookup("include-initial-state")
                    {
                        options.include_initial_state = *include_initial_state;
                    }
                    if let Some(Value::Boolean(include_queue)) = lookup("include-queue") {
                        options.include_queue = *include_queue;
                    }
                } else if !matches!(params, Value::Nil) {
                    return Err(RuntimeError::Generic(
                        "mcp.stream.inspect expects a map value".to_string(),
                    ));
                }

                if let Some(id) = stream_id {
                    inspect_provider.inspect_stream(&id, options)
                } else {
                    Ok(inspect_provider.inspect_streams(options))
                }
            }),
        )
        .await?;

    Ok(())
}
