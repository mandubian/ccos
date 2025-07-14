// Example: Bidirectional Streaming in RTFS Capability Marketplace
// This demonstrates the different streaming patterns supported

use std::collections::HashMap;

// Import the streaming types from the capability marketplace
use rtfs_compiler::runtime::capability_marketplace::{
    CapabilityMarketplace, StreamType, StreamingProvider, BidirectionalConfig,
    StreamItem, StreamDirection
};
use rtfs_compiler::runtime::values::Value;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let marketplace = CapabilityMarketplace::new();
    
    // Example 1: WebSocket-based bidirectional chat stream
    println!("🔄 Example 1: Bidirectional Chat Stream");
    
    let chat_config = BidirectionalConfig {
        input_buffer_size: 100,
        output_buffer_size: 100,
        flow_control: true,
        timeout_ms: 30000,
    };
    
    marketplace.register_bidirectional_stream_capability(
        "chat.websocket".to_string(),
        "WebSocket Chat".to_string(),
        "Real-time bidirectional chat capability".to_string(),
        StreamingProvider::WebSocket {
            url: "ws://localhost:8080/chat".to_string(),
            protocols: vec!["chat-v1".to_string()],
        },
        chat_config,
    ).await?;
    
    // Start the bidirectional stream
    let (sender, _receiver) = marketplace.start_bidirectional_stream(
        "chat.websocket",
        &Value::Map(HashMap::new()),
    ).await?;
    
    // Example usage: Send a message
    let message = StreamItem {
        data: Value::String("Hello, world!".to_string()),
        sequence: 1,
        timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs(),
        metadata: HashMap::new(),
        direction: StreamDirection::Outbound,
        correlation_id: Some("msg-001".to_string()),
    };
    
    // In a real implementation, this would send to the WebSocket
    if let Err(e) = sender.send(message).await {
        println!("Failed to send message: {}", e);
    }
    
    println!("✅ Bidirectional streaming example completed!");
    
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
        ("Source (SSE)", StreamType::Source),
        ("Sink (Logs)", StreamType::Sink),
        ("Transform (Processing)", StreamType::Transform),
        ("Bidirectional (Chat)", StreamType::Bidirectional),
        ("Duplex (Pipeline)", StreamType::Duplex),
    ];
    
    for (name, stream_type) in patterns {
        println!("   - {}: {:?}", name, stream_type);
    }
}
