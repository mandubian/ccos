# Capability Serialization & Session Routing Architecture

> **Guide Level**: Intermediate  
> **Audience**: System integrators, capability developers  
> **Related**: [Serialization Guide](capability-serialization-guide.md), [Session Management](session-management-architecture.md)

## Quick Reference

For API details and code examples, see **[Capability Serialization Guide](capability-serialization-guide.md)**.

This guide focuses on **how data flows** through the serialization and execution pipelines.

---

## System Architecture Overview

```
                    Runtime Marketplace
                    ═══════════════════
               ┌─────────────────────────┐
               │ Arc<RwLock<HashMap>>    │
               │ {cap_id → Capability}   │
               └────────┬────────────────┘
                        │
       ┌────────────────┼────────────────┐
       ▼                ▼                ▼
   EXPORT           EXECUTE          IMPORT
   ══════           ═══════          ══════
   │ HTTP │         │MCP │       ┌─────────┐
   │ MCP  │ ─────→  │HTTP│──────→│RTFS Dir │
   └──────┘   via   │   │        │ or JSON │
   (in-mem)  Session │   │        └─────────┘
             Pool    │   │
                     └───┘
                    Router
               (metadata-driven)
```

Three primary flows:
1. **Export** → Serialize in-memory capabilities to RTFS files
2. **Execute** → Route requests with optional session management
3. **Import** → Parse RTFS files back into in-memory marketplace

---

## Export Flow (Serialization)

### Entry Point
```rust
marketplace.export_capabilities_to_rtfs_dir("/path/to/caps")
```

### Step-by-Step Process

#### 1. Filter Capabilities
```
For each capability in Arc<RwLock<HashMap>>:
  ├─ Serializable (kept):
  │  ├─ Http       → :provider :Http
  │  ├─ Mcp        → :provider :Mcp
  │  ├─ A2A        → :provider :A2a
  │  └─ RemoteRtfs → :provider :RemoteRtfs
  │
  └─ Non-Serializable (skipped):
     ├─ Local      (function pointers)
     ├─ Stream     (channel handles)
     ├─ Registry   (registry handles)
     └─ Plugin     (plugin handles)
```

#### 2. Generate RTFS Files
For each serializable capability:

```rtfs
:module
  :type "rtfs/capabilities/marketplace-snapshot"
  :version "1.0.0"
  :generated-at "2025-10-26T10:00:00Z"
  
  :capabilities [
    {
      :id "weather_api"
      :name "OpenWeatherMap"
      :version "1.0.0"
      
      :provider :Http
      :provider-meta {
        :base-url "https://api.openweathermap.org"
        :timeout-ms 5000
        :auth-token "..."
      }
      
      :input-schema [:map [:city :string] [:units :string]]
      :output-schema [:map [:temperature :float] [:condition :string]]
      
      :metadata {
        :api-version "2.5"
        :rate-limit "1000/day"
      }
      
      :permissions ["read:weather"]
      :effects ["call:external_api"]
    }
  ]
```

#### 3. Output Structure
```
/path/to/caps/
├─ weather_api.rtfs      (HTTP capability)
├─ github_mcp.rtfs       (MCP capability with session metadata)
├─ translator_a2a.rtfs   (A2A capability)
└─ remote_module.rtfs    (RemoteRTFS reference)
```

**Format Properties**:
- ✓ Human-readable (plain text RTFS)
- ✓ Editable (can modify before re-import)
- ✓ Homoiconic (code = data)
- ✓ Self-contained (includes all config)

---

## Import Flow (Deserialization)

### Entry Point
```rust
marketplace.import_capabilities_from_rtfs_dir("/path/to/caps")
```

### Step-by-Step Process

#### 1. Discovery
```
Scan directory for *.rtfs files
├─ Matches: weather_api.rtfs, github_mcp.rtfs, ...
└─ Skip: other_file.txt, README.md, etc.
```

#### 2. Parsing Strategy (Layered)

```
For each .rtfs file:
  │
  ├─→ Try: MCPDiscoveryProvider.load_rtfs_capabilities()
  │   (robust RTFS parser using full grammar)
  │   Success? → Go to Step 3
  │
  └─→ Fallback: Heuristic regex parsing
      (extract :provider, :provider-meta, schemas)
      Success? → Go to Step 3
      Failed?  → Log error, skip file
```

