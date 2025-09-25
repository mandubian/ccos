use crate::runtime::error::RuntimeResult;
use crate::runtime::streaming::McpStreamingProvider;
use crate::runtime::values::Value;
use std::time::Duration;
use tokio::time::sleep;

/// Simple mock event loop that feeds synthetic chunks into a registered stream processor.
/// This simulates an MCP server pushing incremental results. In real integration this
/// would read from a network/WebSocket source.
pub async fn run_mock_stream_loop(
    provider: &McpStreamingProvider,
    stream_id: String,
    total: usize,
) -> RuntimeResult<()> {
    for i in 0..total {
        let chunk = Value::Map({
            let mut m = std::collections::HashMap::new();
            m.insert(
                crate::ast::MapKey::Keyword(crate::ast::Keyword("seq".into())),
                Value::Integer(i as i64),
            );
            m.insert(
                crate::ast::MapKey::Keyword(crate::ast::Keyword("payload".into())),
                Value::String(format!("mock-payload-{}", i)),
            );
            m
        });
        let meta = Value::Map(std::collections::HashMap::new());
        provider.process_chunk(&stream_id, chunk, meta).await?;
        sleep(Duration::from_millis(25)).await;
    }
    Ok(())
}
