# Capability Synthesis & API Introspection

Capability Synthesis is the process by which CCOS automatically generates RTFS capabilities from external specifications like **OpenAPI**, **Model Context Protocol (MCP)**, or **GraphQL**.

## 1. Synthesis Workflows

### API Introspection (OpenAPI)
CCOS can crawl an OpenAPI/Swagger endpoint and generate one specialized capability per API operation.
- **Tool**: `api_introspector.rs`
- **Output**: Typed `CapabilityManifests` with RTFS implementations that use `ccos.network.http-fetch`.

### MCP Discovery (Tool Synthesis)
MCP servers export a list of tools. CCOS connects to these servers, parses the tool definitions, and registers them in the marketplace.
- **Tool**: `mcp_introspector.rs`
- **Output**: Capabilities with `ProviderType::MCP` that route calls through the `SessionPoolManager`.

---

## 2. Using the Synthesizer Programmatically

The `CapabilitySynthesizer` provides high-level methods to ingest external APIs.

```rust
use ccos::synthesis::dialogue::capability_synthesizer::CapabilitySynthesizer;

let synthesizer = CapabilitySynthesizer::new(auth_token);

// Synthesize from an OpenAPI spec
let results = synthesizer
    .synthesize_from_api_introspection("https://api.github.com", "github")
    .await?;

// Register results in marketplace
for cap_result in results {
    marketplace.register_capability(cap_result.manifest).await?;
}
```

---

## 3. Schema Transformation

A critical part of synthesis is converting **JSON Schema** (used by OpenAPI/MCP) into **RTFS Type Expressions**.

| JSON Schema | RTFS TypeExpr |
| :--- | :--- |
| `{"type": "string"}` | `:string` |
| `{"type": "integer"}` | `:int` |
| `{"type": "object", "properties": {...}}` | `{:key :type ...}` |
| `{"type": "array", "items": {...}}` | `[:vector :type]` |

Optional fields are handled via the `?` suffix or `[:optional :type]` wrappers.

---

## 4. Generated RTFS Format

Synthesized capabilities are stored as `.rtfs` files in `capabilities/servers/approved/` or `capabilities/generated/`.

### Example: GitHub `list_issues`
```rtfs
(capability "mcp.github.list_issues"
  :name "List Issues"
  :description "List issues in a GitHub repository"
  :provider :mcp
  :provider-meta {
    :server_url "https://api.githubcopilot.com/mcp/"
    :tool_name "list_issues"
    :requires_session "true"
  }
  :input-schema {
    :owner :string
    :repo :string
    :state [:optional :string]
  }
  :output-schema :any
  :implementation
    (fn [input]
      ;; Implementation is handled by the MCPExecutor
      input)
)
```

## 5. Multi-Capability Results

For complex APIs with many endpoints, the synthesizer returns a `MultiCapabilitySynthesisResult`. This allows CCOS to:
1.  **Group** related capabilities under a common namespace.
2.  **Shared Authentication**: Apply the same auth credentials to all endpoints in the set.
3.  **Atomic Registration**: Ensure all endpoints are registered or none (preventing fragmented API support).
