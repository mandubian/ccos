# CCOS Marketplace Serialization Summary

> **Quick Start**: For architecture diagrams and data flow overview, see  
> **[Capability Serialization & Session Routing Architecture](capability-serialization-architecture.md)**

## Overview

The CCOS marketplace provides **native RTFS serialization** for capabilities, enabling:

- **Export**: Save registered capabilities to RTFS files (human-readable, editable)
- **Import**: Load capabilities from RTFS files back into the marketplace
- **Session Preservation**: Metadata enables stateful MCP sessions (e.g., GitHub MCP)
- **Round-Trip Integrity**: Export→Edit→Import preserves all configuration

## RTFS Type Syntax (Corrected)

Schemas use proper RTFS type expressions:

### Primitive Types
```rtfs
:int       ; Integer
:float     ; Floating-point number
:string    ; String
:bool      ; Boolean
:nil       ; Null
:any       ; Any type
```

### Collection Types
```rtfs
[:vector :string]                              ; Vector of strings
[:tuple :string :int]                          ; Tuple: (string, int)
[:map [:name :string] [:age :int]]             ; Map: {name: string, age: int}
[:map {:name :string :age :int}]               ; Map: alternate braced syntax
```

### Optional Fields
```rtfs
[:map [:name :string] [:expand :bool?]]        ; Optional fields use ? suffix
;; or braced form:
[:map {:name :string :expand :bool?}]
```

## Capability RTFS Format

Exported capabilities are RTFS modules with this structure:

```rtfs
:module
  :type "rtfs/capabilities/marketplace-snapshot"
  :version "1.0.0"
  :generated-at "2025-10-25T14:32:10Z"
  
  :capabilities [
    {:id "capability_id"
     :name "Display Name"
     :version "1.0.0"
     :description "What this does"
     
     :provider :Http
     :provider-meta {
       :base-url "https://..."
       :timeout-ms 5000
       :auth-token "..."
     }
     
     :input-schema [:map [:param1 :string] [:param2 :int]]
     
     :output-schema [:map [:result :string] [:status :bool]]
     
     :metadata {
       :custom-key "custom-value"
       :mcp-requires-session "true"  ;; For MCP session management
       :mcp-server-url "http://..."   ;; MCP server endpoint
       :mcp-tool-name "..."           ;; MCP tool identifier
     }
     
     :permissions ["read:weather" "write:data"]
     :effects ["call:external_api" "maintain:session"]
    }
  ]
```

## Provider Types

### Serializable Providers
1. **Http** - REST API endpoints
   - `:provider-meta { :base-url :string :timeout-ms :int :auth-token :string }`

2. **Mcp** - Model Context Protocol servers
   - `:provider-meta { :server-url :string :tool-name :string :timeout-ms :int }`

3. **A2A** - Agent-to-agent communication
   - `:provider-meta { :endpoint :string :namespace :string }`

4. **RemoteRTFS** - Remote RTFS modules
   - `:provider-meta { :module-url :string }`