**Why layered approach?**
- Primary parser handles edge cases, full grammar validation
- Fallback handles human-edited files (minor formatting variations)
- Graceful degradation

#### 3. Manifest Reconstruction

Parse RTFS keywords → CapabilityManifest fields:

```
:provider :Http              → ProviderType::Http
:provider-meta { ... }       → HttpCapability { base_url, timeout_ms, auth_token }
:input-schema [:map [...]]   → TypeExpr::Map
:output-schema [...]         → TypeExpr
:metadata { ... }            → HashMap<String, String>
:permissions [...]           → Vec<String>
:effects [...]               → Vec<String>
```

#### 4. Re-registration

```rust
for manifest in parsed_manifests {
    marketplace.register_capability_manifest(manifest).await?;
}
// Each capability now in Arc<RwLock<HashMap>>
```

---

## Execute Flow with Session Routing

### Entry Point
```rust
marketplace.execute("github_mcp", json!({ "action": "get_issues", "repo": "..." }))
```

### Routing Decision Tree

```
marketplace.execute(cap_id, args)
│
├─→ Load capability manifest
│   ├─ Not found? → Err(CapabilityNotFound)
│   └─ Found? → continue
│
├─→ Check metadata for session keys
│   └─ Keys: *_requires_session, *_server_url, *_tool_name, etc.
│
├─→ Session metadata detected?
│   │
│   ├─ YES → Route to SessionPoolManager
│   │         marketplace.execute_with_session(cap_id, provider_type, args)
│   │
│   └─ NO  → Use standard executor
│             marketplace.execute_standard(cap_id, args)
```

### Session Management Flow (Detailed)

When `mcp_requires_session = "true"` in metadata:

```
SessionPoolManager
  │
  ├─→ Detect provider type (mcp_, graphql_, etc.)
  │   └─ Example: "mcp_*" prefix → Provider::Mcp
  │
  ├─→ Route to provider-specific handler
  │   └─ MCPSessionHandler for Mcp
  │
  └─→ MCPSessionHandler
       │
       ├─→ Get or create session
       │   ├─ Cache key: (cap_id, server_url) → session_id
       │   └─ Call MCPSessionManager.get_or_create_session()
       │
       ├─→ Execute tool call WITH session
       │   ├─ Add header: "Mcp-Session-Id: <session_id>"
       │   ├─ Call provider (HTTP POST to MCP server)
       │   └─ Return result
       │
       └─→ Session persists for next call
           └─ Same session_id reused for (cap_id, server_url) pair
```

**Session Lifecycle**:

```
Call 1: list_issues
  ├─ No session → Create with GitHub token
  ├─ Execute: GET /repos/{owner}/{repo}/issues
  └─ Session ID: sess_abc123

Call 2: create_issue  
  ├─ Session exists → Reuse
  ├─ Header: Mcp-Session-Id: sess_abc123
  ├─ Execute: POST /repos/{owner}/{repo}/issues
  └─ Auth token automatically included

Call 3: close_issue
  ├─ Session exists → Reuse
  ├─ Header: Mcp-Session-Id: sess_abc123
  ├─ Execute: PATCH /repos/{owner}/{repo}/issues/{id}
  └─ Auth token automatically included

Expiration: Session times out after 30 minutes of inactivity
  └─ Next call creates new session with fresh token
```

---

## RTFS Type System

Types in serialized capabilities use RTFS syntax:

### Primitives
```rtfs
:int        ; 32-bit integer
:float      ; 64-bit float
:string     ; Unicode string
:bool       ; Boolean
:nil        ; Null/None
:any        ; Any type (unrestricted)
```

### Collections
```rtfs
[:vector :string]                   ; List of strings
[:tuple :string :int :bool]         ; Typed tuple
[:map [:name :string] [:age :int]]  ; Object/dict (bracketed form)
[:map {:name :string :age :int}]    ; Object/dict (braced form)
```

### Optional Fields
```rtfs
[:map [:required :string] [:optional :int?]]
```

### Examples from Marketplace

