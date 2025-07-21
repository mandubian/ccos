# Bidirectional Streaming in RTFS Capability Marketplace

## Overview

The RTFS Capability Marketplace provides comprehensive streaming support with five distinct streaming patterns, including elegant bidirectional streaming inspired by MCP (Model Context Protocol) patterns. This document explains how to use each streaming pattern and provides practical examples.

## Analysis and Motivation

### Why Bidirectional Streams?

Traditional streaming systems often only support unidirectional data flow or require complex setup for bidirectional communication. Inspired by MCP's streaming patterns, we designed a unified streaming architecture that provides:

1. **Elegant Simplicity**: All streaming patterns follow consistent APIs
2. **Type Safety**: Rust's type system ensures stream safety at compile time
3. **Flow Control**: Built-in backpressure and buffering mechanisms
4. **Direction Awareness**: Explicit tracking of data flow direction
5. **Correlation Support**: Request-response patterns with correlation IDs

### Technical Architecture

The streaming system is built around several key components:

- **StreamType**: Defines the five streaming patterns
- **StreamingProvider**: Handles the actual transport (WebSocket, HTTP, SSE, Local)
- **StreamItem**: Wraps data with metadata, direction, and correlation
- **Stream Configurations**: Pattern-specific configuration structures

## Streaming Patterns

### Design Philosophy: Channels by Default, Callbacks When Needed

The RTFS streaming architecture follows a **dual-approach design**:

1. **Channel-based streaming (default)**: Traditional `mpsc::Sender`/`mpsc::Receiver` pattern for direct data flow
2. **Callback-based streaming (optional)**: Event-driven callbacks for custom behavior and middleware patterns

This design allows you to:
- Use familiar channel patterns for simple streaming needs
- Add callbacks only when you need custom event handling
- Combine both approaches in hybrid scenarios
- Maintain backward compatibility with existing channel-based code

### Stream Event System

When callbacks are enabled, the streaming system generates events that can trigger custom behavior:

```rust
pub enum StreamEvent {
    Connected { stream_id: String, metadata: HashMap<String, String> },
    Disconnected { stream_id: String, reason: String },
    DataReceived { stream_id: String, item: StreamItem },
    DataSent { stream_id: String, item: StreamItem },
    Error { stream_id: String, error: String },
    Progress { stream_id: String, progress: ProgressNotification },
    BackpressureTriggered { stream_id: String, buffer_size: usize },
    BackpressureRelieved { stream_id: String, buffer_size: usize },
}
```

### Stream Configuration

```rust
pub struct StreamConfig {
    pub buffer_size: usize,           // Buffer size for the stream
    pub enable_callbacks: bool,       // Enable/disable callbacks (default: false)
    pub callbacks: StreamCallbacks,   // Callback handlers
    pub metadata: HashMap<String, String>, // Custom metadata
}
```

### Callback Registration

```rust
pub struct StreamCallbacks {
    pub on_connected: Option<StreamCallback>,
    pub on_disconnected: Option<StreamCallback>,
    pub on_data_received: Option<StreamCallback>,
    pub on_data_sent: Option<StreamCallback>,
    pub on_error: Option<StreamCallback>,
    pub on_progress: Option<StreamCallback>,
    pub on_backpressure: Option<StreamCallback>,
}
```

### 1. Source Stream (Unidirectional Outbound)

**Use Case**: Server-sent events, RSS feeds, log streams, metrics  
**Direction**: System → External  
**Example**: Real-time notifications, monitoring dashboards

```rust
marketplace.register_streaming_capability(
    "events.sse".to_string(),
    "Real-time Updates".to_string(),
    "Server-sent events for real-time updates".to_string(),
    StreamType::Source,
    StreamingProvider::ServerSentEvents {
        url: "http://localhost:8082/events".to_string(),
        headers: HashMap::from([("Authorization".to_string(), "Bearer token123".to_string())]),
    },
).await?;
```

### 2. Sink Stream (Unidirectional Inbound)

**Use Case**: Log collection, metrics ingestion, data upload  
**Direction**: External → System  
**Example**: Collecting telemetry data, receiving file uploads

