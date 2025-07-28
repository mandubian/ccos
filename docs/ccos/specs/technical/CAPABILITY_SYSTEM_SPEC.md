# RTFS Capability System Specification

**Version:** 3.0
**Status:** ✅ **UPDATED** - Official MCP SDK Integration and CapabilityExecutor Pattern

---

## 1. Overview

This document specifies the architecture and implementation of the RTFS Capability System. The system is designed to provide a secure, extensible, and robust mechanism for executing a wide range of operations, from simple, sandboxed computations to complex, asynchronous interactions with external systems.

The core of the system is a **three-component architecture** that separates high-level orchestration, extensible execution, and low-level secure execution:

1. **CapabilityMarketplace**: High-level orchestration and discovery
2. **CapabilityExecutor Pattern**: Extensible execution framework
3. **CapabilityRegistry**: Low-level, secure execution engine for built-in capabilities

## 2. Core Components

### 2.1. `CapabilityMarketplace`

The `CapabilityMarketplace` is the primary, high-level interface for the RTFS runtime to interact with capabilities.

**Responsibilities:**

*   **Discovery**: Provides mechanisms to discover and list available capabilities.
*   **Orchestration**: Acts as the main entry point for all `(call ...)` operations. It inspects the capability type and determines the correct execution path.
*   **Executor Management**: Manages registered `CapabilityExecutor` instances for extensible capability execution.
*   **High-Level Capability Execution**: Directly handles the execution of capabilities that are asynchronous or involve I/O. This includes:
    *   HTTP-based remote capabilities.
    *   MCP (Model Context Protocol) capabilities using the official Rust SDK.
    *   A2A (Agent-to-Agent) capabilities.
    *   Plugin-based capabilities.
    *   Streaming capabilities.
*   **Delegation**: Forwards requests for local, built-in capabilities to the `CapabilityRegistry`.

### 2.2. `CapabilityExecutor` Pattern

The `CapabilityExecutor` pattern provides an extensible framework for executing different types of capabilities. This pattern allows for:

*   **Extensibility**: New capability types can be added without modifying the core marketplace.
*   **Separation of Concerns**: Each executor handles its specific provider type.
*   **Testability**: Executors can be tested independently.
*   **Plugin Architecture**: Third-party executors can be registered dynamically.

**Key Components:**

```rust
pub trait CapabilityExecutor: Send + Sync {
    fn provider_type_id(&self) -> TypeId;
    async fn execute(&self, provider: &ProviderType, inputs: &Value) -> RuntimeResult<Value>;
}
```

**Built-in Executors:**

*   **MCPExecutor**: Uses the official MCP Rust SDK for Model Context Protocol communication
*   **A2AExecutor**: Handles Agent-to-Agent communication across multiple protocols
*   **LocalExecutor**: Executes local, in-process capabilities
*   **HttpExecutor**: Handles HTTP-based remote capabilities
*   **PluginExecutor**: Manages plugin-based capabilities
*   **RemoteRTFSExecutor**: Handles remote RTFS system communication
*   **StreamExecutor**: Manages streaming capabilities

### 2.3. `CapabilityRegistry`

The `CapabilityRegistry` is the low-level, secure execution engine for a curated set of built-in, sandboxed capabilities.

**Responsibilities:**

*   **Secure Execution**: Executes trusted, built-in functions in a controlled environment, without direct access to I/O (e.g., file system, network).
*   **Performance**: Optimized for fast, synchronous execution of core functionalities (e.g., math operations, data manipulation).
*   **Isolation**: Is completely decoupled from the `CapabilityMarketplace` and other high-level components to ensure a secure sandbox.

## 3. Core Data Structures

The system relies on a set of key data structures to describe and execute capabilities.

### 3.1. `CapabilityManifest`

The `CapabilityManifest` is a public-facing data structure that describes a capability to the system.

```rust
/// Describes a capability and how to execute it.
#[derive(Debug, Clone)]
pub struct CapabilityManifest {
    pub id: String,
    pub name: String,
    pub description: String,
    /// The specific provider type that implements the capability.
    pub provider: ProviderType,
    pub version: String,
    pub input_schema: Option<TypeExpr>,  // RTFS type expression for input validation
    pub output_schema: Option<TypeExpr>, // RTFS type expression for output validation
    pub attestation: Option<CapabilityAttestation>,
    pub provenance: Option<CapabilityProvenance>,
    pub permissions: Vec<String>,
    pub metadata: std::collections::HashMap<String, String>,
}
```

### 3.2. `ProviderType` Enum

This enum is the central mechanism that determines the execution strategy for a capability.