```rtfs
;; HTTP API input
:input-schema [:map 
  [:city :string] 
  [:units :string]
]

;; MCP server response
:output-schema [:map 
  [:result :string] 
  [:status :string] 
  [:data :any]
]
```

---

## Serializable Provider Types

| Provider | Serializable | Export Key | Config Fields |
|----------|--------------|------------|---------------|
| **Http** | ✓ Yes | `:provider :Http` | `base_url`, `timeout_ms`, `auth_token` |
| **Mcp** | ✓ Yes | `:provider :Mcp` | `server_url`, `tool_name`, `timeout_ms` |
| **A2A** | ✓ Yes | `:provider :A2a` | `endpoint`, `namespace` |
| **RemoteRtfs** | ✓ Yes | `:provider :RemoteRtfs` | `module_url` |
| **Local** | ✗ No | (skipped) | Function pointers |
| **Stream** | ✗ No | (skipped) | Channel handles |
| **Registry** | ✗ No | (skipped) | Registry references |
| **Plugin** | ✗ No | (skipped) | Plugin handles |

**Non-serializable** providers are gracefully skipped during export. This allows:
- Selective export of portable capabilities
- Mix of local and portable capabilities in same marketplace
- No errors when exporting hybrid environments

---

## Round-Trip Integrity Guarantees

After export → inspect/edit → import, the following are preserved:

### HTTP Provider
```rtfs
BEFORE EXPORT:
  base_url: "https://api.openweathermap.org"
  timeout_ms: 5000
  auth_token: "sk-weather-token"

EXPORTED RTFS:
  :provider-meta {
    :base-url "https://api.openweathermap.org"
    :timeout-ms 5000
    :auth-token "sk-weather-token"
  }

AFTER IMPORT:
  ✓ base_url matches
  ✓ timeout_ms matches
  ✓ auth_token matches
```

### MCP Provider with Session Metadata
```rtfs
BEFORE EXPORT:
  server_url: "http://localhost:3001"
  tool_name: "github_operations"
  metadata["mcp_requires_session"]: "true"
  metadata["mcp_server_url"]: "http://localhost:3001"

EXPORTED RTFS:
  :provider-meta {
    :server-url "http://localhost:3001"
    :tool-name "github_operations"
  }
  :metadata {
    :mcp-requires-session "true"
    :mcp-server-url "http://localhost:3001"
  }

AFTER IMPORT:
  ✓ server_url matches
  ✓ tool_name matches
  ✓ Session routing metadata intact
  ✓ Session management re-enabled
```

---

## Implementation Architecture

### Core Files

**Serialization**:
- `src/ccos/capability_marketplace/marketplace.rs`
  - `export_capabilities_to_rtfs_dir()` - RTFS generation
  - `import_capabilities_from_rtfs_dir()` - RTFS parsing
  - `export_capabilities_to_file()` - JSON export
  - `import_capabilities_from_file()` - JSON import

**Schema Rendering**:
- `src/ccos/synthesis/schema_serializer.rs`
  - `type_expr_to_rtfs_pretty()` - Human-readable
  - `type_expr_to_rtfs_compact()` - Canonical

**RTFS Parsing**:
- `src/ccos/capability_marketplace/mcp_discovery.rs`
  - `parse_rtfs_module()` - RTFS parser
  - `rtfs_to_capability_manifest()` - Conversion

**Session Routing**:
- `src/ccos/capabilities/session_pool.rs`
  - `SessionPoolManager.execute_with_session()`
- `src/ccos/capabilities/mcp_session_handler.rs`
  - `MCPSessionHandler` - MCP implementation

---

## Key Design Principles

### 1. Homoiconicity ✓
Capabilities **are** RTFS structures (code = data), not serialized to them.

```rtfs
;; This is both:
;; - A complete capability definition
;; - A data structure that can be introspected
;; - A document that humans can read/edit
{:provider :Http :input-schema [...] :metadata {...}}
```

### 2. Native Format ✓
RTFS is **canonical** (not a translation from internal state).

- Export → RTFS modules
- JSON is optional snapshot for interop
- Schema types use RTFS syntax natively

### 3. Session-Aware ✓
Metadata drives runtime behavior.