```rust
marketplace.register_streaming_capability(
    "logs.collector".to_string(),
    "Log Collection".to_string(),
    "Collect application logs".to_string(),
    StreamType::Sink,
    StreamingProvider::Http {
        url: "http://localhost:8081/logs".to_string(),
        method: "POST".to_string(),
        headers: HashMap::from([("Content-Type".to_string(), "application/json".to_string())]),
    },
).await?;
```

### 3. Transform Stream (Unidirectional with Processing)

**Use Case**: Data processing, format conversion, filtering  
**Direction**: Input → Process → Output  
**Example**: JSON transformation, data validation, format conversion

```rust
marketplace.register_streaming_capability(
    "transform.json".to_string(),
    "JSON Transform".to_string(),
    "Transform JSON data in real-time".to_string(),
    StreamType::Transform,
    StreamingProvider::Local {
        buffer_size: 200,
    },
).await?;
```

### 4. Bidirectional Stream (Simultaneous Send/Receive)

**Use Case**: Chat applications, collaborative editing, real-time games  
**Direction**: Both directions simultaneously  
**Example**: WebSocket chat, collaborative document editing

```rust
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
let (sender, receiver) = marketplace.start_bidirectional_stream(
    "chat.websocket",
    &Value::Map(HashMap::new()),
).await?;
```

### 5. Duplex Stream (Separate Input/Output Channels)

**Use Case**: Data processing pipelines, request-response patterns  
**Direction**: Separate channels for input and output  
**Example**: Data processing pipeline, API gateway

```rust
let duplex_config = DuplexChannels {
    input_channel: StreamChannelConfig {
        buffer_size: 1000,
        metadata: HashMap::from([("purpose".to_string(), "data-ingestion".to_string())]),
    },
    output_channel: StreamChannelConfig {
        buffer_size: 500,
        metadata: HashMap::from([("purpose".to_string(), "processed-results".to_string())]),
    },
};

marketplace.register_duplex_stream_capability(
    "data.processor".to_string(),
    "Data Processing Pipeline".to_string(),
    "Duplex stream for data processing with separate input/output channels".to_string(),
    StreamingProvider::Http {
        url: "http://localhost:8081/process".to_string(),
        method: "POST".to_string(),
        headers: HashMap::from([("Content-Type".to_string(), "application/json".to_string())]),
    },
    duplex_config,
).await?;
```

## Key Components

### StreamItem Structure

Every piece of data in the streaming system is wrapped in a `StreamItem`:

```rust
pub struct StreamItem {
    pub data: Value,                              // The actual data
    pub sequence: u64,                            // Sequence number for ordering
    pub timestamp: u64,                           // Unix timestamp
    pub metadata: HashMap<String, String>,        // Additional metadata
    pub direction: StreamDirection,               // Flow direction
    pub correlation_id: Option<String>,           // For request-response patterns
}
```

### StreamDirection Enum

Tracks the direction of data flow:

```rust
pub enum StreamDirection {
    Inbound,        // Data flowing into the system
    Outbound,       // Data flowing out of the system
    Bidirectional,  // Data can flow in both directions
}
```

### StreamingProvider Types

Multiple transport mechanisms are supported:

```rust
pub enum StreamingProvider {
    WebSocket {
        url: String,
        protocols: Vec<String>,
    },
    Http {
        url: String,
        method: String,
        headers: HashMap<String, String>,
    },
    ServerSentEvents {
        url: String,
        headers: HashMap<String, String>,
    },
    Local {
        buffer_size: usize,
    },
}
```

## Configuration Options

### BidirectionalConfig

```rust
pub struct BidirectionalConfig {
    pub input_buffer_size: usize,   // Buffer size for incoming data
    pub output_buffer_size: usize,  // Buffer size for outgoing data  
    pub flow_control: bool,         // Enable backpressure handling
    pub timeout_ms: u64,            // Connection timeout
}
```

### DuplexChannels

```rust
pub struct DuplexChannels {
    pub input_channel: StreamChannelConfig,
    pub output_channel: StreamChannelConfig,
}

pub struct StreamChannelConfig {
    pub buffer_size: usize,
    pub metadata: HashMap<String, String>,
}
```

## Usage Examples

