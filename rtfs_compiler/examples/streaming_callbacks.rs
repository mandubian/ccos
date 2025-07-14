// Example: Streaming with Callbacks in RTFS Capability Marketplace
// This demonstrates how to use both channel-based and callback-based streaming

use std::collections::HashMap;
use std::sync::Arc;

// Import the streaming types from the capability marketplace
use rtfs_compiler::runtime::capability_marketplace::{
    CapabilityMarketplace, StreamType, StreamingProvider, BidirectionalConfig,
    StreamConfig, StreamCallbacks, StreamEvent, StreamDirection, StreamItem
};
use rtfs_compiler::runtime::values::Value;
use rtfs_compiler::runtime::error::RuntimeError;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let marketplace = CapabilityMarketplace::new();
    
    // Example 1: Channel-based streaming (default behavior)
    println!("🔄 Example 1: Channel-based Streaming (Default)");
    
    marketplace.register_streaming_capability(
        "logs.channel".to_string(),
        "Channel-based Log Stream".to_string(),
        "Traditional channel-based streaming".to_string(),
        StreamType::Source,
        StreamingProvider::Local { buffer_size: 100 },
    ).await?;
    
    // Start channel-based stream (traditional way)
    let (_sender, _receiver) = marketplace.start_bidirectional_stream(
        "logs.channel",
        &Value::Map(HashMap::new()),
    ).await?;
    
    println!("✅ Channel-based stream started successfully");
    
    // Example 2: Callback-based streaming
    println!("\n📞 Example 2: Callback-based Streaming");
    
    // Define callback functions
    let on_connected = Arc::new(|event: StreamEvent| -> Result<(), RuntimeError> {
        if let StreamEvent::Connected { stream_id, metadata } = event {
            println!("🔌 Stream '{}' connected with metadata: {:?}", stream_id, metadata);
        }
        Ok(())
    });
    
    let on_data_received = Arc::new(|event: StreamEvent| -> Result<(), RuntimeError> {
        if let StreamEvent::DataReceived { stream_id, item } = event {
            println!("📥 Stream '{}' received data: {:?}", stream_id, item.data);
            println!("   Direction: {:?}, Sequence: {}", item.direction, item.sequence);
        }
        Ok(())
    });
    
    let on_data_sent = Arc::new(|event: StreamEvent| -> Result<(), RuntimeError> {
        if let StreamEvent::DataSent { stream_id, item } = event {
            println!("📤 Stream '{}' sent data: {:?}", stream_id, item.data);
            println!("   Correlation ID: {:?}", item.correlation_id);
        }
        Ok(())
    });
    
    let on_error = Arc::new(|event: StreamEvent| -> Result<(), RuntimeError> {
        if let StreamEvent::Error { stream_id, error } = event {
            println!("❌ Stream '{}' error: {}", stream_id, error);
        }
        Ok(())
    });
    
    let on_backpressure = Arc::new(|event: StreamEvent| -> Result<(), RuntimeError> {
        match event {
            StreamEvent::BackpressureTriggered { stream_id, buffer_size } => {
                println!("⚠️ Backpressure triggered on stream '{}' (buffer: {})", stream_id, buffer_size);
            }
            StreamEvent::BackpressureRelieved { stream_id, buffer_size } => {
                println!("✅ Backpressure relieved on stream '{}' (buffer: {})", stream_id, buffer_size);
            }
            _ => {}
        }
        Ok(())
    });
    
    // Create stream configuration with callbacks
    let callback_config = StreamConfig {
        buffer_size: 50,
        enable_callbacks: true,
        callbacks: StreamCallbacks {
            on_connected: Some(on_connected),
            on_disconnected: None,
            on_data_received: Some(on_data_received),
            on_data_sent: Some(on_data_sent),
            on_error: Some(on_error),
            on_progress: None,
            on_backpressure: Some(on_backpressure),
        },
        metadata: HashMap::from([
            ("purpose".to_string(), "callback-demo".to_string()),
            ("version".to_string(), "1.0".to_string()),
        ]),
    };
    
    // Register a chat capability for callback demonstration
    let chat_config = BidirectionalConfig {
        input_buffer_size: 50,
        output_buffer_size: 50,
        flow_control: true,
        timeout_ms: 30000,
    };
    
    marketplace.register_bidirectional_stream_capability(
        "chat.callback".to_string(),
        "Callback-based Chat".to_string(),
        "Chat with callback event handling".to_string(),
        StreamingProvider::WebSocket {
            url: "ws://localhost:8080/chat".to_string(),
            protocols: vec!["chat-v1".to_string()],
        },
        chat_config,
    ).await?;
    
    // Start callback-based stream
    let stream_handle = marketplace.start_bidirectional_stream_with_config(
        "chat.callback",
        &Value::Map(HashMap::new()),
        &callback_config,
    ).await?;
    
    println!("✅ Callback-based stream started successfully");
    
    // Example 3: Using both channels and callbacks
    println!("\n🔄 Example 3: Using Both Channels and Callbacks");
    
    // Simulate connection event
    stream_handle.trigger_event(StreamEvent::Connected {
        stream_id: "chat.callback".to_string(),
        metadata: HashMap::from([("status".to_string(), "ready".to_string())]),
    })?;
    
    // Send a message using the stream handle (this will trigger callbacks)
    let message = StreamItem {
        data: Value::String("Hello from callback world!".to_string()),
        sequence: 1,
        timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs(),
        metadata: HashMap::new(),
        direction: StreamDirection::Outbound,
        correlation_id: Some("demo-001".to_string()),
    };
    
    if let Err(e) = stream_handle.send(message).await {
        println!("Failed to send message: {}", e);
    }
    
    // Simulate receiving a message (this will trigger callbacks)
    let received_message = StreamItem {
        data: Value::String("Hello back!".to_string()),
        sequence: 2,
        timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs(),
        metadata: HashMap::new(),
        direction: StreamDirection::Inbound,
        correlation_id: Some("demo-002".to_string()),
    };
    
    stream_handle.trigger_event(StreamEvent::DataReceived {
        stream_id: "chat.callback".to_string(),
        item: received_message,
    })?;
    
    // Simulate backpressure events
    stream_handle.trigger_event(StreamEvent::BackpressureTriggered {
        stream_id: "chat.callback".to_string(),
        buffer_size: 50,
    })?;
    
    stream_handle.trigger_event(StreamEvent::BackpressureRelieved {
        stream_id: "chat.callback".to_string(),
        buffer_size: 50,
    })?;
    
    // Example 4: Hybrid approach - channels for data, callbacks for events
    println!("\n🔀 Example 4: Hybrid Approach - Channels + Events");
    
    // Create a config with callbacks disabled for data flow but enabled for events
    let hybrid_config = StreamConfig {
        buffer_size: 100,
        enable_callbacks: false, // Use channels for data
        callbacks: StreamCallbacks::default(), // No callbacks
        metadata: HashMap::from([("mode".to_string(), "hybrid".to_string())]),
    };
    
    marketplace.register_streaming_capability(
        "data.hybrid".to_string(),
        "Hybrid Stream".to_string(),
        "Uses channels for data, callbacks for events".to_string(),
        StreamType::Transform,
        StreamingProvider::Local { buffer_size: 100 },
    ).await?;
    
    let hybrid_handle = marketplace.start_stream_with_config(
        "data.hybrid",
        &Value::Map(HashMap::new()),
        &hybrid_config,
    ).await?;
    
    println!("✅ Hybrid stream started: callbacks={}, channels available", hybrid_handle.callbacks_enabled);
    
    // Example 5: Advanced callback pattern - middleware-like behavior
    println!("\n🔧 Example 5: Advanced Callback Pattern");
    
    let middleware_callback = Arc::new(|event: StreamEvent| -> Result<(), RuntimeError> {
        match event {
            StreamEvent::DataReceived { stream_id, ref item } => {
                // Middleware-like processing
                println!("🔧 Middleware processing data from '{}': {:?}", stream_id, item.data);
                
                // Could perform validation, transformation, logging, etc.
                if let Value::String(content) = &item.data {
                    if content.contains("error") {
                        return Err(RuntimeError::Generic("Detected error content".to_string()));
                    }
                }
                
                println!("✅ Middleware approved data");
            }
            _ => {}
        }
        Ok(())
    });
    
    let advanced_config = StreamConfig {
        buffer_size: 200,
        enable_callbacks: true,
        callbacks: StreamCallbacks {
            on_connected: None,
            on_disconnected: None,
            on_data_received: Some(middleware_callback),
            on_data_sent: None,
            on_error: None,
            on_progress: None,
            on_backpressure: None,
        },
        metadata: HashMap::from([("pattern".to_string(), "middleware".to_string())]),
    };
    
    marketplace.register_streaming_capability(
        "middleware.stream".to_string(),
        "Middleware Stream".to_string(),
        "Stream with middleware-like callback processing".to_string(),
        StreamType::Sink,
        StreamingProvider::Http {
            url: "http://localhost:8081/data".to_string(),
            method: "POST".to_string(),
            headers: HashMap::from([("Content-Type".to_string(), "application/json".to_string())]),
        },
    ).await?;
    
    let middleware_handle = marketplace.start_stream_with_config(
        "middleware.stream",
        &Value::Map(HashMap::new()),
        &advanced_config,
    ).await?;
    
    // Test middleware with valid data
    middleware_handle.trigger_event(StreamEvent::DataReceived {
        stream_id: "middleware.stream".to_string(),
        item: StreamItem {
            data: Value::String("Valid data".to_string()),
            sequence: 1,
            timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs(),
            metadata: HashMap::new(),
            direction: StreamDirection::Inbound,
            correlation_id: Some("valid-001".to_string()),
        },
    })?;
    
    // Test middleware with error data
    if let Err(e) = middleware_handle.trigger_event(StreamEvent::DataReceived {
        stream_id: "middleware.stream".to_string(),
        item: StreamItem {
            data: Value::String("This contains error".to_string()),
            sequence: 2,
            timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs(),
            metadata: HashMap::new(),
            direction: StreamDirection::Inbound,
            correlation_id: Some("error-001".to_string()),
        },
    }) {
        println!("❌ Middleware rejected data: {}", e);
    }
    
    println!("\n✅ All callback examples completed!");
    
    // Summary
    println!("\n📋 Summary:");
    println!("1. Channel-based streaming (default): Traditional mpsc::Sender/Receiver");
    println!("2. Callback-based streaming: Event-driven with custom handlers");
    println!("3. Hybrid approach: Choose between channels and callbacks per use case");
    println!("4. Advanced patterns: Middleware-like processing with callbacks");
    println!("5. Both approaches coexist: You can use channels by default and add callbacks when needed");
    
    Ok(())
}

// Helper function to demonstrate callback patterns
fn demonstrate_callback_patterns() {
    println!("\n🎯 Callback Pattern Examples:");
    println!("1. Event Logging: Track all stream events for debugging");
    println!("2. Middleware Processing: Validate/transform data before processing");
    println!("3. Metrics Collection: Collect statistics on stream performance");
    println!("4. Error Handling: Custom error processing and recovery");
    println!("5. Flow Control: Dynamic backpressure management");
    println!("6. Security: Real-time security monitoring and threat detection");
    println!("7. Caching: Intelligent caching based on stream patterns");
    println!("8. Routing: Dynamic routing based on stream content");
}
