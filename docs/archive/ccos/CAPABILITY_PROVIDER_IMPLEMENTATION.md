# CCOS Capability Implementation Guide

**Status:** ✅ **REFACTORED** – v2.0 (Two-Component Architecture)

## Overview

This guide provides implementation details for the CCOS Capability Architecture, showing how different capability types are defined and executed within the RTFS runtime. The system uses a `ProviderType` enum within a `CapabilityManifest` to determine the execution strategy for any given capability.

## Core Implementation: `ProviderType`

The `ProviderType` enum is the core of the capability implementation model. It specifies whether a capability is local, remote via HTTP, a data stream, or another type. The `CapabilityMarketplace` uses this enum to decide how to handle an execution request.

### ProviderType Enum Definition

```rust
/// Enum defining the different types of capability providers.
#[derive(Clone, Debug)]
pub enum ProviderType {
    /// A local capability, executed by the `CapabilityRegistry`.
    /// This type is a unit-like variant because the registry manages the implementation details internally.
    Local,

    /// A remote capability accessed over HTTP.
    /// It holds the necessary configuration to make the HTTP request.
    Http(HttpCapability),

    /// A streaming capability for continuous data flow.
    /// It holds the implementation for the streaming logic.
    Stream(StreamCapabilityImpl),

    /// A capability that communicates using the Model Context Protocol (MCP).
    MCP(MCPCapability),

    /// A capability for agent-to-agent (A2A) communication.
    A2A(A2ACapability),
}
```

### Supporting Structs

#### `HttpCapability`

This struct contains all the necessary information to connect to and authenticate with a remote HTTP-based capability.

```rust
/// Configuration for an HTTP-based remote capability.
#[derive(Debug, Clone)]
pub struct HttpCapability {
    pub base_url: String,
    pub auth_token: Option<String>,
    pub timeout_ms: u64,
}
```

#### `MCPCapability`

This struct holds the configuration for a capability that communicates using the Model Context Protocol (MCP).

```rust
/// Configuration for a capability that uses the Model Context Protocol (MCP).
#[derive(Debug, Clone)]
pub struct MCPCapability {
    pub server_url: String,
    pub tool_name: String,
}
```

#### `A2ACapability`

This struct holds the configuration for an agent-to-agent (A2A) communication capability.

```rust
/// Configuration for an agent-to-agent (A2A) communication capability.
#[derive(Debug, Clone)]
pub struct A2ACapability {
    pub agent_id: String,
    pub endpoint: String,
}
```

#### `StreamCapabilityImpl`

This struct defines a streaming capability, including its type, data schemas, and the underlying provider that handles the stream's logic.

```rust
/// Streaming capability implementation details
#[derive(Clone)]
pub struct StreamCapabilityImpl {
    pub provider: StreamingProvider,
    pub stream_type: StreamType,
    pub input_schema: Option<String>, // JSON schema for input validation
    pub output_schema: Option<String>, // JSON schema for output validation
    pub supports_progress: bool,
    pub config: StreamConfig,
}

/// Trait for a provider of a streaming capability
#[async_trait]
pub trait StreamingProvider: Send + Sync {
    async fn connect(&self, options: Value) -> RuntimeResult<()>;
    async fn disconnect(&self) -> RuntimeResult<()>;
    async fn send(&self, data: Value) -> RuntimeResult<()>;
    // ... other methods
}
```

## Execution Flow

When a `(call ...)` expression is evaluated, the following steps occur:

1.  The `CapabilityMarketplace` looks up the `CapabilityManifest` for the requested capability (e.g., `:ccos.http.fetch`).
2.  It inspects the `provider_type` field within the manifest.
3.  **If `ProviderType::Local`**: The marketplace delegates the execution request to the `CapabilityRegistry`, which runs the capability in a secure, sandboxed environment.
4.  **If `ProviderType::Http(http_config)`**: The marketplace uses the `http_config` data to build, send, and handle the response of an HTTP request. This is handled within the marketplace's own `execute_http_capability` method.
5.  **If `ProviderType::Stream(stream_impl)`**: The marketplace uses the `stream_impl` to establish and manage a data stream, handling the connection and data flow asynchronously.
6.  **If `ProviderType::MCP(mcp_config)` or `ProviderType::A2A(a2a_config)`**: The execution for these types is planned. The marketplace will use the configuration to orchestrate communication with the respective protocols.

This model provides a clear separation of concerns, where the `Marketplace` acts as a high-level orchestrator and the `Registry` serves as a secure execution engine for trusted, built-in functions.
