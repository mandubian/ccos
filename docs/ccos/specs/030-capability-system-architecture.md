# CCOS Capability System Architecture

**Status**: Authoritative  
**Version**: 1.0  
**Last Updated**: 2025-11-29  
**Scope**: Complete capability system including types, discovery, resolution, and lifecycle management

---

## 1. Overview

The CCOS Capability System provides a unified framework for discovering, registering, executing, and managing capabilities from multiple sources. A **capability** is a discrete, well-typed unit of functionality that can be invoked from RTFS plans.

### Design Principles

1. **Provider-Agnostic**: Capabilities from any source (MCP, OpenAPI, A2A, local) share the same manifest structure
2. **RTFS-First**: All capabilities are expressible as RTFS code, enabling serialization and portability
3. **Schema-Driven**: Input/output schemas define contracts and enable validation
4. **Discovery-Oriented**: The system proactively finds capabilities rather than requiring manual registration
5. **Security-Aware**: Governance, attestation, and trust tiers control capability access

---

## 2. Core Types

### 2.1 CapabilityManifest

The central data structure representing a registered capability:

```rust
pub struct CapabilityManifest {
    // Identity
    pub id: String,                           // Unique ID: "mcp.github.list_issues"
    pub name: String,                         // Human name: "list_issues"
    pub description: String,                  // What it does
    pub version: String,                      // Semantic version: "1.2.3"
    
    // Provider
    pub provider: ProviderType,               // How to execute it
    
    // Schema
    pub input_schema: Option<TypeExpr>,       // RTFS type for inputs
    pub output_schema: Option<TypeExpr>,      // RTFS type for outputs
    
    // Trust & Provenance
    pub attestation: Option<CapabilityAttestation>,
    pub provenance: Option<CapabilityProvenance>,
    
    // Security
    pub permissions: Vec<String>,             // Required permissions
    pub effects: Vec<String>,                 // Side effects declared
    
    // Classification (NEW)
    pub domains: Vec<String>,                 // Hierarchical domains: ["github", "github.issues"]
    pub categories: Vec<String>,              // Operation types: ["crud.read", "search"]
    
    // Metadata
    pub metadata: HashMap<String, String>,    // Extensible key-value pairs
    pub agent_metadata: Option<AgentMetadata>, // For agent capabilities
}
```

### 2.2 ProviderType

Defines how a capability is executed:

```rust
pub enum ProviderType {
    // Local execution (Rust handler)
    Local(LocalCapability),
    
    // HTTP API call
    Http(HttpCapability),
    
    // Model Context Protocol server
    MCP(MCPCapability),
    
    // Agent-to-Agent protocol
    A2A(A2ACapability),
    
    // OpenAPI specification
    OpenApi(OpenApiCapability),
    
    // Plugin system
    Plugin(PluginCapability),
    
    // Remote RTFS execution
    RemoteRTFS(RemoteRTFSCapability),
    
    // Streaming capability
    Stream(StreamCapabilityImpl),
    
    // Registry-backed capability
    Registry(RegistryCapability),
}
```

#### Provider Details

| Provider | Use Case | Transport | Schema Source |
|----------|----------|-----------|---------------|
| `MCP` | MCP server tools | HTTP SSE/JSON-RPC | `tools/list` response |
| `OpenApi` | REST APIs | HTTP | OpenAPI spec |
| `A2A` | Agent-to-Agent | HTTP/WebSocket | A2A agent card |
| `Http` | Generic HTTP | HTTP | Manual definition |
| `Local` | In-process Rust | Direct call | Code definition |
| `Native` | Built-in CCOS capabilities | Direct call | Code definition |
| `RemoteRTFS` | Federated CCOS | HTTP | Remote manifest |
| `Stream` | Streaming data | Various | Manual definition |
| `Plugin` | Native plugins | FFI | Plugin manifest |

### 2.4 Native Capabilities

Native capabilities are built-in CCOS capabilities implemented in Rust and registered during CCOS initialization. They include:

| Capability | Description | Effects |
|------------|-------------|---------|
| `ccos.llm.generate` | LLM text generation with prompt sanitization (summarization, analysis) | `llm`, `network` |
| `ccos.io.println` | Print output to console | `io` |
| `ccos.cli.*` | CLI command capabilities | `io`, `read` |
| `ccos.config.*` | Configuration access | `read` |