### Basic Bidirectional Stream

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let marketplace = CapabilityMarketplace::new();
    
    // Register a bidirectional chat capability
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
    
    // Start the stream
    let (sender, receiver) = marketplace.start_bidirectional_stream(
        "chat.websocket",
        &Value::Map(HashMap::new()),
    ).await?;
    
    // Send a message
    let message = StreamItem {
        data: Value::String("Hello, world!".to_string()),
        sequence: 1,
        timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs(),
        metadata: HashMap::new(),
        direction: StreamDirection::Outbound,
        correlation_id: Some("msg-001".to_string()),
    };
    
    sender.send(message).await?;
    
    // Receive messages (in practice, this would be in a loop)
    if let Ok(received) = receiver.recv().await {
        println!("Received: {:?}", received);
    }
    
    Ok(())
}
```

### Duplex Data Processing

```rust
// Start a duplex stream for data processing
let duplex_channels = marketplace.start_duplex_stream(
    "data.processor",
    &Value::Map(HashMap::new()),
).await?;

// Send data for processing
let data_item = StreamItem {
    data: Value::Map(HashMap::from([
        (MapKey::String("operation".to_string()), Value::String("analyze".to_string())),
        (MapKey::String("data".to_string()), Value::Vector(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
        ])),
    ])),
    sequence: 1,
    timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs(),
    metadata: HashMap::new(),
    direction: StreamDirection::Inbound,
    correlation_id: Some("proc-001".to_string()),
};

duplex_channels.input_sender.send(data_item).await?;

// Receive processed results
if let Ok(result) = duplex_channels.output_receiver.recv().await {
    println!("Processed result: {:?}", result);
}
```

### Channel-based Usage (Default)

```rust
// Traditional channel-based approach
let (sender, receiver) = marketplace.start_bidirectional_stream(
    "chat.websocket",
    &Value::Map(HashMap::new()),
).await?;

// Send data through channel
sender.send(message).await?;

// Receive data from channel
if let Some(item) = receiver.recv().await {
    println!("Received: {:?}", item);
}
```

### Callback-based Usage

```rust
// Define callbacks for specific events
let on_data_received = Arc::new(|event: StreamEvent| -> Result<(), RuntimeError> {
    if let StreamEvent::DataReceived { stream_id, item } = event {
        println!("Stream '{}' received: {:?}", stream_id, item.data);
        // Custom processing logic here
    }
    Ok(())
});

// Configure stream with callbacks
let config = StreamConfig {
    buffer_size: 100,
    enable_callbacks: true,
    callbacks: StreamCallbacks {
        on_data_received: Some(on_data_received),
        ..Default::default()
    },
    metadata: HashMap::new(),
};

// Start stream with callback configuration
let stream_handle = marketplace.start_bidirectional_stream_with_config(
    "chat.websocket",
    &Value::Map(HashMap::new()),
    &config,
).await?;

// Send data (triggers callbacks automatically)
stream_handle.send(message).await?;
```

### Hybrid Approach

```rust
// Use channels for data flow, callbacks for events
let config = StreamConfig {
    buffer_size: 100,
    enable_callbacks: false,  // Disable callbacks for data flow
    callbacks: StreamCallbacks::default(),
    metadata: HashMap::new(),
};

let stream_handle = marketplace.start_stream_with_config(
    "data.stream",
    &Value::Map(HashMap::new()),
    &config,
).await?;

// Access the underlying channel for data
if let Some(mut receiver) = stream_handle.receiver {
    while let Some(item) = receiver.recv().await {
        // Process data through channels
        println!("Channel data: {:?}", item);
    }
}
```

### Middleware Pattern with Callbacks

```rust
// Middleware-like callback for data validation/transformation
let middleware_callback = Arc::new(|event: StreamEvent| -> Result<(), RuntimeError> {
    if let StreamEvent::DataReceived { stream_id, item } = event {
        // Validate data
        if let Value::String(content) = &item.data {
            if content.contains("blocked_word") {
                return Err(RuntimeError::Generic("Content blocked".to_string()));
            }
        }
        
        // Log successful validation
        println!("✅ Data validated for stream '{}'", stream_id);
    }
    Ok(())
});

