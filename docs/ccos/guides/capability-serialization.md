# Capability Serialization & Marketplace Persistence

The CCOS marketplace provides native **RTFS and JSON serialization** for capabilities. This enables exporting registered capabilities (e.g., discovered MCP tools, synthesized APIs) into human-readable files that can be versioned, edited, and re-imported.

## 1. Exporting Capabilities

You can export the current state of the marketplace to a directory of RTFS files:

```rust
marketplace.export_capabilities_to_rtfs_dir("./capabilities/exported").await?;
```

### Serializable Providers
Only providers with external or declarative definitions are serializable:
- **HTTP**: Standard REST endpoints.
- **OpenAPI**: Discovered via introspection.
- **MCP**: Discovered Model Context Protocol tools.
- **A2A**: Agent-to-Agent endpoints.
- **RemoteRTFS**: References to remote executable RTFS modules.

*Non-serializable providers (like `Local` closures or `Stream` handles) are skipped during export.*

---

## 2. RTFS Capability Format

Exported capabilities use the `(capability ...)` macro format. This format is designed to be human-readable and compatible with the RTFS runtime.

### Example Snapshot (`weather.rtfs`)
```rtfs
;; Exported capability snapshot
(capability "weather.get_current.v1"
  :name "Current Weather"
  :version "1.0.0"
  :description "Fetch current weather conditions for a location"
  :provider :openapi
  :provider-meta {
    :base_url "https://api.openweathermap.org"
    :timeout_ms 5000
    :spec_url "https://api.openweathermap.org/data/2.5/weather"
    :operations 1
    :auth_type "api_key"
    :auth_location "header"
  }
  :metadata {
    :kind :primitive
    :category "environmental"
  }
  :permissions [:network:request]
  :effects [:network]
  :input-schema {:q :string :units :string?}
  :output-schema {:main {:temp :float} :weather [:vector {:main :string}]}
  :implementation
    (fn [input]
      "Exported manifest only; runtime-managed execution"
      input)
)
```

---

## 3. RTFS Type Expressions (Schemas)

Schemas for inputs and outputs use RTFS Type Expressions.

### Primitive Types
- `:int`, `:float`, `:string`, `:bool`, `:nil`, `:any`
- Append `?` for optional fields: `:string?`

### Collection Types
- **Maps**: `{:field1 :type1 :field2 :type2}`
- **Vectors**: `[:vector :type]`
- **Tuples**: `[:tuple :type1 :type2]`

### Advanced Types (IR Support)
- **Type Variables**: Used in generic capabilities.
- **Parametric Maps**: Maps with dynamic keys/values based on type parameters.

---

## 4. Persistent Storage (JSON)

While RTFS is used for human-readable snapshots, CCOS uses a JSON-based local storage for runtime persistence by default. This is managed by the `LocalConfigMcpDiscovery` and the `CapabilityMarketplace` internals.

- Capabilities are stored in `capabilities/generated/` or `capabilities/servers/approved/`.
- Large artifacts like Agent Memory use their own dedicated storage in `working_memory/storage/`.

---

## 5. Round-Trip Integrity

The goal of CCOS serialization is to ensure that a capability can be exported, modified (e.g., adding governance hints or manually refining schemas), and re-imported without losing its operational context.

### Metadata-Driven Execution
Capabilities often declare requirements via metadata:
- `:mcp_requires_session "true"`: Instructs the orchestrator to route this through the `SessionPoolManager`.
- `:kind :agent`: Informs the `DelegatingCognitiveEngine` that this artifact has autonomy (Unified Artifact Model).