Native capabilities are registered via `NativeCapabilityProvider` and `ops::native::register_native_capabilities()`.

### 2.3 Domain and Category Taxonomy

Capabilities are classified using flexible, extensible taxonomies:

**Domains** (hierarchical, dot-notation):
- `github` → `github.issues`, `github.repos`, `github.pull_requests`
- `slack` → `slack.messages`, `slack.channels`
- `cloud.aws` → `cloud.aws.s3`, `cloud.aws.lambda`
- `database` → `database.postgres`, `database.mysql`
- `filesystem` → `filesystem.files`, `filesystem.directories`

**Categories** (operation types):
- `crud.read` - Read operations (list, get, fetch)
- `crud.create` - Create operations (create, add, post)
- `crud.update` - Update operations (update, edit, patch)
- `crud.delete` - Delete operations (delete, remove)
- `search` - Search operations
- `notify` - Notification operations
- `transform` - Data transformation
- `validate` - Validation operations

#### Automatic Inference

Domains and categories are automatically inferred from:

1. **Server/source name** → Primary domain
   - `"modelcontextprotocol/github"` → `"github"`
   - `"aws-s3-server"` → `"cloud.aws"`

2. **Capability name** → Sub-domain
   - `"list_issues"` → `"issues"`
   - `"send_message"` → `"messages"`

3. **Action prefix** → Category
   - `"list_*"` → `"crud.read"`
   - `"create_*"` → `"crud.create"`
   - `"search_*"` → `"search"`

---

## 3. Storage and Indexing

### 3.1 CapabilityMarketplace

The primary registry for capabilities:

```rust
pub struct CapabilityMarketplace {
    // In-memory capability storage
    capabilities: Arc<RwLock<HashMap<String, CapabilityManifest>>>,
    
    // Discovery agents
    discovery_agents: Vec<Box<dyn CapabilityDiscovery>>,
    
    // Integration points
    capability_registry: Arc<RwLock<CapabilityRegistry>>,
    catalog: Arc<RwLock<Option<Arc<CatalogService>>>>,
    
    // Execution support
    executor_registry: HashMap<TypeId, ExecutorVariant>,
    isolation_policy: CapabilityIsolationPolicy,
    causal_chain: Option<Arc<Mutex<CausalChain>>>,
    resource_monitor: Option<Arc<ResourceMonitor>>,
}
```

### 3.2 CatalogService

Secondary index for search and discovery:

```rust
pub struct CatalogEntry {
    pub id: String,
    pub kind: CatalogEntryKind,        // Capability or Plan
    pub source: CatalogSource,          // Discovered, Generated, User, System
    pub location: Option<CatalogLocation>,
    pub description: String,
    pub input_signature_hash: u64,
    pub output_signature_hash: u64,
    
    // Classification
    pub domains: Vec<String>,
    pub categories: Vec<String>,
}

pub struct CatalogFilter {
    pub kind: Option<CatalogEntryKind>,
    pub source: Option<CatalogSource>,
    pub domains: Vec<String>,
    pub categories: Vec<String>,
}
```

### 3.3 RTFS File Storage

Capabilities can be persisted as RTFS files:

```
capabilities/
├── core/                    # System capabilities
│   └── io.rtfs
├── discovered/              # Auto-discovered
│   └── mcp/
│       ├── github/
│       │   ├── list_issues.rtfs
│       │   └── create_issue.rtfs
│       └── slack/
│           └── send_message.rtfs
├── generated/               # Synthesized capabilities
│   └── custom/
└── user/                    # User-defined
```

---

## 4. Capability Lifecycle

### 4.1 Discovery → Registration → Execution Flow

```
┌─────────────┐     ┌──────────────┐     ┌─────────────────┐
│  Discovery  │────▶│ Registration │────▶│    Execution    │
│   Sources   │     │   Pipeline   │     │                 │
└─────────────┘     └──────────────┘     └─────────────────┘
      │                    │                      │
      ▼                    ▼                      ▼
 ┌─────────┐         ┌──────────┐          ┌──────────┐
 │ MCP     │         │ Validate │          │ Provider │
 │ OpenAPI │         │ Govern   │          │ Execute  │
 │ A2A     │         │ Catalog  │          │ Cache    │
 │ Web     │         │ Version  │          │ Monitor  │
 └─────────┘         └──────────┘          └──────────┘
```

