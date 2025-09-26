use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::runtime::streaming::{
    McpStreamingProvider, StreamChunkSink, StreamTransport, StreamTransportArgs,
};
use crate::runtime::values::Value;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;

/// Event payload used by the mock transport to deliver data to the provider sink.
#[derive(Clone, Debug)]
pub struct MockTransportEvent {
    pub chunk: Value,
    pub metadata: Value,
    pub delay: Option<Duration>,
}

impl MockTransportEvent {
    pub fn immediate(chunk: Value, metadata: Value) -> Self {
        Self {
            chunk,
            metadata,
            delay: None,
        }
    }

    pub fn with_delay(chunk: Value, metadata: Value, delay: Duration) -> Self {
        Self {
            chunk,
            metadata,
            delay: Some(delay),
        }
    }
}

/// Mock transport that satisfies the generic transport trait for tests.
#[derive(Default, Clone)]
pub struct MockStreamTransport {
    senders: Arc<Mutex<HashMap<String, mpsc::Sender<MockTransportEvent>>>>,
}

impl MockStreamTransport {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn send_event(
        &self,
        stream_id: &str,
        event: MockTransportEvent,
    ) -> RuntimeResult<()> {
        let sender = {
            let guard = self
                .senders
                .lock()
                .map_err(|_| RuntimeError::Generic("mock transport poisoned".into()))?;
            guard.get(stream_id).cloned()
        };

        match sender {
            Some(tx) => tx
                .send(event)
                .await
                .map_err(|_| RuntimeError::Generic("mock transport channel closed".into())),
            None => Err(RuntimeError::Generic(format!(
                "no mock transport channel for stream {}",
                stream_id
            ))),
        }
    }

    pub fn try_send_event(&self, stream_id: &str, event: MockTransportEvent) -> RuntimeResult<()> {
        let sender = {
            let guard = self
                .senders
                .lock()
                .map_err(|_| RuntimeError::Generic("mock transport poisoned".into()))?;
            guard.get(stream_id).cloned()
        };

        match sender {
            Some(tx) => tx
                .try_send(event)
                .map_err(|_| RuntimeError::Generic("mock transport channel full".into())),
            None => Err(RuntimeError::Generic(format!(
                "no mock transport channel for stream {}",
                stream_id
            ))),
        }
    }

    fn register_sender(&self, stream_id: String, sender: mpsc::Sender<MockTransportEvent>) {
        if let Ok(mut guard) = self.senders.lock() {
            guard.insert(stream_id, sender);
        }
    }

    fn remove_sender(&self, stream_id: &str) {
        if let Ok(mut guard) = self.senders.lock() {
            guard.remove(stream_id);
        }
    }
}

#[async_trait]
impl StreamTransport for MockStreamTransport {
    async fn run(&self, args: StreamTransportArgs) -> RuntimeResult<()> {
        let StreamTransportArgs {
            stream_id,
            mut stop_rx,
            sink,
            ..
        } = args;

        let (tx, mut rx) = mpsc::channel::<MockTransportEvent>(64);
        self.register_sender(stream_id.clone(), tx);

        loop {
            tokio::select! {
                _ = stop_rx.recv() => {
                    self.remove_sender(&stream_id);
                    break;
                }
                maybe_event = rx.recv() => {
                    match maybe_event {
                        Some(event) => {
                            if let Some(delay) = event.delay {
                                sleep(delay).await;
                            }
                            sink.ingest_chunk(&stream_id, event.chunk, event.metadata).await?;
                        }
                        None => {
                            self.remove_sender(&stream_id);
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

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