let middleware_config = StreamConfig {
    buffer_size: 200,
    enable_callbacks: true,
    callbacks: StreamCallbacks {
        on_data_received: Some(middleware_callback),
        ..Default::default()
    },
    metadata: HashMap::from([("pattern".to_string(), "middleware".to_string())]),
};
```

## Callback Use Cases

### 1. Event Logging and Monitoring
```rust
let logging_callback = Arc::new(|event: StreamEvent| -> Result<(), RuntimeError> {
    match event {
        StreamEvent::Connected { stream_id, .. } => {
            println!("[LOG] Stream {} connected", stream_id);
        }
        StreamEvent::DataReceived { stream_id, item } => {
            println!("[LOG] Stream {} received data: seq={}", stream_id, item.sequence);
        }
        StreamEvent::Error { stream_id, error } => {
            eprintln!("[ERROR] Stream {} error: {}", stream_id, error);
        }
        _ => {}
    }
    Ok(())
});
```

### 2. Metrics Collection
```rust
let metrics_callback = Arc::new(|event: StreamEvent| -> Result<(), RuntimeError> {
    match event {
        StreamEvent::DataReceived { stream_id, item } => {
            // Update metrics
            METRICS.increment_counter(&format!("stream.{}.messages_received", stream_id));
            METRICS.record_histogram(&format!("stream.{}.message_size", stream_id), item.data.size());
        }
        StreamEvent::BackpressureTriggered { stream_id, .. } => {
            METRICS.increment_counter(&format!("stream.{}.backpressure_events", stream_id));
        }
        _ => {}
    }
    Ok(())
});
```

### 3. Security Monitoring
```rust
let security_callback = Arc::new(|event: StreamEvent| -> Result<(), RuntimeError> {
    if let StreamEvent::DataReceived { stream_id, item } = event {
        // Check for suspicious patterns
        if let Value::String(content) = &item.data {
            if content.contains("malicious_pattern") {
                return Err(RuntimeError::Generic(format!(
                    "Security violation detected in stream {}", stream_id
                )));
            }
        }
    }
    Ok(())
});
```

### 4. Dynamic Flow Control
```rust
let flow_control_callback = Arc::new(|event: StreamEvent| -> Result<(), RuntimeError> {
    match event {
        StreamEvent::BackpressureTriggered { stream_id, buffer_size } => {
            println!("⚠️ Applying flow control to stream {}", stream_id);
            // Implement custom backpressure logic
        }
        StreamEvent::BackpressureRelieved { stream_id, .. } => {
            println!("✅ Flow control released for stream {}", stream_id);
        }
        _ => {}
    }
    Ok(())
});
```

### 5. Data Transformation Pipeline
```rust
let transform_callback = Arc::new(|event: StreamEvent| -> Result<(), RuntimeError> {
    if let StreamEvent::DataReceived { stream_id, item } = event {
        // Transform data before processing
        let transformed_data = match &item.data {
            Value::String(s) => Value::String(s.to_uppercase()),
            Value::Integer(i) => Value::Integer(i * 2),
            other => other.clone(),
        };
        
        println!("Transformed data in stream {}: {:?}", stream_id, transformed_data);
    }
    Ok(())
});
```

## When to Use Callbacks vs Channels

### Use Channels When:
- Simple data flow is sufficient
- You need traditional producer/consumer patterns
- Performance is critical (channels have lower overhead)
- You're building straightforward streaming applications

### Use Callbacks When:
- You need custom event handling
- Implementing middleware or plugin patterns
- Building monitoring/logging systems
- Need to react to stream lifecycle events
- Implementing complex flow control
- Building security or validation layers

### Use Hybrid Approach When:
- You want efficient data flow with selective event handling
- Building systems that need both patterns
- Migrating from channel-based to event-driven architecture
- Different parts of your system have different requirements

## Best Practices for Callbacks

1. **Keep Callbacks Lightweight**: Avoid heavy processing in callbacks
2. **Handle Errors Gracefully**: Always return appropriate errors from callbacks
3. **Use Arc for Shared State**: Callbacks must be thread-safe
4. **Avoid Blocking Operations**: Use async patterns when possible
5. **Consider Performance**: Callbacks add overhead, use judiciously
6. **Test Callback Logic**: Ensure callbacks don't introduce bugs
7. **Document Callback Behavior**: Make callback effects clear

## Performance Considerations

- **Channel-only**: Lowest overhead, best for high-throughput scenarios
- **Callback-only**: Higher overhead, best for event-driven scenarios
- **Hybrid**: Balanced approach, use callbacks selectively

The streaming architecture automatically optimizes based on configuration:
- Callbacks disabled: Pure channel-based operation
- Callbacks enabled: Event-driven operation with channel access
- Hybrid: Channels for data, callbacks for events