### 4.2 Registration Pipeline

1. **Manifest Creation**: Build `CapabilityManifest` from source
2. **Domain/Category Inference**: Automatically classify
3. **Validation**: Schema validation, static analysis
4. **Governance Check**: Apply policies (trust, permissions)
5. **Version Comparison**: Check for breaking changes
6. **Marketplace Registration**: Store in memory
7. **Catalog Indexing**: Index for search
8. **RTFS Export** (optional): Persist to filesystem

### 4.3 Versioning and Updates

```rust
pub enum VersionComparison {
    Equal,
    PatchUpdate,    // 1.0.0 → 1.0.1 (bug fixes)
    MinorUpdate,    // 1.0.0 → 1.1.0 (backward-compatible)
    MajorUpdate,    // 1.0.0 → 2.0.0 (breaking changes)
    Downgrade,      // 1.1.0 → 1.0.0
}

pub struct UpdateResult {
    pub updated: bool,
    pub previous_version: Option<String>,
    pub version_comparison: VersionComparison,
    pub breaking_changes: Vec<String>,
}
```

Breaking changes are detected when:
- Major version increases
- Input/output schemas change incompatibly
- Effects or permissions broaden (security concern)

---

## 5. Integration Points

### 5.1 With Planner

The planner discovers and resolves capabilities during plan decomposition:

```rust
// SubIntent with domain hints
pub struct SubIntent {
    pub id: String,
    pub description: String,
    pub intent_type: IntentType,
    pub domain_hint: Option<DomainHint>,  // Used for capability search
    pub arguments: HashMap<String, String>,
    pub dependencies: Vec<String>,
}

// Resolution uses domains for filtering
let filter = CatalogFilter::for_domain("github.issues")
    .with_category("crud.read");
let candidates = catalog.search(&filter);
```

### 5.2 With Governance

Capabilities are subject to governance policies:

- **Trust Tiers**: Capabilities have trust levels affecting execution
- **Permission Checks**: `RuntimeContext::is_capability_allowed`
- **Effect Validation**: Side effects must be declared and approved
- **Attestation**: Signed capabilities from trusted sources

### 5.3 With Causal Chain

All capability operations are logged for auditability:

- Registration events
- Execution events
- Version updates
- Governance decisions

---

## 6. Configuration

### 6.1 Feature Flags

```bash
# Master switch for missing capability resolution
CCOS_MISSING_CAPABILITY_ENABLED=true

# Auto-resolution at runtime
CCOS_AUTO_RESOLUTION_ENABLED=true

# MCP registry access
CCOS_MCP_REGISTRY_ENABLED=true

# Human approval requirement
CCOS_HUMAN_APPROVAL_REQUIRED=true
```

### 6.2 Discovery Options

```rust
pub struct DiscoveryOptions {
    pub introspect_output_schemas: bool,  // Expensive, disabled by default
    pub use_cache: bool,                   // Use cached results
    pub register_in_marketplace: bool,     // Auto-register
    pub export_to_rtfs: bool,              // Persist to files
    pub max_parallel_discoveries: usize,   // Concurrency limit (default: 5)
    pub lazy_output_schemas: bool,         // Skip introspection (default: true)
}
```

---

## 7. File Locations

| Component | Location |
|-----------|----------|
| Capability Types | `ccos/src/capability_marketplace/types.rs` |
| Marketplace | `ccos/src/capability_marketplace/marketplace.rs` |
| Versioning | `ccos/src/capability_marketplace/versioning.rs` |
| Catalog | `ccos/src/catalog/mod.rs` |
| MCP Discovery | `ccos/src/mcp/core.rs` |
| OpenAPI Importer | `ccos/src/synthesis/importers/openapi_importer.rs` |
| GraphQL Importer | `ccos/src/synthesis/importers/graphql_importer.rs` |
| Missing Resolver | `ccos/src/synthesis/core/missing_capability_resolver.rs` |
| Registration | `ccos/src/synthesis/registration/registration_flow.rs` |

---

## 8. See Also

- [031-mcp-discovery-unified-service.md](./031-mcp-discovery-unified-service.md) - MCP Discovery details
- [032-missing-capability-resolution.md](./032-missing-capability-resolution.md) - Resolution system
- [033-capability-importers-and-synthesis.md](./033-capability-importers-and-synthesis.md) - Importers