### Non-Serializable Providers (Gracefully Skipped)
- **Local** - Closures/lambdas (can't serialize function pointers)
- **Stream** - Channels/async streams (runtime-specific)
- **Registry** - External capability registry handles
- **Plugin** - Plugin handles (runtime-specific)

## API Usage

### Export capabilities to RTFS files
```rust
let count = marketplace
  .export_capabilities_to_rtfs_dir("/tmp/capabilities")
  .await?;
// Generates: weather_api.rtfs, github_mcp.rtfs, etc.
```

### Import capabilities from RTFS directory
```rust
let count = marketplace
  .import_capabilities_from_rtfs_dir("/tmp/capabilities")
  .await?;
// Loads all .rtfs files and registers capabilities
```

### Export to JSON (portable snapshot)
```rust
marketplace
  .export_capabilities_to_file("capabilities.json")
  .await?;
```

### Import from JSON
```rust
marketplace
  .import_capabilities_from_file("capabilities.json")
  .await?;
```

## Session Management

For stateful providers like MCP (GitHub API), metadata carries session requirements:

```rtfs
:metadata {
  :mcp-requires-session "true"          ;; Trigger session management
  :mcp-server-url "http://localhost:3001"
  :mcp-tool-name "github_operations"
}
```

At runtime:
1. Marketplace detects metadata with `_requires_session` suffix
2. Routes to `SessionPoolManager`
3. SessionPoolManager detects provider type (mcp_, graphql_, etc.)
4. Gets/creates session via provider-specific handler
5. Maintains session ID across tool calls (e.g., list→create→close)

## Implementation Files

- **src/ccos/capability_marketplace/marketplace.rs**
  - `export_capabilities_to_rtfs_dir()` - Generate RTFS files
  - `import_capabilities_from_rtfs_dir()` - Parse and load RTFS files
  - `export_capabilities_to_file()` - JSON snapshot
  - `import_capabilities_from_file()` - Load from JSON

- **src/ccos/synthesis/schema_serializer.rs**
  - `type_expr_to_rtfs_pretty()` - Human-readable schema rendering
  - `type_expr_to_rtfs_compact()` - Compact format with brackets

- **src/ccos/capability_marketplace/mcp_discovery.rs**
  - `parse_rtfs_module()` - Robust RTFS parsing
  - `rtfs_to_capability_manifest()` - Convert RTFS to CapabilityManifest

- **src/ccos/capabilities/session_pool.rs**
  - `SessionPoolManager` - Routes session requests by provider type

- **src/ccos/capabilities/mcp_session_handler.rs**
  - `MCPSessionHandler` - MCP-specific session management

## Key Principles

1. **Homoiconicity**: Capabilities ARE RTFS structures (code = data)
2. **Native Format**: RTFS is the canonical serialization (not a translation)
3. **Human-Readable**: Can be edited manually in text editors
4. **Round-Trip Safe**: Export→Edit→Import preserves all fields
5. **Session-Aware**: Metadata drives stateful provider routing
6. **Extensible**: New provider types can be added following the pattern

## Example: Complete Round-Trip

```rust
// 1. Create and register capability
let cap = CapabilityManifest {
    id: "weather_api".to_string(),
    name: "Weather API".to_string(),
    provider: ProviderType::Http(HttpCapability {
        base_url: "https://api.openweathermap.org".to_string(),
        timeout_ms: 5000,
        auth_token: Some("token".to_string()),
    }),
    // ... other fields
};
marketplace.register_capability_manifest(cap).await;

// 2. Export to RTFS
marketplace.export_capabilities_to_rtfs_dir("/caps").await?;
// Creates: /caps/weather_api.rtfs

// 3. User can inspect/edit the file
// $ cat /caps/weather_api.rtfs
// $ vim /caps/weather_api.rtfs

// 4. Load into new marketplace
let mut new_marketplace = CapabilityMarketplace::new();
new_marketplace.import_capabilities_from_rtfs_dir("/caps").await?;

// 5. Capability fully reconstructed
if let Some(reloaded) = new_marketplace.get_capability("weather_api").await {
    println!("Reloaded: {}", reloaded.name);
    if let ProviderType::Http(http) = &reloaded.provider {
        println!("URL: {}", http.base_url);           // Preserved!
        println!("Auth: {}", http.auth_token.is_some()); // Preserved!
    }
}
```

## Testing

Run the demonstration:
```bash
./target/debug/demo_serialization
```

Run comprehensive round-trip tests:
```bash
cargo test --test roundtrip_export_import_tests -- --nocapture
```

## Related Documentation

- [RTFS 2.0 Syntax](../specs/02-syntax-and-grammar.md)
- [Capability Marketplace Architecture](../specs/)
- [Session Management for Stateful MCPs](session-management-architecture.md)

---

## Architecture Guide

For detailed architecture, data flows, and visual diagrams, see:  
**[Capability Serialization & Session Routing Architecture](capability-serialization-architecture.md)**

This guide covers:
- System overview with ASCII diagrams
- Step-by-step export/import/execute flows
- Session lifecycle and routing decisions
- Round-trip integrity guarantees
- Practical workflows and examples