```rtfs
:metadata {
  :mcp-requires-session "true"    ;; Triggers stateful routing
  :mcp-server-url "http://..."    ;; Server endpoint
  :mcp-tool-name "github_ops"     ;; Tool identifier
}
```

### 4. Human-Readable & Editable ✓
Export format is plain text, fully editable before re-import.

```bash
$ cat capabilities/github_mcp.rtfs
$ vim capabilities/github_mcp.rtfs
$ cargo run -- import capabilities/
```

### 5. Round-Trip Safe ✓
Export → edit → import preserves everything.

- Provider configuration intact
- Schemas preserved
- Permissions/effects maintained
- Session metadata carried through

### 6. Gracefully Degrading ✓
Non-serializable providers skipped without error.

```rust
// Marketplace with mix of providers
├─ http_api       → Exported ✓
├─ mcp_github     → Exported ✓
├─ local_closure  → Skipped (non-serializable)
├─ stream_service → Skipped (non-serializable)
└─ // Rest continue working normally
```

---

## Practical Workflows

### Workflow 1: Multi-Environment Sync

```
Dev Environment           Prod Environment
═══════════════           ════════════════
marketplace               marketplace
  ├─ http_api               ├─ http_api
  ├─ mcp_github             ├─ mcp_github
  └─ local_dev              └─ (missing)

# Export from dev
cargo run export-capabilities /tmp/caps

# Transfer /tmp/caps/http_api.rtfs and github_mcp.rtfs to prod

# Import into prod
cargo run import-capabilities /tmp/caps
# Result: prod now has http_api and mcp_github
```

### Workflow 2: Capability Versioning

```
capabilities/
├─ v1/
│  ├─ weather_api.rtfs     (base_url: v1)
│  └─ github_mcp.rtfs      (tool_name: v1)
├─ v2/
│  ├─ weather_api.rtfs     (base_url: v2, enhanced schema)
│  └─ github_mcp.rtfs      (tool_name: v2)
└─ current → v2/

# User can audit what changed between versions
diff v1/weather_api.rtfs v2/weather_api.rtfs
```

### Workflow 3: Emergency Recovery

```
# Last backup restored to /tmp/backup
# Current marketplace corrupted

marketplace = CapabilityMarketplace::new();
marketplace.import_capabilities_from_rtfs_dir("/tmp/backup").await?;
# Fully operational with last known good state
```

---

## Testing & Verification

**Unit Tests**:
```bash
cargo test --test roundtrip_export_import_tests -- --nocapture
```

Tests validate:
- HTTP capability fields preserved
- MCP capability fields + session metadata preserved
- Schema type expressions correct
- Non-serializable providers gracefully skipped

**Demo**:
```bash
./target/debug/demo_serialization
```

Shows:
- HTTP capability serialization
- MCP capability with session routing
- Complete round-trip example

---

## Next Steps & Extensions

### 1. OpenAPI Executor Support
- Create `OpenApiExecutor` (follows same export/import pattern)
- Add `:provider :OpenApi` to RTFS format
- Test round-trip with OpenAPI capabilities

### 2. Auto-Load on Bootstrap
- Wire `import_capabilities_from_rtfs_dir()` into `CCOSEnvironment::new()`
- Auto-restore capabilities on startup
- Configuration for RTFS directory path

### 3. CLI Tools
```bash
ccos export-capabilities --output ./caps
ccos import-capabilities --input ./caps
ccos diff-capabilities v1/weather.rtfs v2/weather.rtfs
```

### 4. Versioning & Migration
- Track capability schema versions
- Migration helpers for schema evolution
- Deprecation markers in metadata

### 5. Diff/Merge for Teams
- Compare RTFS files across branches
- Merge strategies for multi-contributor environments
- Conflict resolution tools

---

## See Also

- **[Capability Serialization Guide](capability-serialization-guide.md)** - API reference & code examples
- **[Session Management Architecture](session-management-architecture.md)** - Deep dive on session lifecycle
- **[Capability Providers Architecture](capability-providers-architecture.md)** - Provider type details
- **[RTFS 2.0 Syntax](../specs/rtfs-2.0-syntax.md)** - Type system reference
