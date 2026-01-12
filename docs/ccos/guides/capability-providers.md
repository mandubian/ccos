# Capability Providers Architecture

CCOS uses a decoupled Provider Architecture. A **Capability Manifest** declares *what* an artifact does (schemas, permissions), while the **Provider Type** defines *how* it is executed.

## 1. Provider Types

The marketplace supports several execution engines:

| Provider | Type | Description |
| :--- | :--- | :--- |
| **Local** | `Local(LocalCapability)` | Executed as a direct Rust closure in the host process. |
| **HTTP** | `Http(HttpCapability)` | Sends requests to standard REST API endpoints. |
| **OpenAPI** | `OpenApi(OpenApiCapability)` | Specialized for introspected services with rich metadata. |
| **MCP** | `MCP(MCPCapability)` | Connects to Model Context Protocol servers (via HTTP/SSE). |
| **A2A** | `A2A(A2ACapability)` | Routes communication between different CCOS agents. |
| **Native** | `Native(NativeCapability)` | Built-in CCOS primitives (filesystem, system info, agent-ops). |
| **Sandboxed** | `Sandboxed(SandboxedCapability)` | Runs code in isolated environments (GVisor, Firecracker, WASM). |
| **RemoteRTFS** | `RemoteRTFS(...)` | Calls a capability hosted by a remote RTFS runtime. |
| **Registry** | `Registry(...)` | Meta-capability for discovering other providers. |

---

## 2. Execution Flow

The `Orchestrator` resolves a capability call through the `CapabilityMarketplace`:

1.  **Lookup**: Find the `CapabilityManifest` by ID.
2.  **Routing**: The marketplace selects the corresponding `Executor` based on the `ProviderType`.
3.  **Governance**: The `GovernanceKernel` checks permissions/limits before the executor is called.
4.  **Execution**: The executor (e.g., `MCPExecutor` or `HttpExecutor`) handles the data transformation and IO.

### RTFS-Based Hybrid Execution
Some capabilities (especially synthesized ones) use a hybrid approach:
- The **Manifest** points to a `Local` provider as a placeholder.
- The **Implementation** field contains the actual logic as an RTFS function.
- The **Evaluator** executes the RTFS code, which in turn calls low-level primitives (like `ccos.network.http-fetch`).

---

## 3. Sandboxing & Security

CCOS supports multi-tier isolation for execution:

- **Host Process**: (Local/Native) Highest performance, lowest isolation.
- **Micro-VM** (Sandboxed): Uses Firecracker or GVisor to run arbitrary code with hardware/kernel-level isolation.
- **Purity Enforcement**: RTFS expressions are analyzed for effects (IO, Network) and can be restricted to "pure" subsets by the interpreter.

---

## 4. MCP Integration

MCP (Model Context Protocol) is the primary way CCOS interacts with the external world (GitHub, Google, etc.). 
- **Dynamic Discovery**: MCP providers can be discovered at runtime via environment variables or the `approved/` server directory.
- **Session Pooling**: MCP calls requiring authentication are routed through the `SessionPoolManager` to reuse connections.
