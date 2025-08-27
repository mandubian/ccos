// Example: Simplified Streaming in RTFS Capability Marketplace
// This demonstrates basic streaming capability registration

use std::sync::Arc;

// Import marketplace and streaming types
use rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace;
use rtfs_compiler::runtime::streaming::{StreamType, StreamingProvider, StreamingCapability, StreamConfig, StreamHandle};
use rtfs_compiler::runtime::capabilities::registry::CapabilityRegistry;
use rtfs_compiler::runtime::values::Value;
use rtfs_compiler::runtime::error::RuntimeResult;
use tokio::sync::mpsc;
use tokio::sync::RwLock;
use async_trait::async_trait;

// Simple streaming capability implementation for demonstration
pub struct SimpleStreamCapability {
    name: String,
}

impl SimpleStreamCapability {
    pub fn new(name: String) -> Self {
        Self { name }
    }
}

#[async_trait]
impl StreamingCapability for SimpleStreamCapability {
    fn start_stream(&self, _params: &Value) -> RuntimeResult<StreamHandle> {
        let (_stop_tx, _stop_rx) = mpsc::channel(1);
        Ok(StreamHandle {
            stream_id: format!("stream-{}", self.name),
            stop_tx: _stop_tx,
        })
    }

    fn stop_stream(&self, _handle: &StreamHandle) -> RuntimeResult<()> {
        Ok(())
    }

    async fn start_stream_with_config(&self, _params: &Value, _config: &StreamConfig) -> RuntimeResult<StreamHandle> {
        let (_stop_tx, _stop_rx) = mpsc::channel(1);
        Ok(StreamHandle {
            stream_id: format!("stream-with-config-{}", self.name),
            stop_tx: _stop_tx,
        })
    }

    async fn send_to_stream(&self, _handle: &StreamHandle, _data: &Value) -> RuntimeResult<()> {
        Ok(())
    }

    fn start_bidirectional_stream(&self, _params: &Value) -> RuntimeResult<StreamHandle> {
        let (_stop_tx, _stop_rx) = mpsc::channel(1);
        Ok(StreamHandle {
            stream_id: format!("bidir-stream-{}", self.name),
            stop_tx: _stop_tx,
        })
    }

    async fn start_bidirectional_stream_with_config(&self, _params: &Value, _config: &StreamConfig) -> RuntimeResult<StreamHandle> {
        let (_stop_tx, _stop_rx) = mpsc::channel(1);
        Ok(StreamHandle {
            stream_id: format!("bidir-config-stream-{}", self.name),
            stop_tx: _stop_tx,
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let capability_marketplace = Arc::new(CapabilityMarketplace::new(registry));
    
    // Example 1: Register a simple streaming capability
    println!("🔄 Example 1: Simple Stream Registration");
    
    let simple_provider: StreamingProvider = Arc::new(SimpleStreamCapability::new("websocket-chat".to_string()));
    
    capability_marketplace.register_streaming_capability(
        "chat.websocket".to_string(),
        "WebSocket Chat".to_string(),
        "Real-time bidirectional chat capability".to_string(),
        StreamType::Bidirectional,
        simple_provider,
    ).await?;

    println!("✅ Simple streaming capability registered successfully!");
    
    // Example 2: Demonstrate stream patterns
    demonstrate_stream_patterns();
    
    Ok(())
}

// Helper function to demonstrate stream patterns
fn demonstrate_stream_patterns() {
    println!("\n📋 Stream Pattern Summary:");
    println!("1. Source: Unidirectional outbound data (e.g., SSE, RSS feeds)");
    println!("2. Sink: Unidirectional inbound data (e.g., log collection, metrics)");
    println!("3. Transform: Unidirectional with processing (input → transform → output)");
    println!("4. Bidirectional: Simultaneous send/receive (e.g., chat, collaboration)");
    println!("5. Duplex: Separate input/output channels (e.g., data processing pipelines)");
    
    // Example configurations for each pattern
    let patterns = vec![
        ("Source (SSE)", StreamType::Unidirectional),
        ("Bidirectional (Chat)", StreamType::Bidirectional),
        ("Duplex (Pipeline)", StreamType::Duplex),
    ];
    
    for (name, stream_type) in patterns {
        println!("   - {}: {:?}", name, stream_type);
    }
}
