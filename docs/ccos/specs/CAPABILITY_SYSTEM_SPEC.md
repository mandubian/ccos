# RTFS Capability System Specification

**Version:** 2.0
**Status:** ✅ **FINALIZED**

---

## 1. Overview

This document specifies the architecture and implementation of the RTFS Capability System. The system is designed to provide a secure, extensible, and robust mechanism for executing a wide range of operations, from simple, sandboxed computations to complex, asynchronous interactions with external systems.

The core of the system is a **two-component architecture** that separates high-level, potentially I/O-bound operations from low-level, security-sensitive, built-in functions.

## 2. Core Components

### 2.1. `CapabilityMarketplace`

The `CapabilityMarketplace` is the primary, high-level interface for the RTFS runtime to interact with capabilities.

**Responsibilities:**

*   **Discovery**: Provides mechanisms to discover and list available capabilities.
*   **Orchestration**: Acts as the main entry point for all `(call ...)` operations. It inspects the capability type and determines the correct execution path.
*   **High-Level Capability Execution**: Directly handles the execution of capabilities that are asynchronous or involve I/O. This includes:
    *   HTTP-based remote capabilities.
    *   Streaming capabilities.
*   **Delegation**: Forwards requests for local, built-in capabilities to the `CapabilityRegistry`.

### 2.2. `CapabilityRegistry`

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
    pub provider_type: ProviderType,
}
```

### 3.2. `ProviderType` Enum

This enum is the central mechanism that determines the execution strategy for a capability.

```rust
/// Enum defining the different types of capability providers.
#[derive(Clone, Debug)]
pub enum ProviderType {
    /// A local capability, executed by the `CapabilityRegistry`.
    /// This is a unit-like variant as the registry manages implementation details internally.
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

### 3.3. Supporting Structs

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

#### `MCPCapability`

Contains the configuration for a capability that uses the Model Context Protocol (MCP).

```rust
/// Configuration for a capability that uses the Model Context Protocol (MCP).
#[derive(Debug, Clone)]
pub struct MCPCapability {
    pub server_url: String,
    pub tool_name: String,
}
```

#### `A2ACapability`

Contains the configuration for an agent-to-agent (A2A) communication capability.

```rust
/// Configuration for an agent-to-agent (A2A) communication capability.
#[derive(Debug, Clone)]
pub struct A2ACapability {
    pub agent_id: String,
    pub endpoint: String,
}
```

#### `StreamCapabilityImpl`

Defines a streaming capability, including its type, data schemas, and the underlying provider that handles the stream's logic.

```rust
/// Streaming capability implementation details
#[derive(Clone)]
pub struct StreamCapabilityImpl {
    pub provider: StreamingProvider,
    pub stream_type: StreamType,
    pub input_schema: Option<String>,
    pub output_schema: Option<String>,
    pub supports_progress: bool,
    pub config: StreamConfig,
}
```

## 4. Execution Flow

When an RTFS script executes a `(call ...)` expression, the following sequence of events occurs:

1.  The RTFS `evaluator` invokes the `CapabilityMarketplace` with the capability ID and its arguments.
2.  The `CapabilityMarketplace` performs a lookup in its collection of `CapabilityManifest`s using the provided ID.
3.  If found, it inspects the `provider_type` field of the manifest.
4.  Based on the `ProviderType`, it proceeds as follows:
    *   **`ProviderType::Local`**: The `CapabilityMarketplace` delegates the call (ID and arguments) to the `CapabilityRegistry`. The `CapabilityRegistry` executes the corresponding built-in function in its secure, sandboxed environment and returns the result.
    *   **`ProviderType::Http(http_config)`**: The `CapabilityMarketplace` uses the `http_config` data and the call arguments to construct and send an asynchronous HTTP request. It awaits the response, processes it, and returns the result. This is handled by the `execute_http_capability` method within the marketplace.
    *   **`ProviderType::Stream(stream_impl)`**: The `CapabilityMarketplace` uses the `stream_impl` to establish and manage a data stream, handling the connection and data flow asynchronously.
    *   **`ProviderType::MCP(mcp_config)`** and **`ProviderType::A2A(a2a_config)`**: The execution for these planned types will be handled by the `CapabilityMarketplace`. It will use the respective configuration objects to orchestrate communication with MCP servers or other agents.

## 5. Security Model

The two-component architecture is fundamental to the system's security:

*   **Principle of Least Privilege**: The `CapabilityRegistry` only has the logic to execute a small, trusted set of core functions. It has no access to networking, the file system, or other sensitive host resources.
*   **Clear Security Boundary**: All potentially risky operations (any form of I/O) are handled exclusively by the `CapabilityMarketplace`. This provides a single place to enforce security policies, logging, and auditing for external interactions.
*   **Sandboxing**: By design, all `Local` capabilities are sandboxed within the `CapabilityRegistry`.

## 6. Extensibility

The system is designed to be easily extended, particularly with new remote capabilities. To add a new HTTP-based capability, a developer only needs to:

1.  Define a `CapabilityManifest` for the new capability.
2.  Populate its `provider_type` field with a `ProviderType::Http` variant containing the appropriate `HttpCapability` configuration.
3.  Register this manifest with the `CapabilityMarketplace` at startup.

No changes are needed to the secure `CapabilityRegistry` to add new remote functionalities.
