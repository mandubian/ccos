# MCP Modules Comparison & Unification Plan

## Current State: Three Overlapping Modules

### 1. `planner/modular_planner/resolution/mcp.rs` (Resolution Strategy)

**Purpose**: Intent → Capability Resolution
- **Trait**: Implements `ResolutionStrategy`
- **Key Struct**: `McpResolution` + `RuntimeMcpDiscovery`
- **Focus**: Scoring tools against intents, registering in marketplace
- **Unique Features**:
  - Intent scoring (keyword + embedding-based)
  - File-based caching (`{server}_tools.json`)
  - Domain hint → server mapping
  - Automatic marketplace registration

**What it does**:
1. Takes a `SubIntent` (e.g., "List issues from GitHub")
2. Maps domain hint → MCP server
3. Discovers tools from server
4. Scores tools against intent
5. Returns best match as `ResolvedCapability`

### 2. `capability_marketplace/mcp_discovery.rs` (Marketplace Discovery)

**Purpose**: Capability Discovery for Marketplace
- **Trait**: Implements `CapabilityDiscovery`
- **Key Struct**: `MCPDiscoveryProvider`
- **Focus**: Discovering and persisting capabilities
- **Unique Features**:
  - RTFS module save/load (multiple capabilities in one file)
  - Effects derivation from metadata
  - Resource discovery (not just tools)
  - Output schema introspection (via `MCPIntrospector`)

**What it does**:
1. Discovers tools/resources from MCP servers
2. Converts to `CapabilityManifest`
3. Optionally saves to RTFS module files
4. Registers in marketplace

### 3. `synthesis/mcp_introspector.rs` (Introspection & RTFS Generation)

**Purpose**: Low-level Introspection & RTFS Code Generation
- **Key Struct**: `MCPIntrospector`
- **Focus**: Schema conversion, RTFS generation
- **Unique Features**:
  - JSON Schema → RTFS `TypeExpr` conversion
  - Output schema introspection (calls tools with safe inputs)
  - RTFS capability string generation
  - Individual capability file saving

**What it does**:
1. Parses MCP tools from JSON-RPC responses
2. Converts JSON Schema → RTFS types
3. Optionally introspects output schemas
4. Generates RTFS code strings

## Overlap Analysis

### Shared Functionality (All Three)
1. **Tool Discovery**: All call `tools/list` via MCP protocol
2. **Session Management**: All use `MCPSessionManager` (but sometimes create new instances)
3. **Schema Conversion**: All convert JSON Schema → RTFS `TypeExpr` (logic duplicated)
4. **Manifest Creation**: All create `CapabilityManifest` from MCP tools

### Redundant Code
- **Tool Discovery**: Implemented 3 times with slight variations
- **Session Setup**: Each module creates its own session manager
- **Schema Conversion**: `mcp_introspector.rs` has the most complete version
- **Cache Management**: Only `mcp.rs` has file caching, others could benefit

## Proposed Solution: Unified Core API

### New Module: `ccos/src/mcp/`

**Structure**:
```
mcp/
├── mod.rs          # Public API
├── core.rs         # Unified discovery service
├── types.rs        # Re-exports existing types
└── cache.rs        # Shared caching layer
```

### Core API (`mcp/core.rs`)

```rust
pub struct MCPDiscoveryService {
    session_manager: Arc<MCPSessionManager>,
    config_discovery: LocalConfigMcpDiscovery,
    cache: Option<MCPCache>,
    introspector: MCPIntrospector,
    /// Optional marketplace for automatic registration
    marketplace: Option<Arc<CapabilityMarketplace>>,
    /// Optional catalog for automatic indexing
    catalog: Option<Arc<CatalogService>>,
}

impl MCPDiscoveryService {
    /// Discover tools from a server (with caching)
    pub async fn discover_tools(
        &self,
        server_config: &MCPServerConfig,
        options: &DiscoveryOptions,
    ) -> RuntimeResult<Vec<DiscoveredMCPTool>>;
    
    /// Convert tool to capability manifest
    pub fn tool_to_manifest(
        &self,
        tool: &DiscoveredMCPTool,
        server_config: &MCPServerConfig,
    ) -> CapabilityManifest;
    
    /// Register discovered capability (marketplace + catalog)
    pub async fn register_capability(
        &self,
        manifest: &CapabilityManifest,
    ) -> RuntimeResult<()>;
    
    /// Get server config for domain hint
    pub fn get_server_for_domain(&self, domain: &DomainHint) -> Option<MCPServerConfig>;
    
    /// List all known servers
    pub fn list_known_servers(&self) -> Vec<MCPServerConfig>;
}
```

### Catalog Integration

The unified API automatically handles catalog indexing when capabilities are discovered:
- When `register_capability()` is called, it:
  1. Registers in marketplace (if provided)
  2. Indexes in catalog with `CatalogSource::Discovered` (if provided)
- This ensures all discovered MCP capabilities are searchable via the catalog

### Refactored Modules

1. **`mcp.rs` (Resolution)**: Uses `MCPDiscoveryService` for discovery, adds intent scoring
2. **`mcp_discovery.rs` (Marketplace)**: Uses `MCPDiscoveryService` for discovery, adds RTFS persistence
3. **`mcp_introspector.rs` (Introspection)**: Uses `MCPDiscoveryService` for discovery, focuses on schema introspection

## Benefits

1. **Single Source of Truth**: One implementation of core discovery logic
2. **Consistent Caching**: All modules share the same cache
3. **Better Performance**: Shared session pool, fewer redundant calls
4. **Easier Maintenance**: Fix bugs in one place
5. **Better Testing**: Test core once, all modules benefit

## Migration Path

1. **Phase 1**: Create `mcp/core.rs` with unified discovery
2. **Phase 2**: Update each module to use the core
3. **Phase 3**: Remove duplicate code
4. **Phase 4**: Add shared caching layer