```rust
/// Enum defining the different types of capability providers.
#[derive(Clone, Debug)]
pub enum ProviderType {
    /// A local capability, executed by the `CapabilityRegistry`.
    Local(LocalCapability),

    /// A remote capability accessed over HTTP.
    Http(HttpCapability),

    /// A capability that communicates using the Model Context Protocol (MCP).
    /// Uses the official MCP Rust SDK for communication.
    MCP(MCPCapability),

    /// A capability for agent-to-agent (A2A) communication.
    A2A(A2ACapability),

    /// A plugin-based capability.
    Plugin(PluginCapability),

    /// A remote RTFS instance capability.
    RemoteRTFS(RemoteRTFSCapability),

    /// A streaming capability for continuous data flow.
    Stream(StreamCapabilityImpl),
}
```

### 3.3. Supporting Structs

#### `MCPCapability`

Contains the configuration for a capability that uses the Model Context Protocol (MCP) with the official Rust SDK.

```rust
/// Configuration for a capability that uses the Model Context Protocol (MCP).
#[derive(Debug, Clone)]
pub struct MCPCapability {
    pub server_url: String,
    pub tool_name: String,
    pub timeout_ms: u64,
}
```

**MCP SDK Integration:**

The MCP capability implementation uses the official [MCP Rust SDK](https://github.com/modelcontextprotocol/rust-sdk) with the following features:

*   **Client Creation**: Uses `rmcp::client::Client` with `TokioChildProcess` transport
*   **Tool Discovery**: Automatic tool discovery via `client.list_tools().await?`
*   **Tool Execution**: Executes tools via `client.call_tool(tool_params).await?`
*   **Content Handling**: Supports both structured and unstructured content from MCP responses
*   **Error Handling**: Comprehensive error handling for MCP protocol errors

**Example MCP Execution:**

```rust
// Create MCP client using child process transport
let client = Client::new(TokioChildProcess::new(
    tokio::process::Command::new("npx")
        .configure(|cmd| {
            cmd.arg("-y")
               .arg("@modelcontextprotocol/server-everything");
        })
)?).await?;

// Discover available tools
let tools_result = client.list_tools().await?;

// Execute tool call
let tool_params = CallToolRequestParam {
    name: tool_name,
    arguments: input_json.as_object().cloned().unwrap_or_default(),
};
let result = client.call_tool(tool_params).await?;
```

#### `A2ACapability`

Contains the configuration for an agent-to-agent (A2A) communication capability.

```rust
/// Configuration for an agent-to-agent (A2A) communication capability.
#[derive(Debug, Clone)]
pub struct A2ACapability {
    pub agent_id: String,
    pub endpoint: String,
    pub protocol: String, // "http", "websocket", "grpc"
    pub timeout_ms: u64,
}
```

#### `HttpCapability`

Contains the configuration for an HTTP-based remote capability.

```rust
/// Configuration for an HTTP-based remote capability.
#[derive(Debug, Clone)]
pub struct HttpCapability {
    pub base_url: String,
    pub auth_token: Option<String>,
    pub timeout_ms: u64,
}
```

#### `PluginCapability`

Contains the configuration for a plugin-based capability.

```rust
/// Configuration for a plugin-based capability.
#[derive(Debug, Clone)]
pub struct PluginCapability {
    pub plugin_path: String,
    pub function_name: String,
}
```

#### `RemoteRTFSCapability`

Contains the configuration for a remote RTFS instance capability.

```rust
/// Configuration for a remote RTFS instance capability.
#[derive(Debug, Clone)]
pub struct RemoteRTFSCapability {
    pub endpoint: String,
    pub timeout_ms: u64,
    pub auth_token: Option<String>,
}
```

## 4. Execution Flow

When an RTFS script executes a `(call ...)` expression, the following sequence of events occurs:

1.  The RTFS `evaluator` invokes the `CapabilityMarketplace` with the capability ID and its arguments.
2.  The `CapabilityMarketplace` performs a lookup in its collection of `CapabilityManifest`s using the provided ID.
3.  If found, it inspects the `provider` field of the manifest.
4.  Based on the `ProviderType`, it proceeds as follows:
    *   **`ProviderType::Local`**: The `CapabilityMarketplace` delegates the call (ID and arguments) to the `CapabilityRegistry`. The `CapabilityRegistry` executes the corresponding built-in function in its secure, sandboxed environment and returns the result.
    *   **`ProviderType::Http(http_config)`**: The `CapabilityMarketplace` uses the `http_config` data and the call arguments to construct and send an asynchronous HTTP request. It awaits the response, processes it, and returns the result. This is handled by the `HttpExecutor` within the marketplace.
    *   **`ProviderType::MCP(mcp_config)`**: The `CapabilityMarketplace` uses the `MCPExecutor` to communicate with MCP servers using the official Rust SDK. This includes tool discovery, tool execution, and response handling.
    *   **`ProviderType::A2A(a2a_config)`**: The `CapabilityMarketplace` uses the `A2AExecutor` to orchestrate communication with other agents across multiple protocols (HTTP, WebSocket, gRPC).
    *   **`ProviderType::Plugin(plugin_config)`**: The `CapabilityMarketplace` uses the `PluginExecutor` to execute external plugins via subprocess communication.
    *   **`ProviderType::RemoteRTFS(remote_rtfs_config)`**: The `CapabilityMarketplace` uses the `RemoteRTFSExecutor` to communicate with remote RTFS instances.
    *   **`ProviderType::Stream(stream_impl)`**: The `CapabilityMarketplace` uses the `StreamExecutor` to establish and manage data streams, handling the connection and data flow asynchronously.

## 5. Extensibility with CapabilityExecutor Pattern

The `CapabilityExecutor` pattern provides a powerful mechanism for extending the capability system:

### 5.1. Registering Executors

```rust
// Register built-in executors
marketplace.register_executor(Arc::new(MCPExecutor));
marketplace.register_executor(Arc::new(A2AExecutor));
marketplace.register_executor(Arc::new(LocalExecutor));
marketplace.register_executor(Arc::new(HttpExecutor));
```

### 5.2. Custom Executors

Developers can create custom executors for new capability types:

```rust
pub struct CustomExecutor;

#[async_trait(?Send)]
impl CapabilityExecutor for CustomExecutor {
    fn provider_type_id(&self) -> TypeId {
        TypeId::of::<CustomCapability>()
    }

    async fn execute(&self, provider: &ProviderType, inputs: &Value) -> RuntimeResult<Value> {
        if let ProviderType::Custom(custom) = provider {
            // Custom execution logic
            Ok(Value::String("Custom result".to_string()))
        } else {
            Err(RuntimeError::Generic("Invalid provider type".to_string()))
        }
    }
}
```

### 5.3. Dynamic Registration

Executors can be registered and unregistered at runtime:

```rust
// Register a custom executor
marketplace.register_executor(Arc::new(CustomExecutor));

// The marketplace will automatically use the registered executor
// for capabilities with the matching provider type
```

## 6. Security Model

The three-component architecture is fundamental to the system's security:

*   **Principle of Least Privilege**: The `CapabilityRegistry` only has the logic to execute a small, trusted set of core functions. It has no access to networking, the file system, or other sensitive host resources.
*   **Clear Security Boundary**: All potentially risky operations (any form of I/O) are handled exclusively by the `CapabilityMarketplace` and its executors. This provides a single place to enforce security policies, logging, and auditing for external interactions.
*   **Sandboxing**: By design, all `Local` capabilities are sandboxed within the `CapabilityRegistry`.
*   **Executor Isolation**: Each executor operates in isolation, preventing cross-contamination between different capability types.

## 7. MCP SDK Integration Details

### 7.1. Dependencies

The system uses the official MCP Rust SDK:

```toml
rmcp = { version = "0.3.0", features = ["client", "server", "transport-stdio", "transport-child-process"] }
```

### 7.2. Transport Configuration

The MCP client is configured to use child process transport for maximum compatibility:

```rust
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParam, CallToolResult, Content, Tool, ListToolsResult},
    transport::{ConfigureCommandExt, TokioChildProcess},
    client::Client,
};
```

### 7.3. Tool Discovery and Execution

The MCP implementation provides:

*   **Automatic Tool Discovery**: When no specific tool is specified, the system automatically discovers available tools
*   **Tool Selection**: Supports both specific tool names and wildcard tool selection
*   **Structured Content**: Handles both structured and unstructured MCP responses
*   **Error Handling**: Comprehensive error handling for MCP protocol errors

## 8. Extensibility

The system is designed to be easily extended, particularly with new capability types and executors. To add a new capability type:

1.  Define a new provider type in the `ProviderType` enum.
2.  Create a corresponding configuration struct.
3.  Implement a `CapabilityExecutor` for the new provider type.
4.  Register the executor with the `CapabilityMarketplace` at startup.

No changes are needed to the secure `CapabilityRegistry` to add new remote functionalities.

## 9. Testing and Validation

The system includes comprehensive testing support:

*   **Unit Tests**: Each executor can be tested independently
*   **Integration Tests**: End-to-end capability execution testing
*   **Mock Executors**: Mock implementations for testing without external dependencies
*   **Schema Validation**: Input/output validation using RTFS type expressions

## 10. Performance Considerations

*   **Executor Caching**: Executors are cached by type ID for efficient lookup
*   **Async Execution**: All external operations are asynchronous
*   **Connection Pooling**: HTTP clients use connection pooling
*   **MCP Optimization**: MCP clients are optimized for tool discovery and execution
*   **Memory Management**: Efficient memory usage with proper resource cleanup
